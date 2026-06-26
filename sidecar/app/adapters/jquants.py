"""J-Quants adapter (API **v2**).

J-Quants migrated v1 -> v2: the old token/auth_refresh flow is gone; v2 uses a
single **API key** from the dashboard, passed via the ``x-api-key`` header.
Responses come as an array under ``data`` with abbreviated field names.

Provides Japan-specific data not available from keyless sources: investor-type
trading flows (投資部門別売買動向 — the project's key differentiator), the
official TOPIX index, and short-selling ratios. Without an API key the adapter
degrades gracefully.
"""

from __future__ import annotations

import os
from datetime import datetime, timezone

import httpx

from app.schemas.common import (
    FetchRequest,
    JpInvestorFlowRow,
    JpShortSellingRow,
    NormalizedBatch,
    PriceRow,
)

_BASE = "https://api.jquants.com/v2"

# our investor_type key -> (buy field, sell field, balance/net field) in v2.
INVESTOR_FIELDS: dict[str, tuple[str, str, str]] = {
    "foreigners": ("FrgnBuy", "FrgnSell", "FrgnBal"),
    "individuals": ("IndBuy", "IndSell", "IndBal"),
    "trust_banks": ("TrstBnkBuy", "TrstBnkSell", "TrstBnkBal"),
    "investment_trusts": ("InvTrBuy", "InvTrSell", "InvTrBal"),
    "business_corps": ("BusCoBuy", "BusCoSell", "BusCoBal"),
}


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _f(v) -> float | None:
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def _pick(row: dict, *keys: str):
    """First present value among candidate keys (v2 abbreviates names)."""
    for k in keys:
        if k in row and row[k] not in (None, ""):
            return row[k]
    return None


def _api_key(req: FetchRequest | None) -> str:
    return (
        (req.api_key if req else None)
        or os.environ.get("JQUANTS_API_KEY", "")
        or os.environ.get("JQUANTS_REFRESH_TOKEN", "")  # back-compat key slot
    ).strip()


def _paged(client: httpx.Client, path: str, key: str, max_pages: int = 5) -> list[dict]:
    """Collect records from a v2 list endpoint (array under ``data``)."""
    out: list[dict] = []
    pagination = None
    for _ in range(max_pages):
        params = {"pagination_key": pagination} if pagination else {}
        r = client.get(f"{_BASE}{path}", params=params)
        if r.status_code != 200:
            raise httpx.HTTPStatusError(
                f"HTTP {r.status_code}: {(r.text or '')[:160]}",
                request=r.request,
                response=r,
            )
        body = r.json()
        out.extend(body.get("data", []))
        pagination = body.get("pagination_key")
        if not pagination:
            break
    return out


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    api_key = _api_key(req)
    if not api_key:
        return NormalizedBatch(
            source="jquants",
            fetched_at=_now(),
            ok=False,
            notes=["J-Quants API キー(v2)が未設定です。ダッシュボードで発行し設定で登録してください。"],
        )

    notes: list[str] = []
    errors: list[str] = []
    flows: list[JpInvestorFlowRow] = []
    shorts: list[JpShortSellingRow] = []
    prices: list[PriceRow] = []

    headers = {"x-api-key": api_key}
    with httpx.Client(timeout=30.0, headers=headers) as client:
        # 1) Investor-type trading flows (投資部門別売買状況).
        try:
            for rec in _paged(client, "/equities/investor-types", "data"):
                week = str(_pick(rec, "EnDate", "PubDate") or "")[:10]
                section = str(_pick(rec, "Section") or "all")
                if not week:
                    continue
                for itype, (bk, sk, nk) in INVESTOR_FIELDS.items():
                    bal = _f(rec.get(nk))
                    if bal is None:
                        continue
                    flows.append(
                        JpInvestorFlowRow(
                            week_ending=week,
                            investor_type=itype,
                            market=section,
                            buy=_f(rec.get(bk)),
                            sell=_f(rec.get(sk)),
                            net=bal,
                        )
                    )
        except httpx.HTTPError as e:
            errors.append(str(e))

        # 2) Official TOPIX index daily bars (defensive field names).
        try:
            for rec in _paged(client, "/indices/bars/daily/topix", "data"):
                d = str(_pick(rec, "Date", "D") or "")[:10]
                if not d:
                    continue
                prices.append(
                    PriceRow(
                        symbol="TOPIX", market="JP_INDEX", ts=d,
                        open=_f(_pick(rec, "O", "Open")),
                        high=_f(_pick(rec, "H", "High")),
                        low=_f(_pick(rec, "L", "Low")),
                        close=_f(_pick(rec, "C", "Close")),
                        volume=None,
                    )
                )
        except httpx.HTTPError as e:
            errors.append(str(e))

        # 3) Short-selling ratio (defensive field names).
        try:
            for rec in _paged(client, "/markets/short-ratio", "data"):
                d = str(_pick(rec, "Date", "D") or "")[:10]
                if not d:
                    continue
                ratio = _f(_pick(rec, "ShortRatio", "Ratio", "ShortSellingRatio"))
                shorts.append(
                    JpShortSellingRow(
                        date=d,
                        market=str(_pick(rec, "Sector33Code", "Sector", "Code") or "all"),
                        short_ratio=ratio,
                    )
                )
        except httpx.HTTPError as e:
            errors.append(str(e))

    ok = bool(flows or shorts or prices)
    subscription_gated = any(
        "subscription" in e or "HTTP 403" in e for e in errors
    )
    if ok:
        notes.append("J-Quants 無料プランは配信遅延（約12週）があります。")
    elif subscription_gated:
        notes.append(
            "J-Quants 無料プランでは 投資部門別・TOPIX・空売り は未提供です"
            "（Light プラン以上が必要）。株価四本値などは無料プランで利用可。"
        )
    elif errors:
        notes.append("J-Quants(v2) 取得失敗: " + " / ".join(errors)[:300])
    else:
        notes.append("J-Quants(v2) から対象データを取得できませんでした。")
    return NormalizedBatch(
        source="jquants",
        fetched_at=_now(),
        ok=ok,
        prices=prices,
        jp_investor_flows=flows,
        jp_short_selling=shorts,
        notes=notes,
    )
