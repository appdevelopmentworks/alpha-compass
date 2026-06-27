"""FastAPI entrypoint for the alpha-compass sidecar.

Responsibilities in Session 0 are intentionally minimal:
- expose ``GET /health``
- enforce the per-session bearer token supplied by Rust via the
  ``ALPHA_COMPASS_SIDECAR_TOKEN`` environment variable (fail closed)

Later sessions add data-source adapters, normalization, and the AI cascade.
"""

from __future__ import annotations

import hmac
import os
from datetime import datetime, timezone

import uvicorn
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse

from app.adapters import calendar as calendar_adapter
from app.adapters import cot as cot_adapter
from app.adapters import edinet as edinet_adapter
from app.adapters import fred as fred_adapter
from app.adapters import fx as fx_adapter
from app.adapters import jquants as jquants_adapter
from app.adapters import news as news_adapter
from app.adapters import tdnet as tdnet_adapter
from app.adapters import yfinance as yfinance_adapter
from app.ai import claude as ai_claude
from app.ai import local_qwen as ai_local
from app.ai import router as ai_router
from app.schemas.common import FetchRequest, NormalizedBatch

SERVICE_NAME = "alpha-compass-sidecar"
TOKEN_ENV = "ALPHA_COMPASS_SIDECAR_TOKEN"
HOST_ENV = "ALPHA_COMPASS_SIDECAR_HOST"
PORT_ENV = "ALPHA_COMPASS_SIDECAR_PORT"

app = FastAPI(
    title=SERVICE_NAME,
    version="0.1.0",
    # No interactive docs surface for a localhost-only internal service.
    docs_url=None,
    redoc_url=None,
    openapi_url=None,
)


def _extract_bearer(authorization: str) -> str:
    """Return the bearer token from an Authorization header, or ``""``."""
    prefix = "bearer "
    if authorization.lower().startswith(prefix):
        return authorization[len(prefix) :].strip()
    return ""


@app.middleware("http")
async def require_session_token(request: Request, call_next):
    """Reject any request whose bearer token does not match the session token.

    Fails closed: if no token is configured in the environment, every request
    is rejected. Uses a constant-time comparison to avoid timing leaks.
    """
    expected = os.environ.get(TOKEN_ENV, "")
    presented = _extract_bearer(request.headers.get("authorization", ""))
    if not expected or not hmac.compare_digest(presented, expected):
        return JSONResponse(status_code=401, content={"detail": "unauthorized"})
    return await call_next(request)


@app.get("/health")
async def health() -> dict[str, str]:
    """Liveness probe. Returns service identity and a UTC timestamp."""
    return {
        "status": "ok",
        "service": SERVICE_NAME,
        "time": datetime.now(timezone.utc).isoformat(),
    }


# Registry of data-source adapters. Each returns a NormalizedBatch.
_ADAPTERS = {
    "yfinance": yfinance_adapter.fetch,
    "fred": fred_adapter.fetch,
    "cot": cot_adapter.fetch,
    "fx": fx_adapter.fetch,
    "jquants": jquants_adapter.fetch,
    "news": news_adapter.fetch,
    "calendar": calendar_adapter.fetch,
    "edinet": edinet_adapter.fetch,
    "tdnet": tdnet_adapter.fetch,
}


@app.post("/fetch/{source}", response_model=NormalizedBatch)
def fetch_source(source: str, req: FetchRequest | None = None) -> NormalizedBatch:
    """Fetch + normalize one data source. Rust upserts the result into DuckDB.

    Runs synchronously on a worker thread (FastAPI runs sync handlers in a
    threadpool) because the adapters do blocking network I/O.
    """
    adapter = _ADAPTERS.get(source)
    if adapter is None:
        raise HTTPException(status_code=404, detail=f"unknown source: {source}")
    return adapter(req or FetchRequest())


@app.post("/ai/brief")
def ai_brief(payload: dict) -> dict:
    """Generate the daily brief from a context payload (cost-cascade)."""
    payload = payload or {}
    max_tier = payload.get("max_tier") or ai_router.DEFAULT_MAX_TIER
    text, tier = ai_router.brief(payload, max_tier)
    return {"text": text, "tier": tier}


@app.get("/ai/status")
def ai_status() -> dict:
    """Report the detected local LLM (endpoint/model) and Claude availability."""
    endpoint, model = ai_local.active_info()
    return {
        "local_available": endpoint is not None and model is not None,
        "local_endpoint": endpoint,
        "local_model": model,
        "claude_configured": bool(os.environ.get("ANTHROPIC_API_KEY")),
        "claude_available": ai_claude.available(),
    }


def main() -> None:
    """Run standalone (debug). Rust normally launches uvicorn directly."""
    host = os.environ.get(HOST_ENV, "127.0.0.1")
    port = int(os.environ.get(PORT_ENV, "8765"))
    uvicorn.run(app, host=host, port=port, log_level="info")


if __name__ == "__main__":
    main()
