"""yfinance adapter: US indices, volatility, dollar, 10y, sectors, breadth.

Stores full daily history for indices and macro series (so the Rust compute
engine can derive 200DMA / z-scores later), and emits latest-snapshot sector
relative strength and a keyless breadth proxy computed over a fixed large-cap
basket. Source semantics are normalized here; Rust never sees Yahoo specifics.
"""

from __future__ import annotations

from datetime import datetime, timezone
from statistics import fmean

from app.adapters._daily import Bar, download_daily
from app.schemas.common import (
    BreadthRow,
    FetchRequest,
    NormalizedBatch,
    PriceRow,
    RateMacroRow,
    SectorPerfRow,
)

# yahoo symbol -> canonical index code stored in `prices` (market="INDEX")
INDEX_CODES: dict[str, str] = {
    "^GSPC": "SPX",
    "^IXIC": "COMP",
    "^DJI": "DJI",
    "^RUT": "RUT",
}

# Japanese indices (keyless). TOPIX has no clean Yahoo index, so the 1306 ETF
# is used as a direction proxy (labeled as such in the UI).
JP_INDEX_CODES: dict[str, str] = {
    "^N225": "N225",
    "1306.T": "TOPIX_ETF",
}

# CME Nikkei 225 futures, for the overnight gap hint vs the TSE close.
JP_FUTURES: dict[str, str] = {
    "NKD=F": "NKD",
}

# SPDR sector ETFs -> display sector name (Japanese UI labels them downstream)
SECTOR_ETFS: dict[str, str] = {
    "XLK": "Technology",
    "XLF": "Financials",
    "XLE": "Energy",
    "XLV": "Health Care",
    "XLI": "Industrials",
    "XLY": "Consumer Discretionary",
    "XLP": "Consumer Staples",
    "XLB": "Materials",
    "XLRE": "Real Estate",
    "XLU": "Utilities",
    "XLC": "Communication Services",
}
BENCHMARK = "SPY"

# Fixed large-cap basket for the keyless breadth proxy.
BREADTH_BASKET: list[str] = [
    "AAPL", "MSFT", "NVDA", "AMZN", "GOOGL", "META", "TSLA", "JPM", "V", "UNH",
    "XOM", "JNJ", "WMT", "MA", "PG", "HD", "CVX", "MRK", "ABBV", "KO",
    "PEP", "BAC", "AVGO", "COST", "MCD", "CSCO", "ADBE", "CRM", "NFLX", "INTC",
]

_REL_WINDOW = 21  # ~1 trading month
_DMA = 200
_HILO = 252  # ~52 weeks


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _closes(bars: list[Bar]) -> list[float]:
    return [b.close for b in bars if b.close is not None]


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    notes: list[str] = []
    symbols = (
        list(INDEX_CODES)
        + list(JP_INDEX_CODES)
        + list(JP_FUTURES)
        + ["^VIX", "DX-Y.NYB", "^TNX", BENCHMARK]
        + list(SECTOR_ETFS)
        + BREADTH_BASKET
    )
    bars = download_daily(symbols, period="2y")

    if not bars:
        return NormalizedBatch(
            source="yfinance",
            fetched_at=_now(),
            ok=False,
            notes=["yfinance/stooq から US データを取得できませんでした（レート制限の可能性）。"],
        )

    prices: list[PriceRow] = []
    rates: list[RateMacroRow] = []
    sectors: list[SectorPerfRow] = []
    breadth: list[BreadthRow] = []

    # --- Indices: store full daily history in prices (market=INDEX) ---
    for ysym, code in INDEX_CODES.items():
        for b in bars.get(ysym, []):
            prices.append(
                PriceRow(
                    symbol=code, market="INDEX", ts=b.date,
                    open=b.open, high=b.high, low=b.low, close=b.close, volume=b.volume,
                )
            )
        if ysym not in bars:
            notes.append(f"指数 {code} を取得できませんでした。")

    # --- Japanese indices + Nikkei futures (keyless; for the Japan view) ---
    for ysym, code in JP_INDEX_CODES.items():
        for b in bars.get(ysym, []):
            prices.append(
                PriceRow(
                    symbol=code, market="JP_INDEX", ts=b.date,
                    open=b.open, high=b.high, low=b.low, close=b.close, volume=b.volume,
                )
            )
    for ysym, code in JP_FUTURES.items():
        for b in bars.get(ysym, []):
            prices.append(
                PriceRow(
                    symbol=code, market="JP_FUT", ts=b.date,
                    open=b.open, high=b.high, low=b.low, close=b.close, volume=b.volume,
                )
            )

    # --- Macro series: VIX, DXY, US10Y (history -> rates_macro) ---
    for b in bars.get("^VIX", []):
        if b.close is not None:
            rates.append(RateMacroRow(series_id="VIX", date=b.date, value=b.close))
    for b in bars.get("DX-Y.NYB", []):
        if b.close is not None:
            rates.append(RateMacroRow(series_id="DXY", date=b.date, value=b.close))
    # ^TNX already quotes the 10y yield in percent (e.g. 4.45 = 4.45%).
    for b in bars.get("^TNX", []):
        if b.close is not None:
            rates.append(RateMacroRow(series_id="US10Y", date=b.date, value=b.close))
    if "^VIX" not in bars:
        notes.append("VIX を取得できませんでした。")
    if "DX-Y.NYB" not in bars:
        notes.append("ドルインデックス(DXY) を取得できませんでした（FRED で補完可）。")
    if "^TNX" not in bars:
        notes.append("米10年金利を取得できませんでした（FRED で補完可）。")

    # --- Sector relative strength vs SPY over ~1 month ---
    bench_closes = _closes(bars.get(BENCHMARK, []))
    bench_ret = None
    if len(bench_closes) > _REL_WINDOW:
        bench_ret = bench_closes[-1] / bench_closes[-1 - _REL_WINDOW] - 1.0
    latest_date = None
    for ysym, name in SECTOR_ETFS.items():
        sb = bars.get(ysym, [])
        cl = _closes(sb)
        if len(cl) <= _REL_WINDOW:
            continue
        ret = cl[-1] / cl[-1 - _REL_WINDOW] - 1.0
        rel = (ret - bench_ret) if bench_ret is not None else None
        d = sb[-1].date
        latest_date = d
        sectors.append(
            SectorPerfRow(date=d, region="US", sector=name, ret=ret, rel_strength=rel)
        )
    if bench_ret is None:
        notes.append("ベンチマーク(SPY) 不足のためセクター相対強弱は絶対リターンのみ。")

    # --- Breadth proxy over the large-cap basket ---
    adv = dec = nh = nl = above = universe = 0
    bdate = latest_date
    for sym in BREADTH_BASKET:
        cl = _closes(bars.get(sym, []))
        if len(cl) < 2:
            continue
        universe += 1
        bdate = bars[sym][-1].date if sym in bars else bdate
        if cl[-1] > cl[-2]:
            adv += 1
        elif cl[-1] < cl[-2]:
            dec += 1
        window = cl[-_HILO:]
        if cl[-1] >= max(window):
            nh += 1
        if cl[-1] <= min(window):
            nl += 1
        if len(cl) >= _DMA and cl[-1] > fmean(cl[-_DMA:]):
            above += 1
    if universe > 0 and bdate is not None:
        breadth.append(
            BreadthRow(
                date=bdate,
                index="US_LARGECAP_SAMPLE",
                advancers=adv,
                decliners=dec,
                new_highs=nh,
                new_lows=nl,
                pct_above_200dma=(above / universe),
                universe=universe,
            )
        )
        notes.append(f"ブレッドスは大型株サンプル {universe} 銘柄基準の簡易指標です。")

    return NormalizedBatch(
        source="yfinance",
        fetched_at=_now(),
        ok=True,
        prices=prices,
        rates_macro=rates,
        sector_perf=sectors,
        us_breadth=breadth,
        notes=notes,
    )
