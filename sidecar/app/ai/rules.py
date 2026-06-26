"""Tier 1 — rule-based, zero-cost summarization and extraction.

Handles the cheap, deterministic cases: cleaning headline/disclosure text into a
short Japanese summary, and templated extraction from structured (XBRL-like)
financial fields. No LLM is invoked here.
"""

from __future__ import annotations

import re

_TAG_RE = re.compile(r"<[^>]+>")
_WS_RE = re.compile(r"\s+")


def clean_text(text: str) -> str:
    """Strip HTML tags and collapse whitespace."""
    if not text:
        return ""
    return _WS_RE.sub(" ", _TAG_RE.sub(" ", text)).strip()


def summarize(title: str, body: str = "", max_len: int = 120) -> str:
    """Extractive one-line Japanese summary (headline + leading body sentence)."""
    title = clean_text(title)
    body = clean_text(body)
    if not body:
        return title[:max_len]
    # First sentence-ish chunk of the body.
    head = re.split(r"[。.!?\n]", body, maxsplit=1)[0]
    summary = head if len(head) >= 12 else body
    if title and title not in summary:
        summary = f"{title} — {summary}"
    return summary[:max_len]


def extract_financials(fields: dict) -> str | None:
    """Templated extraction from structured earnings fields (revenue/profit)."""
    rev = fields.get("net_sales") or fields.get("revenue")
    op = fields.get("operating_income")
    yoy = fields.get("operating_income_yoy")
    if rev is None and op is None:
        return None
    parts = []
    if rev is not None:
        parts.append(f"売上 {rev}")
    if op is not None:
        s = f"営業益 {op}"
        if yoy is not None:
            s += f"（前年比 {yoy}）"
        parts.append(s)
    return "・".join(parts) if parts else None
