//! Alert engine: evaluate corroboration on the latest signals, dedup, persist,
//! and emit `alert:fired` (architecture §11).

pub mod eval;

use tauri::{AppHandle, Emitter};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::store::models::Alert;
use crate::store::Store;
use crate::util::now_rfc3339;
use eval::{evaluate, SignalReading};

/// Suppress repeats of the same alert title within this window.
const DEDUP_WINDOW_SECS: i64 = 6 * 3600;

/// Run a corroboration check against the latest composite components. Returns
/// the fired alert (if any), persisting it and emitting an event.
pub fn run_check(store: &Store, app: &AppHandle) -> Result<Option<Alert>, String> {
    let min_families = store
        .read_setting("alert_min_families")?
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3);
    let threshold = store
        .read_setting("alert_threshold")?
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.3);

    let readings: Vec<SignalReading> = store
        .latest_components()?
        .into_iter()
        .map(|(name, label, n)| SignalReading { name, label, n })
        .collect();

    let hit = match evaluate(&readings, min_families, threshold) {
        Some(h) => h,
        None => return Ok(None),
    };

    // Dedup / rate-limit by title within the window.
    if let Some(prev_ts) = store.last_alert_ts(&hit.title)? {
        if within_window(&prev_ts, DEDUP_WINDOW_SECS) {
            return Ok(None);
        }
    }

    let ts = now_rfc3339();
    let alert = Alert {
        id: format!("{}-{}", hit.direction, ts),
        ts,
        severity: hit.severity,
        title: hit.title,
        triggers: hit.triggers,
        status: "new".to_string(),
    };
    store.insert_alert(&alert)?;
    let _ = app.emit("alert:fired", &alert);
    Ok(Some(alert))
}

fn within_window(prev_ts: &str, secs: i64) -> bool {
    match OffsetDateTime::parse(prev_ts, &Rfc3339) {
        Ok(prev) => (OffsetDateTime::now_utc() - prev).whole_seconds() < secs,
        Err(_) => false,
    }
}
