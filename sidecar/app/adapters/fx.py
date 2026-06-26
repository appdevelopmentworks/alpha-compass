"""FX adapter: USD/JPY core + EUR/JPY cross (keyless).

Daily rates via yfinance with a Stooq fallback. USD/JPY history feeds the
composite (yen trend) and the Japan view; EUR/JPY is shown as a cross.
"""

from __future__ import annotations

from datetime import datetime, timezone

from app.adapters._daily import download_daily
from app.schemas.common import FetchRequest, FxRow, NormalizedBatch

# pair label -> yahoo symbol
PAIRS: dict[str, str] = {
    "USDJPY": "USDJPY=X",
    "EURJPY": "EURJPY=X",
}


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    bars = download_daily(list(PAIRS.values()), period="2y")
    rows: list[FxRow] = []
    notes: list[str] = []

    for pair, ysym in PAIRS.items():
        series = bars.get(ysym, [])
        if not series:
            notes.append(f"{pair} を取得できませんでした。")
            continue
        for b in series:
            if b.close is not None:
                rows.append(FxRow(pair=pair, ts=b.date, rate=b.close))

    ok = len(rows) > 0
    if not ok:
        notes.append("為替レートを取得できませんでした。")
    return NormalizedBatch(source="fx", fetched_at=_now(), ok=ok, fx_rates=rows, notes=notes)
