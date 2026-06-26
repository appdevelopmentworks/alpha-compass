"""Japanese financial news adapter (keyless Google News RSS).

Fetches headlines for a few market topics, de-duplicates, and attaches a
Japanese summary via the AI cost-cascade (Tier 1 rules by default; higher tiers
when a local LLM / Claude key is available). The generating tier is recorded.
"""

from __future__ import annotations

import hashlib
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from xml.etree import ElementTree as ET

import httpx

from app.ai import router
from app.schemas.common import FetchRequest, NewsRow, NormalizedBatch

# (query, importance 1..3)
TOPICS: list[tuple[str, int]] = [
    ("日経平均 株価", 1),
    ("東証 マーケット", 1),
    ("ドル円 為替", 1),
    ("FOMC 金利", 2),
    ("日銀 金融政策", 2),
]

_RSS = "https://news.google.com/rss/search"
_PER_TOPIC = 6


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _to_iso(pubdate: str) -> str:
    try:
        return parsedate_to_datetime(pubdate).astimezone(timezone.utc).isoformat()
    except (TypeError, ValueError):
        return _now()


def _id(url: str) -> str:
    return hashlib.sha1(url.encode("utf-8")).hexdigest()[:16]


def fetch(req: FetchRequest | None = None) -> NormalizedBatch:
    rows: dict[str, NewsRow] = {}
    notes: list[str] = []

    with httpx.Client(
        headers={"User-Agent": "Mozilla/5.0 alpha-compass"}, timeout=20.0
    ) as client:
        for query, importance in TOPICS:
            try:
                r = client.get(
                    _RSS, params={"q": query, "hl": "ja", "gl": "JP", "ceid": "JP:ja"}
                )
                if r.status_code != 200:
                    notes.append(f"ニュース取得失敗（{query}）: HTTP {r.status_code}")
                    continue
                root = ET.fromstring(r.text)
            except (httpx.HTTPError, ET.ParseError) as e:
                notes.append(f"ニュース取得失敗（{query}）: {e}")
                continue

            for item in list(root.iterfind(".//item"))[:_PER_TOPIC]:
                link = (item.findtext("link") or "").strip()
                title = (item.findtext("title") or "").strip()
                if not link or not title or link in rows:
                    continue
                desc = item.findtext("description") or ""
                summary, tier = router.summarize(title, desc, importance=importance)
                rows[link] = NewsRow(
                    id=_id(link),
                    source="GoogleNews",
                    datetime=_to_iso(item.findtext("pubDate") or ""),
                    title=title,
                    url=link,
                    lang="ja",
                    summary=summary,
                    summarized_tier=tier,
                    tickers=[],
                )

    items = list(rows.values())
    ok = len(items) > 0
    if not ok:
        notes.append("ニュースを取得できませんでした。")
    return NormalizedBatch(
        source="news", fetched_at=_now(), ok=ok, news=items, notes=notes
    )
