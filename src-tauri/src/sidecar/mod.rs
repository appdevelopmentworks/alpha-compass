//! Python sidecar lifecycle + HTTP client.
//!
//! Rust spawns the stateless FastAPI sidecar, generates a random per-session
//! token (passed via environment variable), binds it to 127.0.0.1 only, and is
//! the only caller of its endpoints. Every request carries the bearer token;
//! the sidecar returns 401 on mismatch (architecture §4.3).

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::util::now_rfc3339;

const TOKEN_ENV: &str = "ALPHA_COMPASS_SIDECAR_TOKEN";
const HOST_ENV: &str = "ALPHA_COMPASS_SIDECAR_HOST";
const PORT_ENV: &str = "ALPHA_COMPASS_SIDECAR_PORT";
const HOST: &str = "127.0.0.1";

/// Body returned by the sidecar's `GET /health`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarHealth {
    pub status: String,
    pub service: String,
    pub time: String,
}

/// Outcome of a frontend → Rust → sidecar → frontend round-trip.
#[derive(Debug, Clone, Serialize)]
pub struct PingResult {
    pub reachable: bool,
    pub http_status: u16,
    pub port: u16,
    pub round_trip_ms: u64,
    /// Rust-side timestamp of the check (UTC RFC3339, freshness).
    pub checked_at: String,
    pub health: Option<SidecarHealth>,
    pub error: Option<String>,
}

/// Owns the spawned sidecar process and the connection parameters.
pub struct SidecarManager {
    host: String,
    port: u16,
    token: String,
    child: Mutex<Option<Child>>,
}

impl SidecarManager {
    /// Pick a free port, generate a token, and spawn the sidecar.
    ///
    /// In dev it runs the project's venv interpreter directly (so the child IS
    /// the server and can be killed cleanly); if no venv is present it falls
    /// back to `uv run`.
    pub fn spawn(sidecar_dir: &Path) -> Result<Self, String> {
        let port = pick_free_port()?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

        let mut cmd = match venv_python(sidecar_dir) {
            Some(python) => {
                let mut c = Command::new(python);
                c.arg("-m").arg("uvicorn");
                c
            }
            None => {
                let mut c = Command::new("uv");
                c.arg("run").arg("uvicorn");
                c
            }
        };

        cmd.current_dir(sidecar_dir)
            .arg("app.main:app")
            .arg("--host")
            .arg(HOST)
            .arg("--port")
            .arg(port.to_string())
            .arg("--log-level")
            .arg("info")
            .env(TOKEN_ENV, &token)
            .env(HOST_ENV, HOST)
            .env(PORT_ENV, port.to_string());

        // Inject configured credentials (from the OS keychain) so the sidecar
        // can reach FRED / J-Quants / Anthropic / EDINET when keys are present.
        for (env_key, value) in crate::creds::sidecar_env() {
            cmd.env(env_key, value);
        }

        // Don't pop a console window for the child process on Windows.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let child = cmd.spawn().map_err(|e| format!("spawn sidecar: {e}"))?;

        Ok(Self {
            host: HOST.to_string(),
            port,
            token,
            child: Mutex::new(Some(child)),
        })
    }

    fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Fetch + normalize one data source via `POST /fetch/{source}`.
    /// Blocking; callers run it off the UI thread.
    pub fn fetch_batch(
        &self,
        source: &str,
    ) -> Result<crate::store::models::NormalizedBatch, String> {
        let url = format!("{}/fetch/{}", self.base_url(), source);
        let resp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(180))
            .send_json(serde_json::json!({}))
            .map_err(|e| match e {
                ureq::Error::Status(code, _) => format!("sidecar /fetch/{source}: HTTP {code}"),
                other => format!("sidecar /fetch/{source}: {other}"),
            })?;
        resp.into_json::<crate::store::models::NormalizedBatch>()
            .map_err(|e| format!("decode {source} batch: {e}"))
    }

    /// Generate the daily brief via `POST /ai/brief`. Returns (text, tier).
    pub fn fetch_brief(&self, payload: &serde_json::Value) -> Result<(String, String), String> {
        #[derive(serde::Deserialize)]
        struct BriefResp {
            text: String,
            tier: String,
        }
        let url = format!("{}/ai/brief", self.base_url());
        let resp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(45))
            .send_json(payload.clone())
            .map_err(|e| match e {
                ureq::Error::Status(code, _) => format!("sidecar /ai/brief: HTTP {code}"),
                other => format!("sidecar /ai/brief: {other}"),
            })?;
        let b = resp
            .into_json::<BriefResp>()
            .map_err(|e| format!("decode brief: {e}"))?;
        Ok((b.text, b.tier))
    }

    /// Poll `/health` until reachable or attempts run out.
    pub fn wait_until_healthy(&self, attempts: u32, delay_ms: u64) -> bool {
        for _ in 0..attempts {
            if self.ping().reachable {
                return true;
            }
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        false
    }

    /// Perform a single authenticated `GET /health`.
    pub fn ping(&self) -> PingResult {
        let url = format!("{}/health", self.base_url());
        let checked_at = now_rfc3339();
        let started = Instant::now();

        let response = ureq::get(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(3))
            .call();

        let round_trip_ms = started.elapsed().as_millis() as u64;

        match response {
            Ok(resp) => {
                let http_status = resp.status();
                match resp.into_json::<SidecarHealth>() {
                    Ok(health) => PingResult {
                        reachable: true,
                        http_status,
                        port: self.port,
                        round_trip_ms,
                        checked_at,
                        health: Some(health),
                        error: None,
                    },
                    Err(e) => PingResult {
                        reachable: false,
                        http_status,
                        port: self.port,
                        round_trip_ms,
                        checked_at,
                        health: None,
                        error: Some(format!("decode health: {e}")),
                    },
                }
            }
            Err(ureq::Error::Status(code, _)) => PingResult {
                reachable: false,
                http_status: code,
                port: self.port,
                round_trip_ms,
                checked_at,
                health: None,
                error: Some(format!("http status {code}")),
            },
            Err(e) => PingResult {
                reachable: false,
                http_status: 0,
                port: self.port,
                round_trip_ms,
                checked_at,
                health: None,
                error: Some(e.to_string()),
            },
        }
    }

    /// Best-effort termination of the sidecar process (called on app exit).
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Bind to an ephemeral port, read it back, then release it for the sidecar.
fn pick_free_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("bind ephemeral port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    Ok(port)
}

/// Path to the sidecar's virtualenv interpreter, if it exists.
fn venv_python(sidecar_dir: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidate = sidecar_dir.join(".venv").join("Scripts").join("python.exe");
    #[cfg(not(windows))]
    let candidate = sidecar_dir.join(".venv").join("bin").join("python");

    candidate.exists().then_some(candidate)
}
