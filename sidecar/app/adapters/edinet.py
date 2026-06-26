"""EDINET adapter (securities reports, large-holding reports).

EDINET API v2 requires a (free) subscription key. Without it the adapter
degrades gracefully. With a key it lists a day's filings and normalizes the
relevant document types, attaching a Tier-1 summary.
"""

from __future__ import annotations

import os
from datetime import datetime, timezone

import httpx

from app.ai import router
from app.schemas.common import DisclosureRow, FetchRequest, NormalizedBatch

_LIST = "https://api.edinet-fsa.go.jp/api/v2/documents.json"

# Document type codes of interest (有報・四半期・大量保有 等).
INTEREST_DOC_TYPES = {"120", "130", "140", "150", "350", "360"}


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    key = (req.api_key if req else None) or os.environ.get("EDINET_API_KEY", "")
    if not key:
        return NormalizedBatch(
            source="edinet",
            fetched_at=_now(),
            ok=False,
            notes=["EDINET API キー(無料)が未設定のため開示は未取得。"],
        )

    day = datetime.now(timezone.utc).date().isoformat()
    rows: list[DisclosureRow] = []
    notes: list[str] = []
    try:
        with httpx.Client(timeout=30.0) as client:
            r = client.get(
                _LIST,
                params={"date": day, "type": "2"},
                headers={"Ocp-Apim-Subscription-Key": key},
            )
            if r.status_code != 200:
                return NormalizedBatch(
                    source="edinet", fetched_at=_now(), ok=False,
                    notes=[f"EDINET HTTP {r.status_code}"],
                )
            for d in r.json().get("results", []):
                code = d.get("docTypeCode")
                if code not in INTEREST_DOC_TYPES:
                    continue
                title = d.get("docDescription") or ""
                summary, tier = router.summarize(title, "", importance=2)
                doc_id = d.get("docID", "")
                rows.append(
                    DisclosureRow(
                        id=f"edinet-{doc_id}",
                        source="EDINET",
                        company_code=d.get("secCode") or d.get("edinetCode"),
                        datetime=d.get("submitDateTime") or _now(),
                        doc_type=code,
                        title=title,
                        url=f"https://disclosure2.edinet-fsa.go.jp/WEEK0010.aspx?bid={doc_id}",
                        summary=summary,
                        summarized_tier=tier,
                    )
                )
    except httpx.HTTPError as e:
        return NormalizedBatch(
            source="edinet", fetched_at=_now(), ok=False, notes=[f"EDINET 取得失敗: {e}"]
        )

    ok = len(rows) > 0
    if not ok:
        notes.append("本日の対象開示はありませんでした。")
    return NormalizedBatch(
        source="edinet", fetched_at=_now(), ok=ok, disclosures=rows, notes=notes
    )
