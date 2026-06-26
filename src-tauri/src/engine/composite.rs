//! Market-regime composite — pure functions (architecture §8).
//!
//! This module contains only math: feature normalization, weighted combination
//! with renormalization over available signals, and regime classification. It
//! has no I/O so it is fully unit-testable. The data-loading wrapper lives in
//! `engine::mod`.

use serde::Serialize;

/// One signal feeding the composite, already sign-applied so that "more
/// risk-on" is positive. `n` is `None` when the underlying data is unavailable.
#[derive(Debug, Clone)]
pub struct SignalInput {
    pub name: String,
    pub label: String,
    pub raw_weight: f64,
    /// Normalized contribution value in [-1, 1], sign already applied.
    pub n: Option<f64>,
    pub note: Option<String>,
}

/// Per-signal breakdown returned to the UI for explainability.
#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub name: String,
    pub label: String,
    /// Effective weight actually used (renormalized over available signals).
    pub weight: f64,
    /// Configured weight before renormalization.
    pub raw_weight: f64,
    pub n: Option<f64>,
    pub contribution: f64,
    pub available: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompositeResult {
    pub ts: String,
    pub score: f64,
    pub regime_key: String,
    pub regime_label: String,
    pub components: Vec<Component>,
    /// Fraction of total configured weight that was available.
    pub coverage: f64,
    pub notes: Vec<String>,
}

/// Map a 0–100 score to a 5-level regime (§8 step 5).
pub fn regime(score: f64) -> (&'static str, &'static str) {
    match score {
        s if s < 20.0 => ("strong_risk_off", "強リスクオフ"),
        s if s < 40.0 => ("risk_off", "リスクオフ"),
        s if s < 60.0 => ("neutral", "中立"),
        s if s < 80.0 => ("risk_on", "リスクオン"),
        _ => ("strong_risk_on", "強リスクオン"),
    }
}

/// Combine signals into a 0–100 score with per-component contributions.
/// Missing signals are excluded and the remaining weights renormalized to sum
/// to 1, so coverage is disclosed rather than silently zero-filled.
pub fn combine(signals: &[SignalInput], ts: String) -> CompositeResult {
    let total_raw: f64 = signals.iter().map(|s| s.raw_weight).sum();
    let avail_raw: f64 = signals
        .iter()
        .filter(|s| s.n.is_some())
        .map(|s| s.raw_weight)
        .sum();

    let mut components = Vec::with_capacity(signals.len());
    let mut a = 0.0;

    for s in signals {
        match s.n {
            Some(n) if avail_raw > 0.0 => {
                let weight = s.raw_weight / avail_raw;
                let contribution = weight * n;
                a += contribution;
                components.push(Component {
                    name: s.name.clone(),
                    label: s.label.clone(),
                    weight,
                    raw_weight: s.raw_weight,
                    n: Some(n),
                    contribution,
                    available: true,
                    note: s.note.clone(),
                });
            }
            _ => components.push(Component {
                name: s.name.clone(),
                label: s.label.clone(),
                weight: 0.0,
                raw_weight: s.raw_weight,
                n: None,
                contribution: 0.0,
                available: false,
                note: s.note.clone(),
            }),
        }
    }

    let score = if avail_raw > 0.0 {
        (50.0 + 50.0 * a).clamp(0.0, 100.0).round()
    } else {
        50.0
    };
    let (regime_key, regime_label) = regime(score);
    let coverage = if total_raw > 0.0 {
        avail_raw / total_raw
    } else {
        0.0
    };

    let mut notes = Vec::new();
    if coverage < 0.999 {
        let missing: Vec<&str> = components
            .iter()
            .filter(|c| !c.available)
            .map(|c| c.label.as_str())
            .collect();
        notes.push(format!(
            "一部シグナル未取得のため利用可能分で再正規化（カバレッジ {:.0}%）。除外: {}",
            coverage * 100.0,
            missing.join("、")
        ));
    }

    CompositeResult {
        ts,
        score,
        regime_key: regime_key.to_string(),
        regime_label: regime_label.to_string(),
        components,
        coverage,
        notes,
    }
}

/// Rolling simple moving average aligned to the input (None until `window`
/// points are available).
pub fn rolling_sma(values: &[f64], window: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if window == 0 {
        return out;
    }
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= window {
            sum -= values[i - window];
        }
        if i + 1 >= window {
            out[i] = Some(sum / window as f64);
        }
    }
    out
}

/// Normalize the last value of a feature series to [-1, 1] using a rolling
/// z-score winsorized at ±3σ, then scaled so ±3σ maps to ±1 (§8 step 2).
/// Returns `None` if there is not enough history.
pub fn normalize_last(feature: &[f64], window: usize, min_points: usize) -> Option<f64> {
    if feature.len() < min_points {
        return None;
    }
    let take = window.min(feature.len());
    let slice = &feature[feature.len() - take..];
    let n = slice.len() as f64;
    let mean = slice.iter().sum::<f64>() / n;
    let var = slice.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let std = var.sqrt();
    let last = *feature.last().unwrap();
    if std == 0.0 {
        return Some(0.0);
    }
    let z = ((last - mean) / std).clamp(-3.0, 3.0);
    Some(z / 3.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(name: &str, w: f64, n: Option<f64>) -> SignalInput {
        SignalInput {
            name: name.to_string(),
            label: name.to_string(),
            raw_weight: w,
            n,
            note: None,
        }
    }

    #[test]
    fn regime_boundaries() {
        assert_eq!(regime(0.0).0, "strong_risk_off");
        assert_eq!(regime(19.9).0, "strong_risk_off");
        assert_eq!(regime(20.0).0, "risk_off");
        assert_eq!(regime(40.0).0, "neutral");
        assert_eq!(regime(59.9).0, "neutral");
        assert_eq!(regime(60.0).0, "risk_on");
        assert_eq!(regime(80.0).0, "strong_risk_on");
        assert_eq!(regime(100.0).0, "strong_risk_on");
    }

    #[test]
    fn all_max_risk_on_is_100() {
        let signals = vec![sig("a", 0.6, Some(1.0)), sig("b", 0.4, Some(1.0))];
        let r = combine(&signals, "t".into());
        assert_eq!(r.score, 100.0);
        assert_eq!(r.regime_key, "strong_risk_on");
        assert!((r.coverage - 1.0).abs() < 1e-9);
    }

    #[test]
    fn all_neutral_is_50() {
        let signals = vec![sig("a", 0.5, Some(0.0)), sig("b", 0.5, Some(0.0))];
        let r = combine(&signals, "t".into());
        assert_eq!(r.score, 50.0);
        assert_eq!(r.regime_key, "neutral");
    }

    #[test]
    fn all_min_risk_off_is_0() {
        let signals = vec![sig("a", 1.0, Some(-1.0))];
        let r = combine(&signals, "t".into());
        assert_eq!(r.score, 0.0);
        assert_eq!(r.regime_key, "strong_risk_off");
    }

    #[test]
    fn missing_signal_renormalizes() {
        // a=+1 weight .2 available, b missing weight .15, c=+1 weight .15.
        // available raw = .35; both n=+1 => A = 1 => score 100, coverage .35/.5.
        let signals = vec![
            sig("a", 0.20, Some(1.0)),
            sig("b", 0.15, None),
            sig("c", 0.15, Some(1.0)),
        ];
        let r = combine(&signals, "t".into());
        assert_eq!(r.score, 100.0);
        assert!((r.coverage - 0.7).abs() < 1e-9);
        let b = r.components.iter().find(|c| c.name == "b").unwrap();
        assert!(!b.available);
        assert_eq!(b.contribution, 0.0);
    }

    #[test]
    fn no_signals_is_neutral_50() {
        let signals = vec![sig("a", 0.5, None)];
        let r = combine(&signals, "t".into());
        assert_eq!(r.score, 50.0);
        assert_eq!(r.coverage, 0.0);
    }

    #[test]
    fn sma_basic() {
        let v = vec![1.0, 2.0, 3.0, 4.0];
        let s = rolling_sma(&v, 2);
        assert_eq!(s[0], None);
        assert_eq!(s[1], Some(1.5));
        assert_eq!(s[2], Some(2.5));
        assert_eq!(s[3], Some(3.5));
    }

    #[test]
    fn normalize_handles_constant_series() {
        let v = vec![5.0; 60];
        assert_eq!(normalize_last(&v, 60, 20), Some(0.0));
    }

    #[test]
    fn normalize_high_last_is_positive() {
        let mut v = vec![0.0; 60];
        *v.last_mut().unwrap() = 10.0;
        let n = normalize_last(&v, 60, 20).unwrap();
        assert!(n > 0.0 && n <= 1.0);
    }

    #[test]
    fn normalize_too_short_is_none() {
        let v = vec![1.0, 2.0, 3.0];
        assert_eq!(normalize_last(&v, 60, 20), None);
    }
}
