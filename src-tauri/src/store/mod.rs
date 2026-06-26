//! Persistence layer.
//!
//! Rust (Tauri Core) is the SOLE owner and writer of both databases
//! (architecture §1). Live connections are held behind mutexes so every read
//! and write is serialized through this process — structurally enforcing the
//! single-writer rule. DuckDB holds time-series/analytics; SQLite holds
//! settings/state and source freshness.

pub mod models;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

use crate::util::now_rfc3339;
use models::{
    Alert, AlertRule, Brief, BreadthQuote, CalendarEvent, CotQuote, CrossTransmission,
    DisclosureItem, FeedFilter, FuturesGap, FxQuote, IndexQuote, InvestorFlowQuote, JpMarket,
    MarginQuote, MetricPoint, NewsItem, NormalizedBatch, RatesSnapshot, SectorQuote, SettingKV,
    ShortSellingQuote, SourceMeta, UpsertCounts, UsMarket, WatchItem,
};

const DUCKDB_FILE: &str = "alpha-compass.duckdb";
const SQLITE_FILE: &str = "alpha-compass.sqlite";

/// Data sources seeded into `source_meta` so freshness shows them from the
/// first launch.
const KNOWN_SOURCES: &[&str] = &[
    "yfinance", "fred", "cot", "fx", "jquants", "news", "calendar", "edinet", "tdnet",
];

/// Display order for index quotes.
const INDEX_ORDER: &[&str] = &["SPX", "COMP", "DJI", "RUT"];
const JP_INDEX_ORDER: &[&str] = &["N225", "TOPIX", "TOPIX_ETF"];

/// Default composite weights (architecture §8). Stored as a setting so the user
/// can tune them later (Session 5).
const DEFAULT_WEIGHTS_JSON: &str = r#"{"us_trend":0.20,"breadth":0.15,"vix":0.15,"credit":0.15,"rate":0.10,"usdjpy":0.10,"foreign_flow":0.15}"#;
/// Default rate-signal sign: rising 10y yields lean risk-off (configurable).
const DEFAULT_RATE_SIGN: &str = "-1";

/// Default editable US→Japan transmission rules (architecture §9). Each rule's
/// `when` conditions must all hold (metrics: us10y_chg, usdjpy_chg_pct,
/// vix_chg, comp_chg_pct, nikkei_gap_pct). Ops: ">", "<", "abs_gt".
const DEFAULT_CROSS_RULES_JSON: &str = r#"[
  {"driver":"米10年金利↑ × ドル円↑","path":"金融・輸出株に追い風 / グロース・不動産に逆風","when":[{"m":"us10y_chg","op":">","v":0.0},{"m":"usdjpy_chg_pct","op":">","v":0.0}]},
  {"driver":"米10年金利↓ × ドル円↓（円高）","path":"輸出株に逆風 / 内需・ディフェンシブが相対優位","when":[{"m":"us10y_chg","op":"<","v":0.0},{"m":"usdjpy_chg_pct","op":"<","v":0.0}]},
  {"driver":"VIX↑ × ナスダック↓","path":"日本のハイグロース・半導体に逆風","when":[{"m":"vix_chg","op":">","v":0.0},{"m":"comp_chg_pct","op":"<","v":0.0}]},
  {"driver":"夜間 日経先物ギャップ","path":"東証寄り付きの方向を示唆","when":[{"m":"nikkei_gap_pct","op":"abs_gt","v":0.5}]}
]"#;

#[derive(Debug, Clone, Serialize)]
pub struct DbStatus {
    pub duckdb_path: String,
    pub sqlite_path: String,
    pub duckdb_exists: bool,
    pub sqlite_exists: bool,
    pub initialized_at: String,
}

pub struct Store {
    duck: Mutex<duckdb::Connection>,
    sqlite: Mutex<rusqlite::Connection>,
    duckdb_path: PathBuf,
    sqlite_path: PathBuf,
    initialized_at: String,
}

impl Store {
    /// Open both databases and run idempotent migrations.
    pub fn initialize(data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {e}"))?;
        let duckdb_path = data_dir.join(DUCKDB_FILE);
        let sqlite_path = data_dir.join(SQLITE_FILE);

        let duck = duckdb::Connection::open(&duckdb_path)
            .map_err(|e| format!("open duckdb: {e}"))?;
        let sqlite = rusqlite::Connection::open(&sqlite_path)
            .map_err(|e| format!("open sqlite: {e}"))?;

        migrate_duckdb(&duck)?;
        migrate_sqlite(&sqlite)?;
        seed_source_meta(&sqlite)?;
        seed_settings(&sqlite)?;
        seed_watchlist(&sqlite)?;
        seed_alert_rules(&sqlite)?;

        Ok(Self {
            duck: Mutex::new(duck),
            sqlite: Mutex::new(sqlite),
            duckdb_path,
            sqlite_path,
            initialized_at: now_rfc3339(),
        })
    }

    pub fn status(&self) -> DbStatus {
        DbStatus {
            duckdb_path: self.duckdb_path.display().to_string(),
            sqlite_path: self.sqlite_path.display().to_string(),
            duckdb_exists: self.duckdb_path.exists(),
            sqlite_exists: self.sqlite_path.exists(),
            initialized_at: self.initialized_at.clone(),
        }
    }

    // -- Writes ------------------------------------------------------------

    /// Upsert all non-empty arrays of a normalized batch into DuckDB, inside a
    /// single transaction.
    pub fn apply_batch(&self, batch: &NormalizedBatch) -> Result<UpsertCounts, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        conn.execute_batch("BEGIN TRANSACTION;")
            .map_err(|e| format!("begin: {e}"))?;

        let result = upsert_all(&conn, batch);

        match result {
            Ok(counts) => {
                conn.execute_batch("COMMIT;")
                    .map_err(|e| format!("commit: {e}"))?;
                Ok(counts)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Record the outcome of a source fetch (freshness + status).
    pub fn update_source_meta(
        &self,
        source: &str,
        status: &str,
        detail: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        conn.execute(
            "INSERT INTO source_meta (source, last_fetched_at, status, detail)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(source) DO UPDATE SET
               last_fetched_at=excluded.last_fetched_at,
               status=excluded.status,
               detail=excluded.detail",
            rusqlite::params![source, now_rfc3339(), status, detail],
        )
        .map_err(|e| format!("update source_meta: {e}"))?;
        Ok(())
    }

    // -- Reads -------------------------------------------------------------

    pub fn read_freshness(&self) -> Result<Vec<SourceMeta>, String> {
        let conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT source, last_fetched_at, status, detail FROM source_meta ORDER BY source")
            .map_err(|e| format!("prepare freshness: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SourceMeta {
                    source: r.get(0)?,
                    last_fetched_at: r.get(1)?,
                    status: r.get(2)?,
                    detail: r.get(3)?,
                })
            })
            .map_err(|e| format!("query freshness: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row freshness: {e}"))?);
        }
        Ok(out)
    }

    /// Assemble the US Market view payload from the stored data.
    pub fn read_us_market(&self) -> Result<UsMarket, String> {
        let freshness = self.read_freshness()?;
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;

        let indices = latest_price_quotes(&conn, "INDEX", INDEX_ORDER)?;
        let sectors = read_sectors(&conn)?;
        let breadth = read_breadth(&conn)?;
        let cot = read_cot(&conn)?;

        let us2y = latest_metric(&conn, &["DGS2"]);
        let us10y = latest_metric(&conn, &["DGS10", "US10Y"]);
        let twos10s = latest_metric(&conn, &["T10Y2Y"]).or_else(|| {
            match (&us2y, &us10y) {
                (Some(a), Some(b)) => Some(MetricPoint {
                    value: b.value - a.value,
                    date: b.date.clone(),
                    series_id: "DGS10-DGS2".to_string(),
                }),
                _ => None,
            }
        });
        let hy_oas = latest_metric(&conn, &["BAMLH0A0HYM2"]);
        let dxy = latest_metric(&conn, &["DTWEXBGS", "DXY"]);
        let vix = latest_metric(&conn, &["VIX", "VIXCLS"]);

        Ok(UsMarket {
            indices,
            sectors,
            breadth,
            rates: RatesSnapshot {
                us2y,
                us10y,
                twos10s,
                hy_oas,
                dxy,
                vix,
            },
            cot,
            freshness,
        })
    }

    /// Assemble the Japan Market view payload.
    pub fn read_jp_market(&self) -> Result<JpMarket, String> {
        let freshness = self.read_freshness()?;
        let jquants_available = freshness
            .iter()
            .any(|m| m.source == "jquants" && m.status == "ok");

        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;

        let indices = latest_price_quotes(&conn, "JP_INDEX", JP_INDEX_ORDER)?;
        let fx = read_fx_quotes(&conn)?;
        let futures_gap = read_futures_gap(&conn)?;
        let investor_flows = read_investor_flows(&conn)?;
        let short_selling = read_short_selling(&conn)?;
        let margin = read_margin(&conn)?;

        Ok(JpMarket {
            indices,
            fx,
            futures_gap,
            investor_flows,
            short_selling,
            margin,
            jquants_available,
            freshness,
        })
    }

    // -- Watchlist ---------------------------------------------------------

    pub fn read_watchlist(&self) -> Result<Vec<WatchItem>, String> {
        let conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT symbol, label, market FROM watchlists ORDER BY sort_order, symbol")
            .map_err(|e| format!("prep watchlist: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(WatchItem {
                    symbol: r.get(0)?,
                    label: r.get(1)?,
                    market: r.get(2)?,
                })
            })
            .map_err(|e| format!("query watchlist: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row watchlist: {e}"))?);
        }
        Ok(out)
    }

    /// Replace the entire watchlist with the given ordered items.
    pub fn set_watchlist(&self, items: &[WatchItem]) -> Result<(), String> {
        let mut conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin watchlist tx: {e}"))?;
        tx.execute("DELETE FROM watchlists", [])
            .map_err(|e| format!("clear watchlist: {e}"))?;
        for (i, item) in items.iter().enumerate() {
            tx.execute(
                "INSERT INTO watchlists (symbol, label, market, sort_order)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![item.symbol, item.label, item.market, i as i64],
            )
            .map_err(|e| format!("insert watchlist: {e}"))?;
        }
        tx.commit().map_err(|e| format!("commit watchlist: {e}"))?;
        Ok(())
    }

    // -- Series reads for the engine (Japan) -------------------------------

    /// FX rate history ascending by date for the composite.
    pub fn read_fx_asc(&self, pair: &str) -> Result<Vec<f64>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT rate FROM fx_rates
                 WHERE pair = ? AND rate IS NOT NULL ORDER BY ts ASC",
            )
            .map_err(|e| format!("prep fx asc: {e}"))?;
        let rows = stmt
            .query_map(duckdb::params![pair], |r| r.get::<_, f64>(0))
            .map_err(|e| format!("query fx asc: {e}"))?;
        collect_f64(rows)
    }

    /// Weekly foreign-investor net (summed across markets) ascending by week.
    pub fn read_foreign_flow_net_asc(&self) -> Result<Vec<f64>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT sum(net) FROM jp_investor_flows
                 WHERE investor_type = 'foreigners' AND net IS NOT NULL
                 GROUP BY week_ending ORDER BY week_ending ASC",
            )
            .map_err(|e| format!("prep foreign flow: {e}"))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, f64>(0))
            .map_err(|e| format!("query foreign flow: {e}"))?;
        collect_f64(rows)
    }

    // -- Disclosures / News / Calendar (Session 4) -------------------------

    pub fn read_news(&self, limit: u32) -> Result<Vec<NewsItem>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT id, source, datetime, title, url, summary, summarized_tier, tickers
                 FROM news ORDER BY datetime DESC LIMIT ?",
            )
            .map_err(|e| format!("prep news read: {e}"))?;
        let rows = stmt
            .query_map(duckdb::params![limit as i64], |r| {
                let tickers: Option<String> = r.get(7)?;
                Ok(NewsItem {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    datetime: r.get(2)?,
                    title: r.get(3)?,
                    url: r.get(4)?,
                    summary: r.get(5)?,
                    summarized_tier: r.get(6)?,
                    tickers: tickers
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|x| x.to_string()).collect())
                        .unwrap_or_default(),
                })
            })
            .map_err(|e| format!("query news read: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row news read: {e}"))?);
        }
        Ok(out)
    }

    pub fn read_disclosures(&self, filter: &FeedFilter) -> Result<Vec<DisclosureItem>, String> {
        let limit = filter.limit.unwrap_or(100).min(500) as i64;
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT id, source, company_code, datetime, doc_type, title, url, summary, summarized_tier
                 FROM disclosures
                 WHERE (? IS NULL OR source = ?)
                 ORDER BY datetime DESC LIMIT ?",
            )
            .map_err(|e| format!("prep disc read: {e}"))?;
        let src = filter.source.clone();
        let rows = stmt
            .query_map(duckdb::params![src, src, limit], |r| {
                Ok(DisclosureItem {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    company_code: r.get(2)?,
                    datetime: r.get(3)?,
                    doc_type: r.get(4)?,
                    title: r.get(5)?,
                    url: r.get(6)?,
                    summary: r.get(7)?,
                    summarized_tier: r.get(8)?,
                })
            })
            .map_err(|e| format!("query disc read: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row disc read: {e}"))?);
        }
        Ok(out)
    }

    pub fn read_calendar_upcoming(&self, limit: u32) -> Result<Vec<CalendarEvent>, String> {
        let today = crate::util::today_utc();
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT id, type, datetime_jst, country, importance, title, actual, forecast, previous
                 FROM calendar_events
                 WHERE substr(datetime_jst, 1, 10) >= ?
                 ORDER BY datetime_jst ASC LIMIT ?",
            )
            .map_err(|e| format!("prep cal read: {e}"))?;
        let rows = stmt
            .query_map(duckdb::params![today, limit as i64], |r| {
                Ok(CalendarEvent {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    datetime_jst: r.get(2)?,
                    country: r.get(3)?,
                    importance: r.get(4)?,
                    title: r.get(5)?,
                    actual: r.get(6)?,
                    forecast: r.get(7)?,
                    previous: r.get(8)?,
                })
            })
            .map_err(|e| format!("query cal read: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row cal read: {e}"))?);
        }
        Ok(out)
    }

    /// Headlines for the brief context.
    pub fn latest_headlines(&self, limit: u32) -> Result<Vec<String>, String> {
        Ok(self.read_news(limit)?.into_iter().map(|n| n.title).collect())
    }

    // -- Alerts ------------------------------------------------------------

    pub fn read_alerts(&self, limit: u32) -> Result<Vec<Alert>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT id, ts, severity, title, triggers, status
                 FROM alerts ORDER BY ts DESC LIMIT ?",
            )
            .map_err(|e| format!("prep alerts read: {e}"))?;
        let rows = stmt
            .query_map(duckdb::params![limit as i64], |r| {
                let triggers_json: Option<String> = r.get(4)?;
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    triggers_json,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| format!("query alerts read: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            let (id, ts, severity, title, triggers_json, status) =
                r.map_err(|e| format!("row alerts read: {e}"))?;
            let triggers: Vec<String> = triggers_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default();
            out.push(Alert {
                id,
                ts,
                severity,
                title,
                triggers,
                status,
            });
        }
        Ok(out)
    }

    pub fn read_alert_rules(&self) -> Result<Vec<AlertRule>, String> {
        let conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT id, name, enabled, condition FROM alert_rules ORDER BY id")
            .map_err(|e| format!("prep rules read: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AlertRule {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    enabled: r.get::<_, i64>(2)? != 0,
                    condition: r.get(3)?,
                })
            })
            .map_err(|e| format!("query rules read: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row rules read: {e}"))?);
        }
        Ok(out)
    }

    /// Most recent alert timestamp for a given title (dedup window check).
    pub fn last_alert_ts(&self, title: &str) -> Result<Option<String>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT max(ts) FROM alerts WHERE title = ?")
            .map_err(|e| format!("prep last alert: {e}"))?;
        let mut rows = stmt
            .query_map(duckdb::params![title], |r| r.get::<_, Option<String>>(0))
            .map_err(|e| format!("query last alert: {e}"))?;
        match rows.next() {
            Some(r) => Ok(r.map_err(|e| format!("row last alert: {e}"))?),
            None => Ok(None),
        }
    }

    pub fn insert_alert(&self, alert: &Alert) -> Result<(), String> {
        let triggers = serde_json::to_string(&alert.triggers)
            .map_err(|e| format!("serialize triggers: {e}"))?;
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        conn.execute(
            "INSERT INTO alerts (id, ts, severity, title, triggers, status)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO NOTHING",
            duckdb::params![
                alert.id, alert.ts, alert.severity, alert.title, triggers, alert.status
            ],
        )
        .map_err(|e| format!("insert alert: {e}"))?;
        Ok(())
    }

    /// Available components (name, label, n) from the latest composite.
    pub fn latest_components(&self) -> Result<Vec<(String, String, f64)>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT components FROM composite_scores ORDER BY ts DESC LIMIT 1")
            .map_err(|e| format!("prep components: {e}"))?;
        let mut rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| format!("query components: {e}"))?;
        let json = match rows.next() {
            Some(r) => r.map_err(|e| format!("row components: {e}"))?,
            None => return Ok(Vec::new()),
        };
        let parsed: Vec<serde_json::Value> =
            serde_json::from_str(&json).map_err(|e| format!("parse components: {e}"))?;
        let mut out = Vec::new();
        for c in parsed {
            if c.get("available").and_then(|v| v.as_bool()) == Some(true) {
                if let (Some(name), Some(n)) = (
                    c.get("name").and_then(|v| v.as_str()),
                    c.get("n").and_then(|v| v.as_f64()),
                ) {
                    let label = c
                        .get("label")
                        .and_then(|v| v.as_str())
                        .unwrap_or(name)
                        .to_string();
                    out.push((name.to_string(), label, n));
                }
            }
        }
        Ok(out)
    }

    // -- Brief -------------------------------------------------------------

    pub fn store_brief(&self, brief: &Brief) -> Result<(), String> {
        let json = serde_json::to_string(brief).map_err(|e| format!("serialize brief: {e}"))?;
        self.set_setting("daily_brief", &json)
    }

    pub fn read_brief(&self) -> Result<Option<Brief>, String> {
        match self.read_setting("daily_brief")? {
            Some(j) => serde_json::from_str(&j)
                .map(Some)
                .map_err(|e| format!("parse brief: {e}")),
            None => Ok(None),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            rusqlite::params![key, value],
        )
        .map_err(|e| format!("set setting: {e}"))?;
        Ok(())
    }

    /// All settings key/value pairs (non-secret; secrets live in the keychain).
    pub fn read_all_settings(&self) -> Result<Vec<SettingKV>, String> {
        let conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings ORDER BY key")
            .map_err(|e| format!("prep settings: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SettingKV {
                    key: r.get(0)?,
                    value: r.get(1)?,
                })
            })
            .map_err(|e| format!("query settings: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row settings: {e}"))?);
        }
        Ok(out)
    }

    // -- Cross-market ------------------------------------------------------

    /// Replace the cross-market snapshot with the current transmissions.
    pub fn replace_cross_market(
        &self,
        ts: &str,
        transmissions: &[CrossTransmission],
    ) -> Result<(), String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        conn.execute_batch("BEGIN TRANSACTION; DELETE FROM cross_market;")
            .map_err(|e| format!("clear cross_market: {e}"))?;
        let res = (|| -> Result<(), String> {
            let mut stmt = conn
                .prepare(
                    "INSERT INTO cross_market (ts, driver, path, effect_note)
                     VALUES (?, ?, ?, ?)
                     ON CONFLICT (ts, driver) DO UPDATE SET
                       path=excluded.path, effect_note=excluded.effect_note",
                )
                .map_err(|e| format!("prep cross: {e}"))?;
            for t in transmissions {
                stmt.execute(duckdb::params![ts, t.driver, t.path, t.effect_note])
                    .map_err(|e| format!("ins cross: {e}"))?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => {
                conn.execute_batch("COMMIT;")
                    .map_err(|e| format!("commit cross: {e}"))?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    pub fn read_cross_market(&self) -> Result<Vec<CrossTransmission>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT driver, path, effect_note FROM cross_market ORDER BY driver")
            .map_err(|e| format!("prep cross read: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(CrossTransmission {
                    driver: r.get(0)?,
                    path: r.get(1)?,
                    effect_note: r.get(2)?,
                })
            })
            .map_err(|e| format!("query cross read: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row cross read: {e}"))?);
        }
        Ok(out)
    }

    // -- Settings ----------------------------------------------------------

    pub fn read_setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.sqlite.lock().map_err(|_| "sqlite lock poisoned")?;
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| format!("prep setting: {e}"))?;
        let mut rows = stmt
            .query_map(rusqlite::params![key], |r| r.get::<_, String>(0))
            .map_err(|e| format!("query setting: {e}"))?;
        match rows.next() {
            Some(r) => Ok(Some(r.map_err(|e| format!("row setting: {e}"))?)),
            None => Ok(None),
        }
    }

    // -- Time-series reads (for the compute engine) ------------------------

    /// Daily closes ascending by date for a stored symbol/market.
    pub fn read_closes_asc(&self, market: &str, symbol: &str) -> Result<Vec<f64>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT close FROM prices
                 WHERE market = ? AND symbol = ? AND close IS NOT NULL ORDER BY ts ASC",
            )
            .map_err(|e| format!("prep closes: {e}"))?;
        let rows = stmt
            .query_map(duckdb::params![market, symbol], |r| r.get::<_, f64>(0))
            .map_err(|e| format!("query closes: {e}"))?;
        collect_f64(rows)
    }

    /// Macro series values ascending by date.
    pub fn read_series_asc(&self, series_id: &str) -> Result<Vec<f64>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT value FROM rates_macro
                 WHERE series_id = ? AND value IS NOT NULL ORDER BY date ASC",
            )
            .map_err(|e| format!("prep series: {e}"))?;
        let rows = stmt
            .query_map(duckdb::params![series_id], |r| r.get::<_, f64>(0))
            .map_err(|e| format!("query series: {e}"))?;
        collect_f64(rows)
    }

    pub fn read_latest_breadth_pct(&self) -> Result<Option<f64>, String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        let mut stmt = conn
            .prepare(
                "SELECT pct_above_200dma FROM us_breadth
                 WHERE pct_above_200dma IS NOT NULL ORDER BY date DESC LIMIT 1",
            )
            .map_err(|e| format!("prep breadth pct: {e}"))?;
        let mut rows = stmt
            .query_map([], |r| r.get::<_, f64>(0))
            .map_err(|e| format!("query breadth pct: {e}"))?;
        match rows.next() {
            Some(r) => Ok(Some(r.map_err(|e| format!("row breadth pct: {e}"))?)),
            None => Ok(None),
        }
    }

    // -- Composite persistence ---------------------------------------------

    /// Persist a computed composite score + its per-signal states.
    pub fn persist_composite(
        &self,
        ts: &str,
        score: f64,
        regime: &str,
        components_json: &str,
        signals: &[(String, Option<f64>, String)],
    ) -> Result<(), String> {
        let conn = self.duck.lock().map_err(|_| "duckdb lock poisoned")?;
        conn.execute_batch("BEGIN TRANSACTION;")
            .map_err(|e| format!("begin composite: {e}"))?;
        let res = (|| -> Result<(), String> {
            conn.execute(
                "INSERT INTO composite_scores (ts, score, regime, components)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT (ts) DO UPDATE SET
                   score=excluded.score, regime=excluded.regime, components=excluded.components",
                duckdb::params![ts, score, regime, components_json],
            )
            .map_err(|e| format!("ins composite: {e}"))?;
            let mut stmt = conn
                .prepare(
                    "INSERT INTO signal_states (ts, signal_name, value, state)
                     VALUES (?, ?, ?, ?)
                     ON CONFLICT (ts, signal_name) DO UPDATE SET
                       value=excluded.value, state=excluded.state",
                )
                .map_err(|e| format!("prep signal_states: {e}"))?;
            for (name, value, state) in signals {
                stmt.execute(duckdb::params![ts, name, value, state])
                    .map_err(|e| format!("ins signal_states: {e}"))?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => {
                conn.execute_batch("COMMIT;")
                    .map_err(|e| format!("commit composite: {e}"))?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }
}

fn collect_f64(
    rows: impl Iterator<Item = Result<f64, duckdb::Error>>,
) -> Result<Vec<f64>, String> {
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("row f64: {e}"))?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Migrations
// ---------------------------------------------------------------------------

fn migrate_duckdb(conn: &duckdb::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _meta (key VARCHAR PRIMARY KEY, value VARCHAR);
         INSERT INTO _meta (key, value) VALUES ('schema_version', '1')
           ON CONFLICT (key) DO UPDATE SET value=excluded.value;

         CREATE TABLE IF NOT EXISTS prices (
           symbol VARCHAR, market VARCHAR, ts DATE,
           open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (symbol, market, ts));

         CREATE TABLE IF NOT EXISTS indices (
           index_code VARCHAR, ts DATE, value DOUBLE, change DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (index_code, ts));

         CREATE TABLE IF NOT EXISTS rates_macro (
           series_id VARCHAR, date DATE, value DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (series_id, date));

         CREATE TABLE IF NOT EXISTS us_breadth (
           date DATE, index_label VARCHAR,
           advancers BIGINT, decliners BIGINT, new_highs BIGINT, new_lows BIGINT,
           pct_above_200dma DOUBLE, universe BIGINT,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (date, index_label));

         CREATE TABLE IF NOT EXISTS sector_perf (
           date DATE, region VARCHAR, sector VARCHAR,
           ret DOUBLE, rel_strength DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (date, region, sector));

         CREATE TABLE IF NOT EXISTS cot (
           date DATE, market VARCHAR,
           comm_long DOUBLE, comm_short DOUBLE,
           noncomm_long DOUBLE, noncomm_short DOUBLE, net DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (date, market));

         CREATE TABLE IF NOT EXISTS composite_scores (
           ts VARCHAR PRIMARY KEY, score DOUBLE, regime VARCHAR, components VARCHAR);

         CREATE TABLE IF NOT EXISTS signal_states (
           ts VARCHAR, signal_name VARCHAR, value DOUBLE, state VARCHAR,
           PRIMARY KEY (ts, signal_name));

         CREATE TABLE IF NOT EXISTS fx_rates (
           pair VARCHAR, ts DATE, rate DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (pair, ts));

         CREATE TABLE IF NOT EXISTS jp_investor_flows (
           week_ending DATE, investor_type VARCHAR, market VARCHAR,
           buy DOUBLE, sell DOUBLE, net DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (week_ending, investor_type, market));

         CREATE TABLE IF NOT EXISTS jp_margin (
           symbol_or_market VARCHAR, week_ending DATE,
           long_balance DOUBLE, short_balance DOUBLE, ratio DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (symbol_or_market, week_ending));

         CREATE TABLE IF NOT EXISTS jp_short_selling (
           date DATE, market VARCHAR, short_ratio DOUBLE,
           source VARCHAR, ingested_at VARCHAR,
           PRIMARY KEY (date, market));

         CREATE TABLE IF NOT EXISTS news (
           id VARCHAR PRIMARY KEY, source VARCHAR, datetime VARCHAR,
           title VARCHAR, url VARCHAR, lang VARCHAR,
           summary VARCHAR, summarized_tier VARCHAR, tickers VARCHAR,
           ingested_at VARCHAR);

         CREATE TABLE IF NOT EXISTS disclosures (
           id VARCHAR PRIMARY KEY, source VARCHAR, company_code VARCHAR,
           datetime VARCHAR, doc_type VARCHAR, title VARCHAR, url VARCHAR,
           summary VARCHAR, summarized_tier VARCHAR, ingested_at VARCHAR);

         CREATE TABLE IF NOT EXISTS calendar_events (
           id VARCHAR PRIMARY KEY, type VARCHAR, datetime_jst VARCHAR,
           country VARCHAR, importance VARCHAR, title VARCHAR,
           actual VARCHAR, forecast VARCHAR, previous VARCHAR, ingested_at VARCHAR);

         CREATE TABLE IF NOT EXISTS alerts (
           id VARCHAR PRIMARY KEY, ts VARCHAR, severity VARCHAR,
           title VARCHAR, triggers VARCHAR, status VARCHAR);

         CREATE TABLE IF NOT EXISTS cross_market (
           ts VARCHAR, driver VARCHAR, path VARCHAR, effect_note VARCHAR,
           PRIMARY KEY (ts, driver));",
    )
    .map_err(|e| format!("migrate duckdb: {e}"))
}

fn migrate_sqlite(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT);
         INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '1');

         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT);

         CREATE TABLE IF NOT EXISTS source_meta (
           source TEXT PRIMARY KEY,
           last_fetched_at TEXT,
           status TEXT,
           detail TEXT);

         CREATE TABLE IF NOT EXISTS watchlists (
           symbol TEXT PRIMARY KEY,
           label TEXT,
           market TEXT,
           sort_order INTEGER);

         CREATE TABLE IF NOT EXISTS alert_rules (
           id TEXT PRIMARY KEY,
           name TEXT,
           enabled INTEGER,
           condition TEXT);",
    )
    .map_err(|e| format!("migrate sqlite: {e}"))
}

fn seed_source_meta(conn: &rusqlite::Connection) -> Result<(), String> {
    for src in KNOWN_SOURCES {
        conn.execute(
            "INSERT INTO source_meta (source, last_fetched_at, status, detail)
             VALUES (?1, NULL, 'never', NULL)
             ON CONFLICT(source) DO NOTHING",
            rusqlite::params![src],
        )
        .map_err(|e| format!("seed source_meta: {e}"))?;
    }
    Ok(())
}

fn seed_watchlist(conn: &rusqlite::Connection) -> Result<(), String> {
    // Only seed when empty, so user edits are preserved.
    let count: i64 = conn
        .query_row("SELECT count(*) FROM watchlists", [], |r| r.get(0))
        .map_err(|e| format!("count watchlists: {e}"))?;
    if count > 0 {
        return Ok(());
    }
    let defaults = [
        ("7203.T", "トヨタ自動車", "JP"),
        ("6758.T", "ソニーグループ", "JP"),
        ("9984.T", "ソフトバンクG", "JP"),
        ("8306.T", "三菱UFJ", "JP"),
        ("6861.T", "キーエンス", "JP"),
    ];
    for (i, (sym, label, mkt)) in defaults.iter().enumerate() {
        conn.execute(
            "INSERT INTO watchlists (symbol, label, market, sort_order)
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT(symbol) DO NOTHING",
            rusqlite::params![sym, label, mkt, i as i64],
        )
        .map_err(|e| format!("seed watchlist: {e}"))?;
    }
    Ok(())
}

fn seed_alert_rules(conn: &rusqlite::Connection) -> Result<(), String> {
    // Default corroboration rule: fire when >= N independent signals agree on a
    // risk direction beyond a threshold (architecture §11).
    let condition = r#"{"type":"corroboration","min_families":3,"threshold":0.3}"#;
    conn.execute(
        "INSERT INTO alert_rules (id, name, enabled, condition)
         VALUES ('corroboration_default', '複数シグナル照合（地合い急変）', 1, ?1)
         ON CONFLICT(id) DO NOTHING",
        rusqlite::params![condition],
    )
    .map_err(|e| format!("seed alert_rules: {e}"))?;
    Ok(())
}

fn seed_settings(conn: &rusqlite::Connection) -> Result<(), String> {
    for (k, v) in [
        ("composite_weights", DEFAULT_WEIGHTS_JSON),
        ("rate_sign", DEFAULT_RATE_SIGN),
        ("alert_min_families", "3"),
        ("alert_threshold", "0.3"),
        ("cross_market_rules", DEFAULT_CROSS_RULES_JSON),
        // AI cost control: "rule" | "local" | "claude". Default avoids paid API.
        ("ai_max_tier", "local"),
    ] {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO NOTHING",
            rusqlite::params![k, v],
        )
        .map_err(|e| format!("seed settings: {e}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Upserts
// ---------------------------------------------------------------------------

fn upsert_all(conn: &duckdb::Connection, batch: &NormalizedBatch) -> Result<UpsertCounts, String> {
    let mut counts = UpsertCounts::default();
    let src = &batch.source;
    let at = &batch.fetched_at;

    if !batch.prices.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO prices (symbol, market, ts, open, high, low, close, volume, source, ingested_at)
                 VALUES (?, ?, CAST(? AS DATE), ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (symbol, market, ts) DO UPDATE SET
                   open=excluded.open, high=excluded.high, low=excluded.low,
                   close=excluded.close, volume=excluded.volume,
                   source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep prices: {e}"))?;
        for r in &batch.prices {
            stmt.execute(duckdb::params![
                r.symbol, r.market, r.ts, r.open, r.high, r.low, r.close, r.volume, src, at
            ])
            .map_err(|e| format!("ins prices: {e}"))?;
            counts.prices += 1;
        }
    }

    if !batch.rates_macro.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO rates_macro (series_id, date, value, source, ingested_at)
                 VALUES (?, CAST(? AS DATE), ?, ?, ?)
                 ON CONFLICT (series_id, date) DO UPDATE SET
                   value=excluded.value, source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep rates: {e}"))?;
        for r in &batch.rates_macro {
            stmt.execute(duckdb::params![r.series_id, r.date, r.value, src, at])
                .map_err(|e| format!("ins rates: {e}"))?;
            counts.rates_macro += 1;
        }
    }

    if !batch.sector_perf.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO sector_perf (date, region, sector, ret, rel_strength, source, ingested_at)
                 VALUES (CAST(? AS DATE), ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (date, region, sector) DO UPDATE SET
                   ret=excluded.ret, rel_strength=excluded.rel_strength,
                   source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep sector: {e}"))?;
        for r in &batch.sector_perf {
            stmt.execute(duckdb::params![
                r.date, r.region, r.sector, r.ret, r.rel_strength, src, at
            ])
            .map_err(|e| format!("ins sector: {e}"))?;
            counts.sector_perf += 1;
        }
    }

    if !batch.us_breadth.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO us_breadth (date, index_label, advancers, decliners, new_highs, new_lows, pct_above_200dma, universe, source, ingested_at)
                 VALUES (CAST(? AS DATE), ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (date, index_label) DO UPDATE SET
                   advancers=excluded.advancers, decliners=excluded.decliners,
                   new_highs=excluded.new_highs, new_lows=excluded.new_lows,
                   pct_above_200dma=excluded.pct_above_200dma, universe=excluded.universe,
                   source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep breadth: {e}"))?;
        for r in &batch.us_breadth {
            stmt.execute(duckdb::params![
                r.date, r.index, r.advancers, r.decliners, r.new_highs, r.new_lows,
                r.pct_above_200dma, r.universe, src, at
            ])
            .map_err(|e| format!("ins breadth: {e}"))?;
            counts.us_breadth += 1;
        }
    }

    if !batch.cot.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO cot (date, market, comm_long, comm_short, noncomm_long, noncomm_short, net, source, ingested_at)
                 VALUES (CAST(? AS DATE), ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (date, market) DO UPDATE SET
                   comm_long=excluded.comm_long, comm_short=excluded.comm_short,
                   noncomm_long=excluded.noncomm_long, noncomm_short=excluded.noncomm_short,
                   net=excluded.net, source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep cot: {e}"))?;
        for r in &batch.cot {
            stmt.execute(duckdb::params![
                r.date, r.market, r.comm_long, r.comm_short, r.noncomm_long, r.noncomm_short, r.net, src, at
            ])
            .map_err(|e| format!("ins cot: {e}"))?;
            counts.cot += 1;
        }
    }

    if !batch.fx_rates.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO fx_rates (pair, ts, rate, source, ingested_at)
                 VALUES (?, CAST(? AS DATE), ?, ?, ?)
                 ON CONFLICT (pair, ts) DO UPDATE SET
                   rate=excluded.rate, source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep fx: {e}"))?;
        for r in &batch.fx_rates {
            stmt.execute(duckdb::params![r.pair, r.ts, r.rate, src, at])
                .map_err(|e| format!("ins fx: {e}"))?;
            counts.fx_rates += 1;
        }
    }

    if !batch.jp_investor_flows.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO jp_investor_flows (week_ending, investor_type, market, buy, sell, net, source, ingested_at)
                 VALUES (CAST(? AS DATE), ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (week_ending, investor_type, market) DO UPDATE SET
                   buy=excluded.buy, sell=excluded.sell, net=excluded.net,
                   source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep flows: {e}"))?;
        for r in &batch.jp_investor_flows {
            stmt.execute(duckdb::params![
                r.week_ending, r.investor_type, r.market, r.buy, r.sell, r.net, src, at
            ])
            .map_err(|e| format!("ins flows: {e}"))?;
            counts.jp_investor_flows += 1;
        }
    }

    if !batch.jp_margin.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO jp_margin (symbol_or_market, week_ending, long_balance, short_balance, ratio, source, ingested_at)
                 VALUES (?, CAST(? AS DATE), ?, ?, ?, ?, ?)
                 ON CONFLICT (symbol_or_market, week_ending) DO UPDATE SET
                   long_balance=excluded.long_balance, short_balance=excluded.short_balance,
                   ratio=excluded.ratio, source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep margin: {e}"))?;
        for r in &batch.jp_margin {
            stmt.execute(duckdb::params![
                r.symbol_or_market, r.week_ending, r.long_balance, r.short_balance, r.ratio, src, at
            ])
            .map_err(|e| format!("ins margin: {e}"))?;
            counts.jp_margin += 1;
        }
    }

    if !batch.jp_short_selling.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO jp_short_selling (date, market, short_ratio, source, ingested_at)
                 VALUES (CAST(? AS DATE), ?, ?, ?, ?)
                 ON CONFLICT (date, market) DO UPDATE SET
                   short_ratio=excluded.short_ratio, source=excluded.source, ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep short: {e}"))?;
        for r in &batch.jp_short_selling {
            stmt.execute(duckdb::params![r.date, r.market, r.short_ratio, src, at])
                .map_err(|e| format!("ins short: {e}"))?;
            counts.jp_short_selling += 1;
        }
    }

    if !batch.news.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO news (id, source, datetime, title, url, lang, summary, summarized_tier, tickers, ingested_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (id) DO UPDATE SET
                   summary=excluded.summary, summarized_tier=excluded.summarized_tier,
                   ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep news: {e}"))?;
        for r in &batch.news {
            let tickers = r.tickers.join(",");
            stmt.execute(duckdb::params![
                r.id, r.source, r.datetime, r.title, r.url, r.lang,
                r.summary, r.summarized_tier, tickers, at
            ])
            .map_err(|e| format!("ins news: {e}"))?;
            counts.news += 1;
        }
    }

    if !batch.disclosures.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO disclosures (id, source, company_code, datetime, doc_type, title, url, summary, summarized_tier, ingested_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (id) DO UPDATE SET
                   summary=excluded.summary, summarized_tier=excluded.summarized_tier,
                   ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep disclosures: {e}"))?;
        for r in &batch.disclosures {
            stmt.execute(duckdb::params![
                r.id, r.source, r.company_code, r.datetime, r.doc_type, r.title, r.url,
                r.summary, r.summarized_tier, at
            ])
            .map_err(|e| format!("ins disclosures: {e}"))?;
            counts.disclosures += 1;
        }
    }

    if !batch.calendar_events.is_empty() {
        let mut stmt = conn
            .prepare(
                "INSERT INTO calendar_events (id, type, datetime_jst, country, importance, title, actual, forecast, previous, ingested_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (id) DO UPDATE SET
                   actual=excluded.actual, forecast=excluded.forecast, previous=excluded.previous,
                   ingested_at=excluded.ingested_at",
            )
            .map_err(|e| format!("prep calendar: {e}"))?;
        for r in &batch.calendar_events {
            stmt.execute(duckdb::params![
                r.id, r.kind, r.datetime_jst, r.country, r.importance, r.title,
                r.actual, r.forecast, r.previous, at
            ])
            .map_err(|e| format!("ins calendar: {e}"))?;
            counts.calendar_events += 1;
        }
    }

    Ok(counts)
}

// ---------------------------------------------------------------------------
// Read helpers
// ---------------------------------------------------------------------------

fn latest_metric(conn: &duckdb::Connection, series: &[&str]) -> Option<MetricPoint> {
    for sid in series {
        let mut stmt = match conn.prepare(
            "SELECT value, CAST(date AS VARCHAR) FROM rates_macro
             WHERE series_id = ? AND value IS NOT NULL ORDER BY date DESC LIMIT 1",
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut rows = match stmt.query_map(duckdb::params![sid], |r| {
            Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?))
        }) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let Some(Ok((value, date))) = rows.next() {
            return Some(MetricPoint {
                value,
                date,
                series_id: (*sid).to_string(),
            });
        }
    }
    None
}

/// Latest close + day-over-day change% per symbol within a price `market`.
fn latest_price_quotes(
    conn: &duckdb::Connection,
    market: &str,
    order: &[&str],
) -> Result<Vec<IndexQuote>, String> {
    let mut stmt = conn
        .prepare(
            "WITH ranked AS (
               SELECT symbol, ts, close,
                 ROW_NUMBER() OVER (PARTITION BY symbol ORDER BY ts DESC) rn,
                 LEAD(close) OVER (PARTITION BY symbol ORDER BY ts DESC) prev
               FROM prices WHERE market = ? AND close IS NOT NULL)
             SELECT symbol, CAST(ts AS VARCHAR), close, prev FROM ranked WHERE rn=1",
        )
        .map_err(|e| format!("prep quotes: {e}"))?;
    let rows = stmt
        .query_map(duckdb::params![market], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<f64>>(3)?,
            ))
        })
        .map_err(|e| format!("query quotes: {e}"))?;

    let mut out: Vec<IndexQuote> = Vec::new();
    for row in rows {
        let (code, ts, close, prev) = row.map_err(|e| format!("row quotes: {e}"))?;
        let change_pct = match (close, prev) {
            (Some(c), Some(p)) if p != 0.0 => Some((c / p - 1.0) * 100.0),
            _ => None,
        };
        out.push(IndexQuote {
            code,
            value: close,
            change_pct,
            ts: Some(ts),
        });
    }
    out.sort_by_key(|q| order.iter().position(|c| *c == q.code).unwrap_or(usize::MAX));
    Ok(out)
}

fn read_fx_quotes(conn: &duckdb::Connection) -> Result<Vec<FxQuote>, String> {
    let mut stmt = conn
        .prepare(
            "WITH ranked AS (
               SELECT pair, ts, rate,
                 ROW_NUMBER() OVER (PARTITION BY pair ORDER BY ts DESC) rn,
                 LEAD(rate) OVER (PARTITION BY pair ORDER BY ts DESC) prev
               FROM fx_rates WHERE rate IS NOT NULL)
             SELECT pair, CAST(ts AS VARCHAR), rate, prev FROM ranked WHERE rn=1",
        )
        .map_err(|e| format!("prep fx quotes: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<f64>>(3)?,
            ))
        })
        .map_err(|e| format!("query fx quotes: {e}"))?;
    let order = ["USDJPY", "EURJPY"];
    let mut out = Vec::new();
    for row in rows {
        let (pair, ts, rate, prev) = row.map_err(|e| format!("row fx quotes: {e}"))?;
        let change_pct = match (rate, prev) {
            (Some(c), Some(p)) if p != 0.0 => Some((c / p - 1.0) * 100.0),
            _ => None,
        };
        out.push(FxQuote {
            pair,
            rate,
            change_pct,
            ts: Some(ts),
        });
    }
    out.sort_by_key(|q| order.iter().position(|p| *p == q.pair).unwrap_or(usize::MAX));
    Ok(out)
}

/// Overnight gap hint: latest Nikkei futures vs the latest TSE spot close.
fn read_futures_gap(conn: &duckdb::Connection) -> Result<Option<FuturesGap>, String> {
    let fut = latest_close(conn, "JP_FUT", "NKD")?;
    // Prefer official TOPIX-less Nikkei spot; N225 is the spot index.
    let spot = latest_close(conn, "JP_INDEX", "N225")?;
    let (futures_value, futures_ts) = match fut {
        Some((v, ts)) => (Some(v), Some(ts)),
        None => (None, None),
    };
    let spot_prev_close = spot.as_ref().map(|(v, _)| *v);
    let gap_pct = match (futures_value, spot_prev_close) {
        (Some(f), Some(s)) if s != 0.0 => Some((f / s - 1.0) * 100.0),
        _ => None,
    };
    if futures_value.is_none() && spot_prev_close.is_none() {
        return Ok(None);
    }
    Ok(Some(FuturesGap {
        futures_code: "NKD".to_string(),
        futures_value,
        futures_ts,
        spot_code: "N225".to_string(),
        spot_prev_close,
        gap_pct,
    }))
}

fn latest_close(
    conn: &duckdb::Connection,
    market: &str,
    symbol: &str,
) -> Result<Option<(f64, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT close, CAST(ts AS VARCHAR) FROM prices
             WHERE market = ? AND symbol = ? AND close IS NOT NULL ORDER BY ts DESC LIMIT 1",
        )
        .map_err(|e| format!("prep latest_close: {e}"))?;
    let mut rows = stmt
        .query_map(duckdb::params![market, symbol], |r| {
            Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("query latest_close: {e}"))?;
    match rows.next() {
        Some(r) => Ok(Some(r.map_err(|e| format!("row latest_close: {e}"))?)),
        None => Ok(None),
    }
}

/// Latest week's investor flows, aggregated across markets per investor type.
fn read_investor_flows(conn: &duckdb::Connection) -> Result<Vec<InvestorFlowQuote>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT CAST(week_ending AS VARCHAR), investor_type,
                    sum(net), sum(buy), sum(sell)
             FROM jp_investor_flows
             WHERE week_ending = (SELECT max(week_ending) FROM jp_investor_flows)
             GROUP BY week_ending, investor_type
             ORDER BY abs(sum(net)) DESC NULLS LAST",
        )
        .map_err(|e| format!("prep flows read: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InvestorFlowQuote {
                week_ending: r.get(0)?,
                investor_type: r.get(1)?,
                market: "all".to_string(),
                net: r.get(2)?,
                buy: r.get(3)?,
                sell: r.get(4)?,
            })
        })
        .map_err(|e| format!("query flows read: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("row flows read: {e}"))?);
    }
    Ok(out)
}

fn read_short_selling(conn: &duckdb::Connection) -> Result<Vec<ShortSellingQuote>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT CAST(date AS VARCHAR), market, short_ratio FROM jp_short_selling
             WHERE date = (SELECT max(date) FROM jp_short_selling)
             ORDER BY short_ratio DESC NULLS LAST LIMIT 12",
        )
        .map_err(|e| format!("prep short read: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ShortSellingQuote {
                date: r.get(0)?,
                market: r.get(1)?,
                short_ratio: r.get(2)?,
            })
        })
        .map_err(|e| format!("query short read: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("row short read: {e}"))?);
    }
    Ok(out)
}

fn read_margin(conn: &duckdb::Connection) -> Result<Vec<MarginQuote>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT symbol_or_market, CAST(week_ending AS VARCHAR),
                    long_balance, short_balance, ratio
             FROM jp_margin
             WHERE week_ending = (SELECT max(week_ending) FROM jp_margin)
             ORDER BY symbol_or_market LIMIT 20",
        )
        .map_err(|e| format!("prep margin read: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MarginQuote {
                symbol_or_market: r.get(0)?,
                week_ending: r.get(1)?,
                long_balance: r.get(2)?,
                short_balance: r.get(3)?,
                ratio: r.get(4)?,
            })
        })
        .map_err(|e| format!("query margin read: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("row margin read: {e}"))?);
    }
    Ok(out)
}

fn read_sectors(conn: &duckdb::Connection) -> Result<Vec<SectorQuote>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT sector, ret, rel_strength, CAST(date AS VARCHAR)
             FROM sector_perf
             WHERE region='US' AND date = (SELECT max(date) FROM sector_perf WHERE region='US')
             ORDER BY rel_strength DESC NULLS LAST",
        )
        .map_err(|e| format!("prep sectors: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SectorQuote {
                sector: r.get(0)?,
                ret: r.get(1)?,
                rel_strength: r.get(2)?,
                date: r.get(3)?,
            })
        })
        .map_err(|e| format!("query sectors: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("row sectors: {e}"))?);
    }
    Ok(out)
}

fn read_breadth(conn: &duckdb::Connection) -> Result<Option<BreadthQuote>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT CAST(date AS VARCHAR), index_label, advancers, decliners,
                    new_highs, new_lows, pct_above_200dma, universe
             FROM us_breadth ORDER BY date DESC LIMIT 1",
        )
        .map_err(|e| format!("prep breadth: {e}"))?;
    let mut rows = stmt
        .query_map([], |r| {
            Ok(BreadthQuote {
                date: r.get(0)?,
                index: r.get(1)?,
                advancers: r.get(2)?,
                decliners: r.get(3)?,
                new_highs: r.get(4)?,
                new_lows: r.get(5)?,
                pct_above_200dma: r.get(6)?,
                universe: r.get(7)?,
            })
        })
        .map_err(|e| format!("query breadth: {e}"))?;
    match rows.next() {
        Some(r) => Ok(Some(r.map_err(|e| format!("row breadth: {e}"))?)),
        None => Ok(None),
    }
}

fn read_cot(conn: &duckdb::Connection) -> Result<Vec<CotQuote>, String> {
    let mut stmt = conn
        .prepare(
            "WITH ranked AS (
               SELECT market, date, net, noncomm_long, noncomm_short,
                 ROW_NUMBER() OVER (PARTITION BY market ORDER BY date DESC) rn,
                 LEAD(net) OVER (PARTITION BY market ORDER BY date DESC) prev_net
               FROM cot)
             SELECT market, CAST(date AS VARCHAR), net, prev_net, noncomm_long, noncomm_short
             FROM ranked WHERE rn=1 ORDER BY market",
        )
        .map_err(|e| format!("prep cot: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(CotQuote {
                market: r.get(0)?,
                date: r.get(1)?,
                noncomm_net: r.get(2)?,
                noncomm_net_prev: r.get(3)?,
                noncomm_long: r.get(4)?,
                noncomm_short: r.get(5)?,
            })
        })
        .map_err(|e| format!("query cot: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("row cot: {e}"))?);
    }
    Ok(out)
}
