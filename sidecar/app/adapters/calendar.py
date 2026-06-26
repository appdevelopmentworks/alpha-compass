"""Economic / policy calendar adapter.

A full econ + earnings calendar needs a paid provider; for v1 this supplies the
high-impact, publicly pre-scheduled central-bank meetings (FOMC, BOJ) keylessly.
Times are the policy-decision moment in JST. Other event types are left to a
later provider integration.
"""

from __future__ import annotations

from datetime import datetime, timezone

from app.schemas.common import CalendarEventRow, FetchRequest, NormalizedBatch

# (type, country, JST decision datetime, title). FOMC announcements land in the
# early JST morning of the following day; BOJ around midday JST.
MEETINGS_2026: list[tuple[str, str, str, str]] = [
    ("fomc", "US", "2026-01-29T04:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-01-23T12:00:00+09:00", "日銀 金融政策決定会合"),
    ("fomc", "US", "2026-03-19T03:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-03-19T12:00:00+09:00", "日銀 金融政策決定会合"),
    ("fomc", "US", "2026-04-30T03:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-04-28T12:00:00+09:00", "日銀 金融政策決定会合"),
    ("fomc", "US", "2026-06-18T03:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-06-16T12:00:00+09:00", "日銀 金融政策決定会合"),
    ("fomc", "US", "2026-07-30T03:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-07-31T12:00:00+09:00", "日銀 金融政策決定会合"),
    ("fomc", "US", "2026-09-17T03:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-09-18T12:00:00+09:00", "日銀 金融政策決定会合"),
    ("fomc", "US", "2026-10-29T03:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-10-30T12:00:00+09:00", "日銀 金融政策決定会合"),
    ("fomc", "US", "2026-12-10T04:00:00+09:00", "FOMC 政策金利発表"),
    ("boj", "JP", "2026-12-18T12:00:00+09:00", "日銀 金融政策決定会合"),
]


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    events = [
        CalendarEventRow(
            id=f"{etype}-{dt[:10]}",
            type=etype,
            datetime_jst=dt,
            country=country,
            importance="high",
            title=title,
        )
        for (etype, country, dt, title) in MEETINGS_2026
    ]
    return NormalizedBatch(
        source="calendar",
        fetched_at=_now(),
        ok=True,
        calendar_events=events,
        notes=["経済指標・決算カレンダーはプロバイダ連携で拡充予定（現状は FOMC/日銀 会合）。"],
    )
