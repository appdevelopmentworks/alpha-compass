//! Alert corroboration logic — pure, unit-tested (architecture §11).
//!
//! An alert fires only when several *independent* signal families agree on a
//! risk direction beyond a threshold — never on a single source. This is the
//! noise-suppression rule that distinguishes the project from naive alerting.

/// A single normalized signal reading (sign-applied, risk-on positive).
#[derive(Debug, Clone)]
pub struct SignalReading {
    /// Signal key (kept for traceability; display uses `label`).
    #[allow(dead_code)]
    pub name: String,
    pub label: String,
    pub n: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertHit {
    pub direction: String, // "risk_off" | "risk_on"
    pub severity: String,  // "medium" | "high"
    pub title: String,
    pub triggers: Vec<String>,
}

/// Fire when at least `min_families` readings exceed `threshold` in the same
/// direction. The stronger-populated direction wins; ties go to risk-off.
pub fn evaluate(
    readings: &[SignalReading],
    min_families: usize,
    threshold: f64,
) -> Option<AlertHit> {
    let off: Vec<&SignalReading> = readings.iter().filter(|r| r.n <= -threshold).collect();
    let on: Vec<&SignalReading> = readings.iter().filter(|r| r.n >= threshold).collect();

    let (direction, title, sigs): (&str, &str, &Vec<&SignalReading>) =
        if off.len() >= min_families && off.len() >= on.len() {
            ("risk_off", "リスクオフ警戒（複数シグナル一致）", &off)
        } else if on.len() >= min_families {
            ("risk_on", "リスクオン傾斜（複数シグナル一致）", &on)
        } else {
            return None;
        };

    let severity = if sigs.len() >= min_families + 1 {
        "high"
    } else {
        "medium"
    };
    let triggers = sigs
        .iter()
        .map(|s| format!("{}: n={:+.2}", s.label, s.n))
        .collect();

    Some(AlertHit {
        direction: direction.to_string(),
        severity: severity.to_string(),
        title: title.to_string(),
        triggers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, n: f64) -> SignalReading {
        SignalReading {
            name: name.to_string(),
            label: name.to_string(),
            n,
        }
    }

    #[test]
    fn fires_on_three_corroborating_risk_off() {
        let readings = vec![r("a", -0.5), r("b", -0.4), r("c", -0.6)];
        let hit = evaluate(&readings, 3, 0.3).unwrap();
        assert_eq!(hit.direction, "risk_off");
        assert_eq!(hit.severity, "medium"); // exactly min, not above
        assert_eq!(hit.triggers.len(), 3);
    }

    #[test]
    fn four_corroborating_is_high_severity() {
        let readings = vec![r("a", -0.5), r("b", -0.4), r("c", -0.6), r("d", -0.9)];
        let hit = evaluate(&readings, 3, 0.3).unwrap();
        assert_eq!(hit.severity, "high");
    }

    #[test]
    fn single_strong_signal_does_not_fire() {
        let readings = vec![r("a", -0.9), r("b", 0.05), r("c", -0.1)];
        assert!(evaluate(&readings, 3, 0.3).is_none());
    }

    #[test]
    fn mixed_directions_below_threshold_count_do_not_fire() {
        let readings = vec![r("a", -0.5), r("b", -0.4), r("c", 0.5), r("d", 0.4)];
        // 2 off, 2 on, neither reaches min 3.
        assert!(evaluate(&readings, 3, 0.3).is_none());
    }

    #[test]
    fn fires_risk_on_when_dominant() {
        let readings = vec![r("a", 0.5), r("b", 0.4), r("c", 0.6), r("d", -0.4)];
        let hit = evaluate(&readings, 3, 0.3).unwrap();
        assert_eq!(hit.direction, "risk_on");
    }
}
