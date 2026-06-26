"""Tier 2 — local LLM (Qwen MoE on RTX 5090) via an OpenAI-compatible endpoint.

Used for free-text Japanese summarization when a local server is running
(``LOCAL_LLM_ENDPOINT``). Cost is GPU-only. If the endpoint is unreachable the
router falls back to Tier 1.
"""

from __future__ import annotations

import os
import time

import httpx

_ENDPOINT = os.environ.get("LOCAL_LLM_ENDPOINT", "http://127.0.0.1:8080")
_MODEL = os.environ.get("LOCAL_LLM_MODEL", "qwen-moe")

_SYSTEM = "あなたは金融ニュースを簡潔な日本語で要約するアシスタントです。1〜2文で要点のみ。"

# Cache the reachability probe so a batch of summaries doesn't pay the timeout
# (and connection cost) once per item.
_avail_cache: dict[str, float | bool] = {"ts": 0.0, "val": False}
_AVAIL_TTL = 30.0


def available() -> bool:
    """Quick (cached) reachability check for the local LLM server."""
    now = time.time()
    if now - float(_avail_cache["ts"]) < _AVAIL_TTL:
        return bool(_avail_cache["val"])
    try:
        r = httpx.get(f"{_ENDPOINT}/v1/models", timeout=1.5)
        val = r.status_code == 200
    except httpx.HTTPError:
        val = False
    _avail_cache["ts"] = now
    _avail_cache["val"] = val
    return val


def summarize(title: str, body: str, max_tokens: int = 160) -> str | None:
    """Return a Japanese summary, or None on failure."""
    prompt = f"見出し: {title}\n本文: {body}\n\n上記を日本語で1〜2文に要約してください。"
    try:
        r = httpx.post(
            f"{_ENDPOINT}/v1/chat/completions",
            json={
                "model": _MODEL,
                "messages": [
                    {"role": "system", "content": _SYSTEM},
                    {"role": "user", "content": prompt},
                ],
                "max_tokens": max_tokens,
                "temperature": 0.2,
            },
            timeout=30.0,
        )
        if r.status_code != 200:
            return None
        return r.json()["choices"][0]["message"]["content"].strip()
    except (httpx.HTTPError, KeyError, IndexError):
        return None
