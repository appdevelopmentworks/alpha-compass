"""TDnet (timely disclosure) adapter — access method pending.

TDnet has no official free API and its web listing is session-constrained;
the architecture flags the access method as "to be settled". This adapter is a
graceful placeholder that surfaces the status until that is resolved (a future
option is a licensed feed or an approved mirror).
"""

from __future__ import annotations

from datetime import datetime, timezone

from app.schemas.common import FetchRequest, NormalizedBatch


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    return NormalizedBatch(
        source="tdnet",
        fetched_at=datetime.now(timezone.utc).isoformat(),
        ok=False,
        notes=["TDnet は取得方式の確定が必要（公式 API なし）。現状は未取得。"],
    )
