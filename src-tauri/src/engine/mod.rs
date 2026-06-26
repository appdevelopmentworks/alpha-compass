//! Compute engine — market-regime composite.
//!
//! Loads time-series from the store, builds the §8 signals (sign-applied,
//! normalized to [-1, 1]), combines them with the configured weights, persists
//! the result, and returns it. The math is in `composite` (pure, tested).

pub mod composite;
pub mod cross;

use std::collections::HashMap;

use crate::store::Store;
use crate::util::now_rfc3339;
use composite::{combine, normalize_last, rolling_sma, CompositeResult, SignalInput};

const Z_WINDOW: usize = 252; // ~1y rolling window for z-scores
const MA: usize = 200; // 200-day MA
const SLOPE_LAG: usize = 20; // ~1 month MA slope
const RATE_LAG: usize = 20; // 10y yield change horizon
const FX_MA: usize = 100; // USD/JPY trend MA

/// Signal definitions in §8 order: (key, Japanese label).
const SIGNAL_DEFS: &[(&str, &str)] = &[
    ("us_trend", "米国株トレンド（200DMA乖離・傾き）"),
    ("breadth", "市場ブレッドス（200日線上比率）"),
    ("vix", "ボラティリティ（VIX・反転）"),
    ("credit", "クレジット（HY OAS・反転）"),
    ("rate", "米10年金利（変化）"),
    ("usdjpy", "ドル円トレンド"),
    ("foreign_flow", "海外勢フロー（投資部門別）"),
];

/// Compute the composite from current stored data, persist it, and return it.
pub fn compute_and_store(store: &Store) -> Result<CompositeResult, String> {
    let weights = load_weights(store)?;
    let rate_sign = store
        .read_setting("rate_sign")?
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(-1.0);

    let mut n: HashMap<&str, Option<f64>> = HashMap::new();
    let mut notes: HashMap<&str, Option<String>> = HashMap::new();

    // 1) US trend: S&P 200DMA distance + slope.
    let spx = store.read_closes_asc("INDEX", "SPX")?;
    n.insert("us_trend", us_trend_signal(&spx));
    if n["us_trend"].is_none() {
        notes.insert("us_trend", Some("S&P500 履歴が不足しています。".into()));
    }

    // 2) Breadth: % above 200DMA mapped linearly around 0.5.
    let breadth = store.read_latest_breadth_pct()?;
    n.insert(
        "breadth",
        breadth.map(|p| ((p - 0.5) * 2.0).clamp(-1.0, 1.0)),
    );
    if breadth.is_none() {
        notes.insert("breadth", Some("ブレッドス未取得。".into()));
    }

    // 3) VIX: level z-score, inverted (high vol = risk-off).
    let vix = store.read_series_asc("VIX")?;
    n.insert("vix", normalize_last(&vix, Z_WINDOW, 60).map(|x| -x));
    if vix.len() < 60 {
        notes.insert("vix", Some("VIX 履歴が不足しています。".into()));
    }

    // 4) Rate: 10y yield change over ~1 month, sign configurable.
    let mut us10y = store.read_series_asc("DGS10")?;
    if us10y.is_empty() {
        us10y = store.read_series_asc("US10Y")?;
    }
    let rate_changes = diffs(&us10y, RATE_LAG);
    n.insert(
        "rate",
        normalize_last(&rate_changes, Z_WINDOW, 40).map(|x| rate_sign * x),
    );
    notes.insert(
        "rate",
        Some(format!(
            "符号は設定値（rate_sign={rate_sign:+.0}、上昇=リスクオフ寄り既定）。"
        )),
    );

    // 5) Credit: HY OAS (FRED). Inverted. Unavailable without a FRED key.
    let hy = store.read_series_asc("BAMLH0A0HYM2")?;
    n.insert("credit", normalize_last(&hy, Z_WINDOW, 40).map(|x| -x));
    if hy.is_empty() {
        notes.insert("credit", Some("HY OAS は FRED キー未設定のため未取得。".into()));
    }

    // 6) USD/JPY trend: distance from its ~100d MA (yen weakness = risk-on lean).
    let usdjpy = store.read_fx_asc("USDJPY")?;
    n.insert("usdjpy", usdjpy_signal(&usdjpy));
    notes.insert(
        "usdjpy",
        Some("円安（ドル円上昇）をリスクオン寄りと既定（設定可）。".into()),
    );
    if usdjpy.len() < 60 {
        notes.insert("usdjpy", Some("ドル円は未取得／履歴不足。".into()));
    }

    // 7) Foreign-investor flow (weekly, J-Quants). Held until next publication.
    let foreign = store.read_foreign_flow_net_asc()?;
    n.insert("foreign_flow", normalize_last(&foreign, 52, 8));
    if foreign.is_empty() {
        notes.insert(
            "foreign_flow",
            Some("海外勢フローは J-Quants 資格情報が必要（週次・配信遅延）。".into()),
        );
    } else {
        notes.insert(
            "foreign_flow",
            Some("投資部門別の海外勢ネット（週次・最新公表値を保持）。".into()),
        );
    }

    // Assemble signals in canonical order.
    let signals: Vec<SignalInput> = SIGNAL_DEFS
        .iter()
        .map(|(key, label)| SignalInput {
            name: (*key).to_string(),
            label: (*label).to_string(),
            raw_weight: *weights.get(*key).unwrap_or(&0.0),
            n: *n.get(key).unwrap_or(&None),
            note: notes.get(key).cloned().flatten(),
        })
        .collect();

    let result = combine(&signals, now_rfc3339());

    // Persist.
    let components_json = serde_json::to_string(&result.components)
        .map_err(|e| format!("serialize components: {e}"))?;
    let signal_rows: Vec<(String, Option<f64>, String)> = result
        .components
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                c.n,
                if c.available { "active" } else { "missing" }.to_string(),
            )
        })
        .collect();
    store.persist_composite(
        &result.ts,
        result.score,
        &result.regime_key,
        &components_json,
        &signal_rows,
    )?;

    Ok(result)
}

fn load_weights(store: &Store) -> Result<HashMap<String, f64>, String> {
    let raw = store
        .read_setting("composite_weights")?
        .unwrap_or_else(|| "{}".to_string());
    serde_json::from_str::<HashMap<String, f64>>(&raw)
        .map_err(|e| format!("parse composite_weights: {e}"))
}

/// US trend signal: mean of normalized 200DMA distance and 200DMA slope.
fn us_trend_signal(closes: &[f64]) -> Option<f64> {
    let ma = rolling_sma(closes, MA);

    // distance = close/ma200 - 1, where ma200 exists.
    let distances: Vec<f64> = closes
        .iter()
        .zip(ma.iter())
        .filter_map(|(c, m)| m.map(|mv| c / mv - 1.0))
        .collect();
    let n_dist = normalize_last(&distances, Z_WINDOW, 60);

    // slope = ma200(t)/ma200(t-lag) - 1.
    let ma_vals: Vec<f64> = ma.iter().filter_map(|m| *m).collect();
    let slopes = ratios(&ma_vals, SLOPE_LAG);
    let n_slope = normalize_last(&slopes, Z_WINDOW, 40);

    match (n_dist, n_slope) {
        (Some(d), Some(s)) => Some((d + s) / 2.0),
        (Some(d), None) => Some(d),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

/// USD/JPY trend signal: normalized distance from its ~100d MA, sign positive
/// (yen weakness leans risk-on for Japanese equities).
fn usdjpy_signal(closes: &[f64]) -> Option<f64> {
    let ma = rolling_sma(closes, FX_MA);
    let distances: Vec<f64> = closes
        .iter()
        .zip(ma.iter())
        .filter_map(|(c, m)| m.map(|mv| c / mv - 1.0))
        .collect();
    normalize_last(&distances, Z_WINDOW, 60)
}

/// Differences x[i] - x[i-lag].
fn diffs(x: &[f64], lag: usize) -> Vec<f64> {
    if x.len() <= lag {
        return Vec::new();
    }
    (lag..x.len()).map(|i| x[i] - x[i - lag]).collect()
}

/// Ratios x[i]/x[i-lag] - 1.
fn ratios(x: &[f64], lag: usize) -> Vec<f64> {
    if x.len() <= lag {
        return Vec::new();
    }
    (lag..x.len())
        .filter_map(|i| {
            if x[i - lag] != 0.0 {
                Some(x[i] / x[i - lag] - 1.0)
            } else {
                None
            }
        })
        .collect()
}
