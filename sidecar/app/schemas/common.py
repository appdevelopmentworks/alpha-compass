"""Common normalized schema returned by every adapter.

Adapters fetch source-specific data and return a :class:`NormalizedBatch`.
Rust deserializes this, upserts each non-empty list into the matching DuckDB
table, and updates source freshness. Source differences must NOT leak past this
boundary (architecture §6).

All timestamps are ISO-8601 strings; dates are ``YYYY-MM-DD``.
"""

from __future__ import annotations

from pydantic import BaseModel, Field


class PriceRow(BaseModel):
    symbol: str
    market: str  # "INDEX" | "ETF" | "US" | ...
    ts: str  # date (YYYY-MM-DD) for daily bars
    open: float | None = None
    high: float | None = None
    low: float | None = None
    close: float | None = None
    volume: float | None = None


class IndexRow(BaseModel):
    index_code: str
    ts: str
    value: float | None = None
    change: float | None = None  # percent change vs previous close


class RateMacroRow(BaseModel):
    series_id: str
    date: str
    value: float | None = None


class SectorPerfRow(BaseModel):
    date: str
    region: str  # "US"
    sector: str
    ret: float | None = None  # window return (fraction, e.g. 0.012 = 1.2%)
    rel_strength: float | None = None  # sector ret minus benchmark ret


class BreadthRow(BaseModel):
    date: str
    index: str  # label of the basket the breadth was computed over
    advancers: int | None = None
    decliners: int | None = None
    new_highs: int | None = None
    new_lows: int | None = None
    pct_above_200dma: float | None = None  # fraction in [0, 1]
    universe: int | None = None  # size of the basket actually evaluated


class CotRow(BaseModel):
    date: str
    market: str
    comm_long: float | None = None
    comm_short: float | None = None
    noncomm_long: float | None = None
    noncomm_short: float | None = None
    net: float | None = None  # non-commercial net (long - short)


class FxRow(BaseModel):
    pair: str  # "USDJPY" | "EURJPY"
    ts: str
    rate: float | None = None


class JpInvestorFlowRow(BaseModel):
    week_ending: str
    investor_type: str  # "foreigners" | "individuals" | "trust_banks" | ...
    market: str  # e.g. "TSEPrime" / "total"
    buy: float | None = None
    sell: float | None = None
    net: float | None = None  # purchases - sales


class JpMarginRow(BaseModel):
    symbol_or_market: str
    week_ending: str
    long_balance: float | None = None
    short_balance: float | None = None
    ratio: float | None = None  # long / short


class JpShortSellingRow(BaseModel):
    date: str
    market: str
    short_ratio: float | None = None  # fraction or percent of turnover


class NewsRow(BaseModel):
    id: str
    source: str
    datetime: str
    title: str
    url: str
    lang: str = "ja"
    summary: str | None = None
    summarized_tier: str | None = None
    tickers: list[str] = Field(default_factory=list)


class DisclosureRow(BaseModel):
    id: str
    source: str  # "EDINET" | "TDnet"
    company_code: str | None = None
    datetime: str
    doc_type: str | None = None
    title: str
    url: str | None = None
    summary: str | None = None
    summarized_tier: str | None = None


class CalendarEventRow(BaseModel):
    id: str
    type: str  # "econ" | "earnings" | "fomc" | "boj"
    datetime_jst: str
    country: str  # "JP" | "US"
    importance: str  # "high" | "medium" | "low"
    title: str
    actual: str | None = None
    forecast: str | None = None
    previous: str | None = None


class NormalizedBatch(BaseModel):
    """Uniform fetch result. Empty lists are simply not upserted by Rust."""

    source: str
    fetched_at: str  # ISO-8601 UTC
    ok: bool = True
    prices: list[PriceRow] = Field(default_factory=list)
    indices: list[IndexRow] = Field(default_factory=list)
    rates_macro: list[RateMacroRow] = Field(default_factory=list)
    sector_perf: list[SectorPerfRow] = Field(default_factory=list)
    us_breadth: list[BreadthRow] = Field(default_factory=list)
    cot: list[CotRow] = Field(default_factory=list)
    fx_rates: list[FxRow] = Field(default_factory=list)
    jp_investor_flows: list[JpInvestorFlowRow] = Field(default_factory=list)
    jp_margin: list[JpMarginRow] = Field(default_factory=list)
    jp_short_selling: list[JpShortSellingRow] = Field(default_factory=list)
    news: list[NewsRow] = Field(default_factory=list)
    disclosures: list[DisclosureRow] = Field(default_factory=list)
    calendar_events: list[CalendarEventRow] = Field(default_factory=list)
    # Human-readable warnings surfaced to the UI (e.g. "FRED key missing").
    notes: list[str] = Field(default_factory=list)


class FetchRequest(BaseModel):
    """Optional parameters passed by Rust. Adapters fall back to US defaults."""

    symbols: list[str] | None = None
    series: list[str] | None = None
    markets: list[str] | None = None
    api_key: str | None = None
    lookback_days: int | None = None
