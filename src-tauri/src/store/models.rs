//! Data models for the store layer.
//!
//! Two groups:
//! - *Inbound* (`Deserialize`): mirror the sidecar's `NormalizedBatch`.
//! - *Outbound* (`Serialize`): shapes returned to the frontend via IPC.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Inbound: normalized batch from the sidecar (architecture §5/§6).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PriceRow {
    pub symbol: String,
    pub market: String,
    pub ts: String,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub close: Option<f64>,
    pub volume: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateMacroRow {
    pub series_id: String,
    pub date: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SectorPerfRow {
    pub date: String,
    pub region: String,
    pub sector: String,
    pub ret: Option<f64>,
    pub rel_strength: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BreadthRow {
    pub date: String,
    pub index: String,
    pub advancers: Option<i64>,
    pub decliners: Option<i64>,
    pub new_highs: Option<i64>,
    pub new_lows: Option<i64>,
    pub pct_above_200dma: Option<f64>,
    pub universe: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CotRow {
    pub date: String,
    pub market: String,
    pub comm_long: Option<f64>,
    pub comm_short: Option<f64>,
    pub noncomm_long: Option<f64>,
    pub noncomm_short: Option<f64>,
    pub net: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FxRow {
    pub pair: String,
    pub ts: String,
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JpInvestorFlowRow {
    pub week_ending: String,
    pub investor_type: String,
    pub market: String,
    pub buy: Option<f64>,
    pub sell: Option<f64>,
    pub net: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JpMarginRow {
    pub symbol_or_market: String,
    pub week_ending: String,
    pub long_balance: Option<f64>,
    pub short_balance: Option<f64>,
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JpShortSellingRow {
    pub date: String,
    pub market: String,
    pub short_ratio: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewsRow {
    pub id: String,
    pub source: String,
    pub datetime: String,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub lang: String,
    pub summary: Option<String>,
    pub summarized_tier: Option<String>,
    #[serde(default)]
    pub tickers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DisclosureRow {
    pub id: String,
    pub source: String,
    pub company_code: Option<String>,
    pub datetime: String,
    pub doc_type: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub summarized_tier: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalendarEventRow {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub datetime_jst: String,
    pub country: String,
    pub importance: String,
    pub title: String,
    pub actual: Option<String>,
    pub forecast: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NormalizedBatch {
    pub source: String,
    pub fetched_at: String,
    #[serde(default = "default_true")]
    pub ok: bool,
    #[serde(default)]
    pub prices: Vec<PriceRow>,
    #[serde(default)]
    pub rates_macro: Vec<RateMacroRow>,
    #[serde(default)]
    pub sector_perf: Vec<SectorPerfRow>,
    #[serde(default)]
    pub us_breadth: Vec<BreadthRow>,
    #[serde(default)]
    pub cot: Vec<CotRow>,
    #[serde(default)]
    pub fx_rates: Vec<FxRow>,
    #[serde(default)]
    pub jp_investor_flows: Vec<JpInvestorFlowRow>,
    #[serde(default)]
    pub jp_margin: Vec<JpMarginRow>,
    #[serde(default)]
    pub jp_short_selling: Vec<JpShortSellingRow>,
    #[serde(default)]
    pub news: Vec<NewsRow>,
    #[serde(default)]
    pub disclosures: Vec<DisclosureRow>,
    #[serde(default)]
    pub calendar_events: Vec<CalendarEventRow>,
    #[serde(default)]
    pub notes: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl NormalizedBatch {
    /// Total rows across every data array — the single source of truth for
    /// "did this batch carry data" (avoids per-call-site undercounting).
    pub fn total_rows(&self) -> usize {
        self.prices.len()
            + self.rates_macro.len()
            + self.sector_perf.len()
            + self.us_breadth.len()
            + self.cot.len()
            + self.fx_rates.len()
            + self.jp_investor_flows.len()
            + self.jp_margin.len()
            + self.jp_short_selling.len()
            + self.news.len()
            + self.disclosures.len()
            + self.calendar_events.len()
    }
}

/// Row counts written by an upsert, for logging / acks.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpsertCounts {
    pub prices: usize,
    pub rates_macro: usize,
    pub sector_perf: usize,
    pub us_breadth: usize,
    pub cot: usize,
    pub fx_rates: usize,
    pub jp_investor_flows: usize,
    pub jp_margin: usize,
    pub jp_short_selling: usize,
    pub news: usize,
    pub disclosures: usize,
    pub calendar_events: usize,
}

// ---------------------------------------------------------------------------
// Outbound: shapes for the US Market view + freshness.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SourceMeta {
    pub source: String,
    pub last_fetched_at: Option<String>,
    pub status: String, // "ok" | "error" | "never"
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexQuote {
    pub code: String,
    pub value: Option<f64>,
    pub change_pct: Option<f64>,
    pub ts: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SectorQuote {
    pub sector: String,
    pub ret: Option<f64>,
    pub rel_strength: Option<f64>,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BreadthQuote {
    pub date: Option<String>,
    pub index: String,
    pub advancers: Option<i64>,
    pub decliners: Option<i64>,
    pub new_highs: Option<i64>,
    pub new_lows: Option<i64>,
    pub pct_above_200dma: Option<f64>,
    pub universe: Option<i64>,
}

/// A macro metric with the date it is as-of.
#[derive(Debug, Clone, Serialize)]
pub struct MetricPoint {
    pub value: f64,
    pub date: String,
    /// series_id the value actually came from (e.g. FRED vs. yfinance proxy).
    pub series_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RatesSnapshot {
    pub us2y: Option<MetricPoint>,
    pub us10y: Option<MetricPoint>,
    pub twos10s: Option<MetricPoint>,
    pub hy_oas: Option<MetricPoint>,
    pub dxy: Option<MetricPoint>,
    pub vix: Option<MetricPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CotQuote {
    pub market: String,
    pub date: Option<String>,
    pub noncomm_net: Option<f64>,
    pub noncomm_net_prev: Option<f64>,
    pub noncomm_long: Option<f64>,
    pub noncomm_short: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsMarket {
    pub indices: Vec<IndexQuote>,
    pub sectors: Vec<SectorQuote>,
    pub breadth: Option<BreadthQuote>,
    pub rates: RatesSnapshot,
    pub cot: Vec<CotQuote>,
    pub freshness: Vec<SourceMeta>,
}

// ---------------------------------------------------------------------------
// Outbound: Japan Market view.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct FxQuote {
    pub pair: String,
    pub rate: Option<f64>,
    pub change_pct: Option<f64>,
    pub ts: Option<String>,
}

/// Overnight gap hint: CME Nikkei futures vs the prior TSE close.
#[derive(Debug, Clone, Serialize)]
pub struct FuturesGap {
    pub futures_code: String,
    pub futures_value: Option<f64>,
    pub futures_ts: Option<String>,
    pub spot_code: String,
    pub spot_prev_close: Option<f64>,
    pub gap_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvestorFlowQuote {
    pub week_ending: String,
    pub investor_type: String,
    pub market: String,
    pub net: Option<f64>,
    pub buy: Option<f64>,
    pub sell: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShortSellingQuote {
    pub date: String,
    pub market: String,
    pub short_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarginQuote {
    pub symbol_or_market: String,
    pub week_ending: String,
    pub long_balance: Option<f64>,
    pub short_balance: Option<f64>,
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JpMarket {
    pub indices: Vec<IndexQuote>,
    pub fx: Vec<FxQuote>,
    pub futures_gap: Option<FuturesGap>,
    pub investor_flows: Vec<InvestorFlowQuote>,
    pub short_selling: Vec<ShortSellingQuote>,
    pub margin: Vec<MarginQuote>,
    pub jquants_available: bool,
    pub freshness: Vec<SourceMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchItem {
    pub symbol: String,
    pub label: String,
    pub market: String,
}

// ---------------------------------------------------------------------------
// Outbound: Disclosures / News / Calendar / Alerts (Session 4).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct NewsItem {
    pub id: String,
    pub source: String,
    pub datetime: String,
    pub title: String,
    pub url: String,
    pub summary: Option<String>,
    pub summarized_tier: Option<String>,
    pub tickers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisclosureItem {
    pub id: String,
    pub source: String,
    pub company_code: Option<String>,
    pub datetime: String,
    pub doc_type: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub summarized_tier: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub datetime_jst: String,
    pub country: String,
    pub importance: String,
    pub title: String,
    pub actual: Option<String>,
    pub forecast: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Alert {
    pub id: String,
    pub ts: String,
    pub severity: String,
    pub title: String,
    pub triggers: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub condition: String, // JSON
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    pub text: String,
    pub tier: String,
    pub generated_at: String,
}

/// Filter for the disclosures/news feed.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FeedFilter {
    pub kind: Option<String>,   // "news" | "disclosure" | None (both)
    pub source: Option<String>, // source filter
    pub limit: Option<u32>,
}

// ---------------------------------------------------------------------------
// Cross-market transmission (Session 5).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct CrossTransmission {
    pub driver: String,      // observed premise
    pub path: String,        // likely sector tilt
    pub effect_note: String, // the observed values behind it
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossMarket {
    pub transmissions: Vec<CrossTransmission>,
    pub metrics: Vec<MetricLabel>,
    pub freshness: Vec<SourceMeta>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricLabel {
    pub label: String,
    pub value: Option<f64>,
}

/// A single settings key/value (non-secret) for the Settings view.
#[derive(Debug, Clone, Serialize)]
pub struct SettingKV {
    pub key: String,
    pub value: String,
}

/// Whether a credential is present in the OS keychain (value never exposed).
#[derive(Debug, Clone, Serialize)]
pub struct CredentialStatus {
    pub source: String,
    pub configured: bool,
}
