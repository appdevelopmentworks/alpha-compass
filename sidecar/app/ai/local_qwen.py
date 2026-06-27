"""Tier 2 — local LLM (Qwen on RTX 5090) via an OpenAI-compatible endpoint.

Auto-discovers a running local server among common providers (Ollama, LM Studio)
or an explicitly configured endpoint, and auto-selects the loaded model. Used for
free-text Japanese summarization; if nothing is running the router falls back to
Tier 1 rules.

Discovery priority (first reachable wins):
  1. ``LOCAL_LLM_ENDPOINT`` (explicit override)
  2. Ollama      — http://127.0.0.1:11434
  3. LM Studio   — http://127.0.0.1:1234
  4. Generic     — http://127.0.0.1:8080
"""

from __future__ import annotations

import os
import time

import httpx

_SYSTEM = "あなたは金融ニュースを簡潔な日本語で要約するアシスタントです。1〜2文で要点のみ。"

_PREFERRED_MODEL = os.environ.get("LOCAL_LLM_MODEL", "")


def _candidates() -> list[str]:
    eps = [
        os.environ.get("LOCAL_LLM_ENDPOINT", "").rstrip("/"),
        "http://127.0.0.1:11434",  # Ollama
        "http://127.0.0.1:1234",   # LM Studio
        "http://127.0.0.1:8080",   # generic (llama.cpp / vLLM)
    ]
    seen: set[str] = set()
    out: list[str] = []
    for e in eps:
        if e and e not in seen:
            seen.add(e)
            out.append(e)
    return out


def _pick_model(models: list[str]) -> str | None:
    if not models:
        # Some servers (older Ollama) don't list models; trust the preferred name.
        return _PREFERRED_MODEL or None
    if _PREFERRED_MODEL and _PREFERRED_MODEL in models:
        return _PREFERRED_MODEL
    # Prefer a Qwen model if present, else the first available.
    for m in models:
        if "qwen" in m.lower():
            return m
    return models[0]


# Cached discovery result so a batch of summaries doesn't re-probe per item.
_active: dict[str, object] = {"ts": 0.0, "endpoint": None, "model": None}
_TTL = 30.0


def _discover() -> tuple[str | None, str | None]:
    for ep in _candidates():
        try:
            r = httpx.get(f"{ep}/v1/models", timeout=1.5)
            if r.status_code != 200:
                continue
            models = [m.get("id", "") for m in r.json().get("data", [])]
            model = _pick_model([m for m in models if m])
            if model:
                return ep, model
        except httpx.HTTPError:
            continue
    return None, None


def _resolve() -> tuple[str | None, str | None]:
    now = time.time()
    if now - float(_active["ts"]) < _TTL:  # type: ignore[arg-type]
        return _active["endpoint"], _active["model"]  # type: ignore[return-value]
    ep, model = _discover()
    _active.update(ts=now, endpoint=ep, model=model)
    return ep, model


def active_info() -> tuple[str | None, str | None]:
    """(endpoint, model) currently in use, or (None, None)."""
    return _resolve()


def available() -> bool:
    ep, model = _resolve()
    return ep is not None and model is not None


# Headroom for "reasoning" models (gemma, qwen3 thinking, etc.) which spend
# tokens thinking before emitting `content`; too low a cap leaves content empty.
def summarize(title: str, body: str, max_tokens: int = 768) -> str | None:
    ep, model = _resolve()
    if not ep or not model:
        return None
    prompt = f"見出し: {title}\n本文: {body}\n\n上記を日本語で1〜2文に要約してください。"
    try:
        r = httpx.post(
            f"{ep}/v1/chat/completions",
            json={
                "model": model,
                "messages": [
                    {"role": "system", "content": _SYSTEM},
                    {"role": "user", "content": prompt},
                ],
                "max_tokens": max_tokens,
                "temperature": 0.2,
            },
            timeout=60.0,
        )
        if r.status_code != 200:
            return None
        msg = r.json()["choices"][0]["message"]
        # Prefer the final answer; ignore separate "reasoning"/"thinking" fields.
        content = (msg.get("content") or "").strip()
        return content or None
    except (httpx.HTTPError, KeyError, IndexError):
        return None
