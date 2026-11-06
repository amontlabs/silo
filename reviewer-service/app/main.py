"""POST /v1/review: reads a diff through up to three lenses. Nothing is stored; logs carry
sizes, lenses and timings, never the diff."""

import asyncio
import hmac
import json
import logging
import os
import sys
import time
import uuid

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from .lenses import LENSES, SYSTEM_PROMPT, parse_answer, user_prompt
from .llm import AnthropicClient, LLMClient, LLMError

DEFAULT_MODEL = "claude-sonnet-5-5"
DEFAULT_MAX_BYTES = 256 * 1024

log = logging.getLogger("reviewer")


def event(**fields: object) -> None:
    log.info(json.dumps({"ts": round(time.time(), 3), **fields}))


def configure_logging() -> None:
    logging.basicConfig(stream=sys.stdout, level=logging.INFO, format="%(message)s")


def _error(status: int, message: str) -> JSONResponse:
    return JSONResponse({"error": message}, status_code=status)


async def _body(request: Request, limit: int) -> bytes | None:
    """The request body, or None when it is larger than `limit`."""
    declared = request.headers.get("content-length")
    if declared and declared.isdigit() and int(declared) > limit:
        return None
    chunks, size = [], 0
    async for chunk in request.stream():
        size += len(chunk)
        if size > limit:
            return None
        chunks.append(chunk)
    return b"".join(chunks)


def create_app(client: LLMClient, tokens: list[str], max_bytes: int = DEFAULT_MAX_BYTES) -> FastAPI:
    if not tokens:
        raise ValueError("at least one bearer token is required")
    app = FastAPI(title="Silo reviewer", docs_url=None, redoc_url=None, openapi_url=None)

    def authorized(request: Request) -> bool:
        header = request.headers.get("authorization", "")
        scheme, _, given = header.partition(" ")
        if scheme.lower() != "bearer" or not given:
            return False
        return any(hmac.compare_digest(given.encode(), t.encode()) for t in tokens)

    @app.get("/healthz")
    async def healthz() -> dict:
        return {"ok": True}

    @app.post("/v1/review")
    async def review(request: Request) -> JSONResponse:
        request_id, started = uuid.uuid4().hex[:8], time.monotonic()
        if not authorized(request):
            event(id=request_id, status=401)
            return _error(401, "missing or wrong bearer token")
        raw = await _body(request, max_bytes)
        if raw is None:
            event(id=request_id, status=413, limit=max_bytes)
            return _error(413, f"the request is larger than {max_bytes} bytes")
        try:
            payload = json.loads(raw)
            diff, asked = payload["diff"], payload.get("lenses", list(LENSES))
            if not isinstance(diff, dict) or not isinstance(asked, list):
                raise TypeError
            lenses = [name for name in LENSES if name in asked]
            if not lenses or any(name not in LENSES for name in asked):
                raise ValueError
        except (ValueError, KeyError, TypeError):
            event(id=request_id, status=400, bytes=len(raw))
            return _error(400, "expected {diff: object, lenses: [general|packages|code]}")

        async def one(name: str) -> dict:
            text = await client.complete(SYSTEM_PROMPT, user_prompt(name, diff))
            return parse_answer(text)

        try:
            results = await asyncio.gather(*(one(name) for name in lenses))
        except (LLMError, ValueError) as error:
            # The error names the provider's status or the parse failure, not the diff.
            event(id=request_id, status=502, bytes=len(raw), error=type(error).__name__)
            return _error(502, "the review could not be produced")
        event(
            id=request_id,
            status=200,
            bytes=len(raw),
            lenses=lenses,
            findings=sum(len(r["findings"]) for r in results),
            ms=round((time.monotonic() - started) * 1000),
        )
        return JSONResponse({"lenses": dict(zip(lenses, results))})

    return app


def from_environment() -> FastAPI:
    configure_logging()
    tokens = [t.strip() for t in os.environ.get("REVIEWER_TOKENS", "").split(",") if t.strip()]
    key = os.environ.get("ANTHROPIC_API_KEY", "")
    if not tokens or not key:
        raise SystemExit("Set REVIEWER_TOKENS (comma separated) and ANTHROPIC_API_KEY.")
    client = AnthropicClient(
        api_key=key,
        model=os.environ.get("REVIEWER_MODEL", DEFAULT_MODEL),
        base_url=os.environ.get("ANTHROPIC_BASE_URL", "https://api.anthropic.com"),
    )
    max_bytes = int(os.environ.get("REVIEWER_MAX_BYTES", DEFAULT_MAX_BYTES))
    return create_app(client, tokens, max_bytes)
