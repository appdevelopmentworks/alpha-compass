"""Tier 3 — Claude API for the highest-value summaries and the daily brief.

Gated by ``ANTHROPIC_API_KEY`` and a per-day USD budget
(``AI_TIER3_DAILY_BUDGET_USD``) so it never runs away. Summaries use a cheap
model; the cross-signal daily brief uses a stronger one.
"""

from __future__ import annotations

import os
from datetime import date

import httpx

_API = "https://api.anthropic.com/v1/messages"
_SUMMARY_MODEL = "claude-haiku-4-5-20251001"
_BRIEF_MODEL = "claude-sonnet-4-6"

# Approximate USD per 1M tokens (input, output) for budgeting.
_PRICES = {
    _SUMMARY_MODEL: (1.0, 5.0),
    _BRIEF_MODEL: (3.0, 15.0),
}

# Daily spend tracker: {iso_date: usd_spent}.
_spend: dict[str, float] = {}


def _budget() -> float:
    try:
        return float(os.environ.get("AI_TIER3_DAILY_BUDGET_USD", "2.0"))
    except ValueError:
        return 2.0


def _spent_today() -> float:
    return _spend.get(date.today().isoformat(), 0.0)


def _record(model: str, usage: dict) -> None:
    pin, pout = _PRICES.get(model, (3.0, 15.0))
    cost = (usage.get("input_tokens", 0) * pin + usage.get("output_tokens", 0) * pout) / 1e6
    today = date.today().isoformat()
    _spend[today] = _spend.get(today, 0.0) + cost


def available() -> bool:
    return bool(os.environ.get("ANTHROPIC_API_KEY")) and _spent_today() < _budget()


def _call(model: str, system: str, user: str, max_tokens: int) -> str | None:
    key = os.environ.get("ANTHROPIC_API_KEY")
    if not key or _spent_today() >= _budget():
        return None
    try:
        r = httpx.post(
            _API,
            headers={
                "x-api-key": key,
                "anthropic-version": "2023-06-01",
                "content-type": "application/json",
            },
            json={
                "model": model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            },
            timeout=40.0,
        )
        if r.status_code != 200:
            return None
        data = r.json()
        _record(model, data.get("usage", {}))
        return "".join(
            b.get("text", "") for b in data.get("content", []) if b.get("type") == "text"
        ).strip()
    except (httpx.HTTPError, KeyError):
        return None


def summarize(title: str, body: str) -> str | None:
    return _call(
        _SUMMARY_MODEL,
        "あなたは金融ニュースを簡潔な日本語で要約します。1〜2文、要点のみ。",
        f"見出し: {title}\n本文: {body}\n\n日本語で1〜2文に要約してください。",
        max_tokens=200,
    )


def brief(context: str) -> str | None:
    return _call(
        _BRIEF_MODEL,
        "あなたは投資情報モニターのアナリストです。複数シグナルを横断し、本日の注目点を"
        "引用元に忠実に、日本語で簡潔に（3〜5行）まとめます。売買推奨はしません。",
        context,
        max_tokens=600,
    )
