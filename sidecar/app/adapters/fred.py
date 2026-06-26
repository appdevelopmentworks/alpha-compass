"""FRED adapter: rates / credit / dollar series.

Requires a (free) FRED API key, supplied by Rust via the request body or the
``FRED_API_KEY`` environment variable. With no key it degrades gracefully:
returns ``ok=False`` plus a note so the UI can show the panel as "未取得"
rather than failing.
"""

from __future__ import annotations

import os
from datetime import datetime, timedelta, timezone

import httpx

from app.schemas.common import FetchRequest, NormalizedBatch, RateMacroRow

# Series the US core needs from FRED (architecture §5/§8).
DEFAULT_SERIES: list[str] = [
    "DGS2",            # 2y Treasury
    "DGS10",           # 10y Treasury
    "T10Y2Y",          # 10y-2y spread
    "BAMLH0A0HYM2",    # ICE BofA US High Yield OAS
    "DTWEXBGS",        # Broad USD index
]

_BASE = "https://api.stlouisfed.org/fred/series/observations"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    api_key = (req.api_key if req else None) or os.environ.get("FRED_API_KEY", "")
    if not api_key:
        return NormalizedBatch(
            source="fred",
            fetched_at=_now(),
            ok=False,
            notes=["FRED API キーが未設定です。金利/クレジット(2年・HY OAS 等)は未取得。"],
        )

    series = (req.series if req and req.series else None) or DEFAULT_SERIES
    lookback = (req.lookback_days if req and req.lookback_days else None) or 760
    start = (datetime.now(timezone.utc) - timedelta(days=lookback)).date().isoformat()

    rates: list[RateMacroRow] = []
    notes: list[str] = []
    ok = True

    with httpx.Client(timeout=20.0) as client:
        for sid in series:
            params = {
                "series_id": sid,
                "api_key": api_key,
                "file_type": "json",
                "observation_start": start,
            }
            try:
                resp = client.get(_BASE, params=params)
                if resp.status_code != 200:
                    ok = False
                    notes.append(f"FRED {sid}: HTTP {resp.status_code}")
                    continue
                obs = resp.json().get("observations", [])
                for o in obs:
                    raw = o.get("value", ".")
                    if raw in (".", "", None):
                        continue
                    try:
                        val = float(raw)
                    except ValueError:
                        continue
                    rates.append(RateMacroRow(series_id=sid, date=o["date"], value=val))
            except httpx.HTTPError as e:
                ok = False
                notes.append(f"FRED {sid}: {e}")

    if not rates and ok:
        ok = False
        notes.append("FRED から観測値が得られませんでした。")

    return NormalizedBatch(
        source="fred", fetched_at=_now(), ok=ok, rates_macro=rates, notes=notes
    )
