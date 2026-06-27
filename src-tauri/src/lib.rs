//! alpha-compass — Tauri Core (Rust).
//!
//! Rust is the command center: sole owner/writer of DuckDB + SQLite, sidecar
//! supervisor, and (in later sessions) compute engine / scheduler / alerts.
//! Session 0 wires up: store init, sidecar spawn + health, and a ping IPC
//! round-trip.

mod alerts;
mod creds;
mod engine;
mod ipc;
mod scheduler;
mod sidecar;
mod store;
mod util;

use std::sync::Arc;

use tauri::Manager;

use sidecar::SidecarManager;
use store::Store;

/// Shared application state managed by Tauri.
pub struct AppState {
    pub sidecar: Arc<SidecarManager>,
    pub store: Arc<Store>,
}

/// Convert any displayable error into the boxed error type Tauri's `setup`
/// expects. Using a concrete `io::Error` avoids relying on blanket `From`
/// impls for trait objects.
fn fatal(e: impl std::fmt::Display) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        e.to_string(),
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // 1) Initialize the Rust-owned databases under the app data dir.
            let data_dir = app.path().app_data_dir().map_err(fatal)?;
            let store = Arc::new(Store::initialize(&data_dir).map_err(fatal)?);
            let status = store.status();
            eprintln!("[alpha-compass] DuckDB: {}", status.duckdb_path);
            eprintln!("[alpha-compass] SQLite: {}", status.sqlite_path);

            // 2) Spawn the Python sidecar and wait for it to become healthy.
            //    Non-fatal: if it can't start, the app still opens and surfaces
            //    the error rather than exiting.
            let manager = Arc::new(match SidecarManager::spawn() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[alpha-compass] sidecar spawn failed: {e}");
                    SidecarManager::disabled(format!("サイドカーを起動できませんでした: {e}"))
                }
            });

            let healthy = manager.wait_until_healthy(40, 250);
            eprintln!("[alpha-compass] sidecar healthy: {healthy}");

            // 3) Self-test the exact chain the "ping" button uses, so dev logs
            //    confirm end-to-end connectivity without a manual click.
            let ping = manager.ping();
            eprintln!(
                "[alpha-compass] self-test ping: reachable={} http_status={} port={} error={:?}",
                ping.reachable, ping.http_status, ping.port, ping.error
            );

            // 4) Start the background scheduler (initial refresh + cadence,
            //    composite recompute, brief, and alert checks).
            scheduler::start(store.clone(), manager.clone(), app.handle().clone());

            app.manage(AppState {
                sidecar: manager,
                store,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::ping_sidecar,
            ipc::get_db_status,
            ipc::get_us_market,
            ipc::get_freshness,
            ipc::refresh,
            ipc::get_composite,
            ipc::get_jp_market,
            ipc::get_watchlist,
            ipc::set_watchlist,
            ipc::get_news,
            ipc::get_disclosures,
            ipc::get_calendar,
            ipc::get_alerts,
            ipc::get_alert_rules,
            ipc::get_brief,
            ipc::get_cross_market,
            ipc::get_settings,
            ipc::set_setting,
            ipc::set_credential,
            ipc::get_credential_status,
            ipc::get_ai_status
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Tear the sidecar down when the app exits.
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    state.sidecar.shutdown();
                }
            }
        });
}
