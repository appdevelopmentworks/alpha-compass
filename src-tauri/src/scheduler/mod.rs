//! Scheduler (tokio) + refresh orchestration.
//!
//! Drives per-source fetches at their own cadence, updates source freshness,
//! and backs off exponentially on errors. The same `refresh_*` functions back
//! the manual `refresh()` IPC command, so scheduled and on-demand refresh share
//! one code path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;

use crate::sidecar::SidecarManager;
use crate::store::models::Brief;
use crate::store::Store;
use crate::util::now_rfc3339;

/// All data sources refreshed by `refresh_all` / the scheduler.
pub const US_SOURCES: &[&str] = &[
    "yfinance", "fred", "cot", "fx", "jquants", "news", "calendar", "edinet", "tdnet",
];

/// Result of refreshing one source.
#[derive(Debug, Clone, Serialize)]
pub struct RefreshOutcome {
    pub source: String,
    pub status: String, // "ok" | "unavailable" | "error"
    pub rows: usize,
    pub detail: Option<String>,
}

/// Result of a refresh request (one or many sources).
#[derive(Debug, Clone, Serialize)]
pub struct RefreshSummary {
    pub outcomes: Vec<RefreshOutcome>,
    pub at: String,
}

/// Fetch one source, upsert it, and record freshness. Never panics; failures
/// are captured as an "error"/"unavailable" outcome + source_meta update.
pub fn refresh_source(store: &Store, sidecar: &SidecarManager, source: &str) -> RefreshOutcome {
    match sidecar.fetch_batch(source) {
        Ok(batch) => {
            let rows = batch.total_rows();
            let detail = if batch.notes.is_empty() {
                None
            } else {
                Some(batch.notes.join(" / "))
            };

            if rows > 0 {
                if let Err(e) = store.apply_batch(&batch) {
                    let _ = store.update_source_meta(source, "error", Some(&e));
                    return RefreshOutcome {
                        source: source.to_string(),
                        status: "error".to_string(),
                        rows: 0,
                        detail: Some(e),
                    };
                }
            }

            // ok=false with no transport error means "nothing available"
            // (e.g. missing API key) — surfaced as a muted state, not an error.
            let status = if batch.ok { "ok" } else { "unavailable" };
            let _ = store.update_source_meta(source, status, detail.as_deref());
            RefreshOutcome {
                source: source.to_string(),
                status: status.to_string(),
                rows,
                detail,
            }
        }
        Err(e) => {
            let _ = store.update_source_meta(source, "error", Some(&e));
            RefreshOutcome {
                source: source.to_string(),
                status: "error".to_string(),
                rows: 0,
                detail: Some(e),
            }
        }
    }
}

pub fn refresh_sources(
    store: &Store,
    sidecar: &SidecarManager,
    sources: &[String],
) -> RefreshSummary {
    let outcomes = sources
        .iter()
        .map(|s| refresh_source(store, sidecar, s))
        .collect();
    RefreshSummary {
        outcomes,
        at: now_rfc3339(),
    }
}

pub fn refresh_all(store: &Store, sidecar: &SidecarManager) -> RefreshSummary {
    let owned: Vec<String> = US_SOURCES.iter().map(|s| s.to_string()).collect();
    refresh_sources(store, sidecar, &owned)
}

/// Minimum interval between AI brief regenerations (cost control).
const BRIEF_MIN_INTERVAL_SECS: i64 = 3600;

/// Recompute the composite and (re)generate the daily brief via the AI cascade.
pub fn refresh_brief(store: &Store, sidecar: &SidecarManager) -> Result<Brief, String> {
    let comp = crate::engine::compute_and_store(store)?;
    generate_brief(store, sidecar, &comp)
}

/// Generate + persist the brief from an already-computed composite. Honors the
/// `ai_max_tier` setting (rule | local | claude) so the user controls cost.
fn generate_brief(
    store: &Store,
    sidecar: &SidecarManager,
    comp: &crate::engine::composite::CompositeResult,
) -> Result<Brief, String> {
    let max_tier = store
        .read_setting("ai_max_tier")?
        .unwrap_or_else(|| "local".to_string());
    let headlines = store.latest_headlines(6)?;
    let payload = serde_json::json!({
        "score": comp.score,
        "regime_label": comp.regime_label,
        "coverage": (comp.coverage * 100.0).round(),
        "headlines": headlines,
        "max_tier": max_tier,
    });
    let (text, tier) = sidecar.fetch_brief(&payload)?;
    let brief = Brief {
        text,
        tier,
        generated_at: now_rfc3339(),
    };
    store.store_brief(&brief)?;
    Ok(brief)
}

/// Whether the cached brief is older than the regeneration interval.
fn brief_is_stale(store: &Store) -> bool {
    match store.read_brief() {
        Ok(Some(b)) => match time::OffsetDateTime::parse(
            &b.generated_at,
            &time::format_description::well_known::Rfc3339,
        ) {
            Ok(t) => {
                (time::OffsetDateTime::now_utc() - t).whole_seconds() >= BRIEF_MIN_INTERVAL_SECS
            }
            Err(_) => true,
        },
        _ => true, // no brief yet
    }
}

/// One source's polling state.
struct Slot {
    source: String,
    interval: Duration,
    next: Instant,
    fails: u32,
}

/// Spawn the background scheduler. Performs an initial refresh shortly after
/// startup, then polls each source at its cadence with exponential backoff.
pub fn start(store: Arc<Store>, sidecar: Arc<SidecarManager>, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Let the sidecar finish coming up.
        tokio::time::sleep(Duration::from_secs(2)).await;

        let now = Instant::now();
        let mut slots: Vec<Slot> = [
            ("yfinance", Duration::from_secs(300)),
            ("fx", Duration::from_secs(300)),
            ("news", Duration::from_secs(600)),
            ("cot", Duration::from_secs(6 * 3600)),
            ("fred", Duration::from_secs(12 * 3600)),
            ("jquants", Duration::from_secs(6 * 3600)),
            ("edinet", Duration::from_secs(3 * 3600)),
            ("tdnet", Duration::from_secs(3 * 3600)),
            ("calendar", Duration::from_secs(12 * 3600)),
        ]
        .into_iter()
        .map(|(s, d)| Slot {
            source: s.to_string(),
            interval: d,
            next: now, // due immediately on first tick
            fails: 0,
        })
        .collect();

        let cap = Duration::from_secs(3600);

        loop {
            let now = Instant::now();
            let mut recompute = false;
            for slot in slots.iter_mut() {
                if slot.next > now {
                    continue;
                }
                let store = store.clone();
                let sidecar = sidecar.clone();
                let src = slot.source.clone();
                let outcome =
                    tauri::async_runtime::spawn_blocking(move || refresh_source(&store, &sidecar, &src))
                        .await;

                if let Ok(o) = &outcome {
                    eprintln!(
                        "[alpha-compass] refresh {}: status={} rows={}{}",
                        o.source,
                        o.status,
                        o.rows,
                        o.detail
                            .as_ref()
                            .map(|d| format!(" detail=\"{d}\""))
                            .unwrap_or_default()
                    );
                }

                match outcome {
                    Ok(o) if o.status == "error" => {
                        slot.fails = (slot.fails + 1).min(6);
                        let factor = 1u32 << slot.fails.min(5);
                        let backoff = slot.interval.saturating_mul(factor).min(cap);
                        slot.next = Instant::now() + backoff;
                    }
                    Ok(o) => {
                        slot.fails = 0;
                        slot.next = Instant::now() + slot.interval;
                        if o.rows > 0 {
                            recompute = true;
                        }
                    }
                    Err(_) => {
                        // spawn_blocking join error: retry soon.
                        slot.next = Instant::now() + Duration::from_secs(60);
                    }
                }
            }

            // Recompute composite, regenerate brief, and run alert checks when
            // fresh data landed this tick.
            if recompute {
                let store = store.clone();
                let sidecar = sidecar.clone();
                let app = app.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    // Composite, cross-market, and alerts are local/free — run
                    // every time fresh data lands.
                    let comp = crate::engine::compute_and_store(&store);
                    match crate::engine::cross::compute_and_store(&store) {
                        Ok(c) => eprintln!(
                            "[alpha-compass] cross-market: {} transmission(s)",
                            c.transmissions.len()
                        ),
                        Err(e) => eprintln!("[alpha-compass] cross-market error: {e}"),
                    }
                    match crate::alerts::run_check(&store, &app) {
                        Ok(Some(a)) => eprintln!(
                            "[alpha-compass] alert fired: {} ({})",
                            a.title, a.severity
                        ),
                        Ok(None) => {}
                        Err(e) => eprintln!("[alpha-compass] alert error: {e}"),
                    }
                    // The brief may hit an LLM (cost) — regenerate at most hourly.
                    if let Ok(comp) = comp {
                        if brief_is_stale(&store) {
                            match generate_brief(&store, &sidecar, &comp) {
                                Ok(b) => eprintln!(
                                    "[alpha-compass] brief regenerated (tier={})",
                                    b.tier
                                ),
                                Err(e) => eprintln!("[alpha-compass] brief error: {e}"),
                            }
                        }
                    }
                })
                .await;
            }

            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}
