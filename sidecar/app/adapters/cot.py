"""CFTC Commitments of Traders adapter (Legacy, futures-only).

Keyless public Socrata endpoint. Fetches a short recent history for a few
markets relevant to the app (equity index, USD, JPY) and normalizes to the
common COT shape. Non-commercial net = non-commercial long - short.
"""

from __future__ import annotations

from datetime import datetime, timezone

import httpx

from app.schemas.common import CotRow, FetchRequest, NormalizedBatch

# Legacy futures-only COT dataset.
_BASE = "https://publicreporting.cftc.gov/resource/6dca-aqww.json"

# (display label, exact CFTC market_and_exchange_names). Exact match avoids
# pulling historical alias contracts that share a substring.
TARGETS: list[tuple[str, str]] = [
    ("E-mini S&P 500", "E-MINI S&P 500 - CHICAGO MERCANTILE EXCHANGE"),
    ("Japanese Yen", "JAPANESE YEN - CHICAGO MERCANTILE EXCHANGE"),
    ("Euro FX", "EURO FX - CHICAGO MERCANTILE EXCHANGE"),
]

_HISTORY = 12  # recent weekly reports per market


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _num(row: dict, *keys: str) -> float | None:
    for k in keys:
        v = row.get(k)
        if v in (None, ""):
            continue
        try:
            return float(v)
        except (TypeError, ValueError):
            continue
    return None


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    rows: list[CotRow] = []
    notes: list[str] = []
    ok = True

    with httpx.Client(timeout=25.0, headers={"User-Agent": "alpha-compass"}) as client:
        for label, market_name in TARGETS:
            params = {
                "$where": f"market_and_exchange_names = '{market_name}'",
                "$order": "report_date_as_yyyy_mm_dd DESC",
                "$limit": str(_HISTORY),
            }
            try:
                resp = client.get(_BASE, params=params)
                if resp.status_code != 200:
                    ok = False
                    notes.append(f"COT {label}: HTTP {resp.status_code}")
                    continue
                data = resp.json()
            except httpx.HTTPError as e:
                ok = False
                notes.append(f"COT {label}: {e}")
                continue

            for r in data:
                date = (r.get("report_date_as_yyyy_mm_dd") or "")[:10]
                if not date:
                    continue
                cl = _num(r, "comm_positions_long_all", "commercial_long")
                cs = _num(r, "comm_positions_short_all", "commercial_short")
                ncl = _num(r, "noncomm_positions_long_all", "noncommercial_long")
                ncs = _num(r, "noncomm_positions_short_all", "noncommercial_short")
                net = (ncl - ncs) if (ncl is not None and ncs is not None) else None
                rows.append(
                    CotRow(
                        date=date, market=label,
                        comm_long=cl, comm_short=cs,
                        noncomm_long=ncl, noncomm_short=ncs, net=net,
                    )
                )

    if not rows:
        ok = False
        notes.append("CFTC COT から建玉データを取得できませんでした。")

    return NormalizedBatch(source="cot", fetched_at=_now(), ok=ok, cot=rows, notes=notes)
