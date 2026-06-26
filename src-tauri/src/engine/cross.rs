//! US→Japan cross-market transmission (architecture §9).
//!
//! Transparent, rule-based notes — NOT predictions. Observed overnight US /
//! FX / rates conditions map to a likely TSE-open sector tilt. Rules are
//! user-editable (stored as JSON in settings).

use std::collections::HashMap;

use serde::Deserialize;

use crate::store::models::{CrossMarket, CrossTransmission, MetricLabel};
use crate::store::Store;
use crate::util::now_rfc3339;

#[derive(Debug, Clone, Deserialize)]
pub struct Cond {
    pub m: String,
    pub op: String,
    pub v: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub driver: String,
    pub path: String,
    pub when: Vec<Cond>,
}

/// Metric keys referenced by rules, with their human labels and units.
const METRIC_LABELS: &[(&str, &str, &str)] = &[
    ("us10y_chg", "米10年金利Δ", "pt"),
    ("usdjpy_chg_pct", "ドル円", "%"),
    ("vix_chg", "VIXΔ", ""),
    ("comp_chg_pct", "ナスダック", "%"),
    ("nikkei_gap_pct", "日経先物ギャップ", "%"),
];

fn label_for(m: &str) -> &str {
    METRIC_LABELS
        .iter()
        .find(|(k, _, _)| *k == m)
        .map(|(_, l, _)| *l)
        .unwrap_or(m)
}

fn unit_for(m: &str) -> &str {
    METRIC_LABELS
        .iter()
        .find(|(k, _, _)| *k == m)
        .map(|(_, _, u)| *u)
        .unwrap_or("")
}

fn cond_holds(cond: &Cond, metrics: &HashMap<String, f64>) -> bool {
    let Some(&val) = metrics.get(&cond.m) else {
        return false; // metric unavailable -> condition cannot hold
    };
    match cond.op.as_str() {
        ">" => val > cond.v,
        "<" => val < cond.v,
        ">=" => val >= cond.v,
        "<=" => val <= cond.v,
        "abs_gt" => val.abs() > cond.v,
        _ => false,
    }
}

/// Pure evaluation: emit a transmission for each rule whose conditions all hold.
pub fn evaluate(rules: &[Rule], metrics: &HashMap<String, f64>) -> Vec<CrossTransmission> {
    let mut out = Vec::new();
    for rule in rules {
        if rule.when.is_empty() || !rule.when.iter().all(|c| cond_holds(c, metrics)) {
            continue;
        }
        let note = rule
            .when
            .iter()
            .map(|c| {
                let v = metrics.get(&c.m).copied().unwrap_or(0.0);
                format!("{} {:+.2}{}", label_for(&c.m), v, unit_for(&c.m))
            })
            .collect::<Vec<_>>()
            .join(" / ");
        out.push(CrossTransmission {
            driver: rule.driver.clone(),
            path: rule.path.clone(),
            effect_note: note,
        });
    }
    out
}

fn last_two(v: &[f64]) -> Option<(f64, f64)> {
    if v.len() >= 2 {
        Some((v[v.len() - 2], v[v.len() - 1]))
    } else {
        None
    }
}

/// Compute the metric snapshot from stored series.
fn compute_metrics(store: &Store) -> Result<HashMap<String, f64>, String> {
    let mut m = HashMap::new();

    let mut us10y = store.read_series_asc("DGS10")?;
    if us10y.is_empty() {
        us10y = store.read_series_asc("US10Y")?;
    }
    if let Some((p, l)) = last_two(&us10y) {
        m.insert("us10y_chg".to_string(), l - p);
    }
    if let Some((p, l)) = last_two(&store.read_fx_asc("USDJPY")?) {
        if p != 0.0 {
            m.insert("usdjpy_chg_pct".to_string(), (l / p - 1.0) * 100.0);
        }
    }
    if let Some((p, l)) = last_two(&store.read_series_asc("VIX")?) {
        m.insert("vix_chg".to_string(), l - p);
    }
    if let Some((p, l)) = last_two(&store.read_closes_asc("INDEX", "COMP")?) {
        if p != 0.0 {
            m.insert("comp_chg_pct".to_string(), (l / p - 1.0) * 100.0);
        }
    }
    let nkd = store.read_closes_asc("JP_FUT", "NKD")?;
    let n225 = store.read_closes_asc("JP_INDEX", "N225")?;
    if let (Some(&f), Some(&s)) = (nkd.last(), n225.last()) {
        if s != 0.0 {
            m.insert("nikkei_gap_pct".to_string(), (f / s - 1.0) * 100.0);
        }
    }
    Ok(m)
}

/// Compute transmissions from current data + editable rules, persist, return.
pub fn compute_and_store(store: &Store) -> Result<CrossMarket, String> {
    let metrics = compute_metrics(store)?;

    let rules_json = store
        .read_setting("cross_market_rules")?
        .unwrap_or_else(|| "[]".to_string());
    let rules: Vec<Rule> =
        serde_json::from_str(&rules_json).map_err(|e| format!("parse cross rules: {e}"))?;

    let transmissions = evaluate(&rules, &metrics);
    let ts = now_rfc3339();
    store.replace_cross_market(&ts, &transmissions)?;

    let metric_labels = METRIC_LABELS
        .iter()
        .map(|(k, l, _u)| MetricLabel {
            label: (*l).to_string(),
            value: metrics.get(*k).copied(),
        })
        .collect();

    Ok(CrossMarket {
        transmissions,
        metrics: metric_labels,
        freshness: store.read_freshness()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<Rule> {
        vec![
            Rule {
                driver: "rates_up_yen_weak".into(),
                path: "exporters tailwind".into(),
                when: vec![
                    Cond { m: "us10y_chg".into(), op: ">".into(), v: 0.0 },
                    Cond { m: "usdjpy_chg_pct".into(), op: ">".into(), v: 0.0 },
                ],
            },
            Rule {
                driver: "gap".into(),
                path: "open tilt".into(),
                when: vec![Cond { m: "nikkei_gap_pct".into(), op: "abs_gt".into(), v: 0.5 }],
            },
        ]
    }

    #[test]
    fn fires_when_all_conditions_hold() {
        let mut m = HashMap::new();
        m.insert("us10y_chg".to_string(), 0.05);
        m.insert("usdjpy_chg_pct".to_string(), 0.3);
        let out = evaluate(&rules(), &m);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].driver, "rates_up_yen_weak");
    }

    #[test]
    fn does_not_fire_when_one_condition_fails() {
        let mut m = HashMap::new();
        m.insert("us10y_chg".to_string(), 0.05);
        m.insert("usdjpy_chg_pct".to_string(), -0.3); // yen strong
        assert!(evaluate(&rules(), &m).is_empty());
    }

    #[test]
    fn missing_metric_blocks_rule() {
        let m = HashMap::new(); // nothing available
        assert!(evaluate(&rules(), &m).is_empty());
    }

    #[test]
    fn abs_gt_gap_fires_both_directions() {
        let mut m = HashMap::new();
        m.insert("nikkei_gap_pct".to_string(), -0.8);
        let out = evaluate(&rules(), &m);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].driver, "gap");
    }
}
