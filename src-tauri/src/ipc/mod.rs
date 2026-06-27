//! Tauri command handlers (frontend-facing IPC surface).
//!
//! App-defined commands like these are callable from the webview once
//! registered in `generate_handler!`; they do not require ACL permission
//! entries (only plugin/core commands do).

use tauri::State;

use crate::engine::composite::CompositeResult;
use crate::scheduler::{self, RefreshSummary};
use crate::sidecar::PingResult;
use crate::creds;
use crate::store::models::{
    Alert, AlertRule, Brief, CalendarEvent, CredentialStatus, CrossMarket, DisclosureItem,
    FeedFilter, JpMarket, NewsItem, SettingKV, SourceMeta, UsMarket, WatchItem,
};
use crate::store::DbStatus;
use crate::AppState;

/// Ping the sidecar through Rust: the full round-trip the "ping" button drives.
#[tauri::command]
pub fn ping_sidecar(state: State<'_, AppState>) -> PingResult {
    state.sidecar.ping()
}

/// Report the DuckDB / SQLite paths and existence.
#[tauri::command]
pub fn get_db_status(state: State<'_, AppState>) -> DbStatus {
    state.store.status()
}

/// US Market view payload (indices, sectors, breadth, rates, COT, freshness).
#[tauri::command]
pub fn get_us_market(state: State<'_, AppState>) -> Result<UsMarket, String> {
    state.store.read_us_market()
}

/// Per-source last-fetched time + status.
#[tauri::command]
pub fn get_freshness(state: State<'_, AppState>) -> Result<Vec<SourceMeta>, String> {
    state.store.read_freshness()
}

/// Compute (and persist) the market-regime composite from current data.
#[tauri::command]
pub fn get_composite(state: State<'_, AppState>) -> Result<CompositeResult, String> {
    crate::engine::compute_and_store(&state.store)
}

/// Japan Market view payload.
#[tauri::command]
pub fn get_jp_market(state: State<'_, AppState>) -> Result<JpMarket, String> {
    state.store.read_jp_market()
}

/// Read the watchlist.
#[tauri::command]
pub fn get_watchlist(state: State<'_, AppState>) -> Result<Vec<WatchItem>, String> {
    state.store.read_watchlist()
}

/// Replace the watchlist with the given ordered items.
#[tauri::command]
pub fn set_watchlist(
    state: State<'_, AppState>,
    items: Vec<WatchItem>,
) -> Result<(), String> {
    state.store.set_watchlist(&items)
}

/// Latest news items (with AI summaries + generating tier).
#[tauri::command]
pub fn get_news(state: State<'_, AppState>, limit: Option<u32>) -> Result<Vec<NewsItem>, String> {
    state.store.read_news(limit.unwrap_or(60))
}

/// Disclosures feed, optionally filtered.
#[tauri::command]
pub fn get_disclosures(
    state: State<'_, AppState>,
    filter: Option<FeedFilter>,
) -> Result<Vec<DisclosureItem>, String> {
    state.store.read_disclosures(&filter.unwrap_or_default())
}

/// Upcoming calendar events (FOMC/BOJ etc.).
#[tauri::command]
pub fn get_calendar(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<CalendarEvent>, String> {
    state.store.read_calendar_upcoming(limit.unwrap_or(12))
}

/// Alert history.
#[tauri::command]
pub fn get_alerts(state: State<'_, AppState>, limit: Option<u32>) -> Result<Vec<Alert>, String> {
    state.store.read_alerts(limit.unwrap_or(50))
}

/// Alert rules.
#[tauri::command]
pub fn get_alert_rules(state: State<'_, AppState>) -> Result<Vec<AlertRule>, String> {
    state.store.read_alert_rules()
}

/// Daily brief — cached if available, else generated via the AI cascade.
#[tauri::command]
pub async fn get_brief(state: State<'_, AppState>) -> Result<Brief, String> {
    let store = state.store.clone();
    let sidecar = state.sidecar.clone();
    tauri::async_runtime::spawn_blocking(move || match store.read_brief() {
        Ok(Some(b)) => Ok(b),
        _ => scheduler::refresh_brief(&store, &sidecar),
    })
    .await
    .map_err(|e| format!("brief join error: {e}"))?
}

/// US→Japan cross-market transmission (computed from current data + rules).
#[tauri::command]
pub fn get_cross_market(state: State<'_, AppState>) -> Result<CrossMarket, String> {
    crate::engine::cross::compute_and_store(&state.store)
}

/// All non-secret settings (composite weights, rate sign, alert thresholds,
/// cross-market rules, etc.).
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Vec<SettingKV>, String> {
    state.store.read_all_settings()
}

/// Update one setting (e.g. composite_weights, rate_sign, alert_threshold).
#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    state.store.set_setting(&key, &value)
}

/// Store a credential in the OS keychain (empty token clears it).
#[tauri::command]
pub fn set_credential(source: String, token: String) -> Result<(), String> {
    creds::set_credential(&source, &token)
}

/// Which credentials are configured (values never exposed).
#[tauri::command]
pub fn get_credential_status() -> Result<Vec<CredentialStatus>, String> {
    Ok(creds::status())
}

/// Which AI provider is currently active (for the header indicator).
#[derive(serde::Serialize)]
pub struct AiStatus {
    pub mode: String,     // ai_max_tier setting
    pub provider: String, // "local" | "claude" | "rule"
    pub label: String,    // human-readable, e.g. "ローカル: gemma4:latest"
    pub detail: Option<String>,
    pub local_available: bool,
    pub local_endpoint: Option<String>,
    pub local_model: Option<String>,
    pub claude_available: bool,
}

#[tauri::command]
pub async fn get_ai_status(state: State<'_, AppState>) -> Result<AiStatus, String> {
    let store = state.store.clone();
    let sidecar = state.sidecar.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mode = store
            .read_setting("ai_max_tier")
            .ok()
            .flatten()
            .unwrap_or_else(|| "local".to_string());
        let s = sidecar.fetch_ai_status()?;

        // Mirror the router's resolution: local is preferred; Claude only in
        // "claude" mode; otherwise rule.
        let (provider, label) = if mode == "rule" {
            ("rule", "ルール（LLM不使用）".to_string())
        } else if s.local_available {
            (
                "local",
                format!("ローカル: {}", s.local_model.clone().unwrap_or_default()),
            )
        } else if mode == "claude" && s.claude_available {
            ("claude", "Claude".to_string())
        } else {
            ("rule", "ルール（フォールバック）".to_string())
        };

        Ok(AiStatus {
            mode,
            provider: provider.to_string(),
            label,
            detail: s.local_endpoint.clone(),
            local_available: s.local_available,
            local_endpoint: s.local_endpoint,
            local_model: s.local_model,
            claude_available: s.claude_available,
        })
    })
    .await
    .map_err(|e| format!("ai status join error: {e}"))?
}

/// Manually refresh one source (`source = "yfinance" | "fred" | "cot"`) or all
/// US sources when `source` is omitted. Runs the blocking fetch off the UI
/// thread via the blocking pool.
#[tauri::command]
pub async fn refresh(
    state: State<'_, AppState>,
    source: Option<String>,
) -> Result<RefreshSummary, String> {
    let store = state.store.clone();
    let sidecar = state.sidecar.clone();
    tauri::async_runtime::spawn_blocking(move || match source {
        Some(s) => scheduler::refresh_sources(&store, &sidecar, &[s]),
        None => scheduler::refresh_all(&store, &sidecar),
    })
    .await
    .map_err(|e| format!("refresh join error: {e}"))
}
