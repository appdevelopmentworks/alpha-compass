"""Shared daily OHLCV fetcher.

Primary source is the ``yfinance`` library (as specified in the architecture);
because Yahoo endpoints are frequently rate-limited, a keyless Stooq CSV
fallback fills any gaps so the US core still renders. Both return the same
simple bar shape so callers don't care which path produced the data.
"""

from __future__ import annotations

import csv
import io
from dataclasses import dataclass

import httpx

# Yahoo ticker -> Stooq symbol overrides. Plain US stocks/ETFs fall through to
# the ``<lower>.us`` convention.
_STOOQ_OVERRIDES: dict[str, str] = {
    "^GSPC": "^spx",
    "^IXIC": "^ndq",
    "^DJI": "^dji",
    "^RUT": "^rut",
    "^VIX": "^vix",
    "DX-Y.NYB": "^dxy",
}


@dataclass
class Bar:
    date: str  # YYYY-MM-DD
    open: float | None
    high: float | None
    low: float | None
    close: float | None
    volume: float | None


def _stooq_symbol(yahoo_symbol: str) -> str:
    if yahoo_symbol in _STOOQ_OVERRIDES:
        return _STOOQ_OVERRIDES[yahoo_symbol]
    if yahoo_symbol.startswith("^"):
        return yahoo_symbol.lower()
    return f"{yahoo_symbol.lower()}.us"


def _f(v: str) -> float | None:
    try:
        x = float(v)
        return x
    except (TypeError, ValueError):
        return None


def _fetch_stooq(symbol: str, client: httpx.Client) -> list[Bar]:
    """Fetch the full available daily history for one symbol from Stooq."""
    s = _stooq_symbol(symbol)
    url = f"https://stooq.com/q/d/l/?s={s}&i=d"
    try:
        resp = client.get(url, timeout=20.0)
        if resp.status_code != 200 or not resp.text:
            return []
        text = resp.text.strip()
        # Stooq returns "No data" or an HTML error for unknown symbols.
        if not text or text.lower().startswith("<") or "date" not in text.splitlines()[0].lower():
            return []
        reader = csv.DictReader(io.StringIO(text))
        bars: list[Bar] = []
        for row in reader:
            d = row.get("Date")
            if not d:
                continue
            bars.append(
                Bar(
                    date=d,
                    open=_f(row.get("Open", "")),
                    high=_f(row.get("High", "")),
                    low=_f(row.get("Low", "")),
                    close=_f(row.get("Close", "")),
                    volume=_f(row.get("Volume", "")),
                )
            )
        return bars
    except httpx.HTTPError:
        return []


def _fetch_yfinance(symbols: list[str], period: str) -> dict[str, list[Bar]]:
    """Batch download via yfinance. Returns {} on any failure."""
    try:
        import yfinance as yf  # absolute import resolves the installed package
    except ImportError:
        return {}

    try:
        data = yf.download(
            tickers=symbols,
            period=period,
            interval="1d",
            auto_adjust=False,
            group_by="ticker",
            threads=True,
            progress=False,
        )
    except Exception:
        return {}

    if data is None or getattr(data, "empty", True):
        return {}

    out: dict[str, list[Bar]] = {}
    multi = len(symbols) > 1

    for sym in symbols:
        try:
            frame = data[sym] if multi else data
        except (KeyError, TypeError):
            continue
        if frame is None or frame.empty:
            continue
        bars: list[Bar] = []
        for idx, r in frame.iterrows():
            close = r.get("Close")
            if close is None or (close != close):  # skip NaN-only rows
                continue
            date = idx.date().isoformat() if hasattr(idx, "date") else str(idx)[:10]

            def num(key: str):
                v = r.get(key)
                if v is None or v != v:
                    return None
                return float(v)

            bars.append(
                Bar(
                    date=date,
                    open=num("Open"),
                    high=num("High"),
                    low=num("Low"),
                    close=num("Close"),
                    volume=num("Volume"),
                )
            )
        if bars:
            out[sym] = bars
    return out


def download_daily(symbols: list[str], period: str = "2y") -> dict[str, list[Bar]]:
    """Return daily bars per symbol, using yfinance then Stooq for any misses."""
    symbols = list(dict.fromkeys(symbols))  # de-dupe, preserve order
    result = _fetch_yfinance(symbols, period)

    missing = [s for s in symbols if s not in result or not result[s]]
    if missing:
        with httpx.Client(headers={"User-Agent": "Mozilla/5.0 alpha-compass"}) as client:
            for sym in missing:
                bars = _fetch_stooq(sym, client)
                if bars:
                    result[sym] = bars
    return result
