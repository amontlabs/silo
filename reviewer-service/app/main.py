"""POST /v1/review: reads a diff through up to three lenses. Nothing is stored; logs carry
sizes, lenses, timings and a short token id, never the diff."""

import asyncio
import hashlib
import hmac
import json
import logging
import os
import sys
import time
import uuid
from collections import defaultdict, deque
from contextlib import asynccontextmanager

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from .lenses import LENSES, RESULT_SCHEMA, SYSTEM_PROMPT, parse_answer, user_prompt
from .llm import AnthropicClient, LLMClient, LLMError

DEFAULT_MODEL = "claude-sonnet-5-5"
DEFAULT_MAX_BYTES = 256 * 1024
DEFAULT_RATE_PER_MINUTE = 30
DEFAULT_CONCURRENCY = 4
MAX_DEPTH = 24

log = logging.getLogger("reviewer")


def event(**fields: object) -> None:
    log.info(json.dumps({"ts": round(time.time(), 3), **fields}))


def configure_logging() -> None:
    logging.basicConfig(stream=sys.stdout, level=logging.INFO, format="%(message)s")
    # httpx logs every request URL at INFO; keep it to problems.
    logging.getLogger("httpx").setLevel(logging.WARNING)
    logging.getLogger("httpcore").setLevel(logging.WARNING)


def _error(status: int, message: str, **headers: str) -> JSONResponse:
    return JSONResponse({"error": message}, status_code=status, headers=headers)


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


def _depth_ok(value: object, depth: int = 0) -> bool:
    """Whether `value` nests no deeper than MAX_DEPTH."""
    if depth > MAX_DEPTH:
        return False
    if isinstance(value, dict):
        return all(_depth_ok(v, depth + 1) for v in value.values())
    if isinstance(value, list):
        return all(_depth_ok(v, depth + 1) for v in value)
    return True


def _digest(text: str) -> bytes:
    return hashlib.sha256(text.encode()).digest()


class Limits:
    """Per-token request rate and concurrency, in memory."""

    def __init__(self, per_minute: int, concurrency: int) -> None:
        self.per_minute, self.concurrency = per_minute, concurrency
        self.times: dict[str, deque] = defaultdict(deque)
        self.active: dict[str, int] = defaultdict(int)

    def enter(self, token_id: str) -> str | None:
        """None when the request may run, else why not."""
        now = time.monotonic()
        window = self.times[token_id]
        while window and now - window[0] > 60:
            window.popleft()
        if len(window) >= self.per_minute:
            return "rate"
        if self.active[token_id] >= self.concurrency:
            return "concurrency"
        window.append(now)
        self.active[token_id] += 1
        return None

    def leave(self, token_id: str) -> None:
        self.active[token_id] -= 1


def create_app(
    client: LLMClient,
    tokens: list[str],
    max_bytes: int = DEFAULT_MAX_BYTES,
    per_minute: int = DEFAULT_RATE_PER_MINUTE,
    concurrency: int = DEFAULT_CONCURRENCY,
) -> FastAPI:
    if not tokens:
        raise ValueError("at least one bearer token is required")
    logging.getLogger("httpx").setLevel(logging.WARNING)
    digests = [_digest(t) for t in tokens]
    limits = Limits(per_minute, concurrency)

    @asynccontextmanager
    async def lifespan(_app: FastAPI):
        yield
        close = getattr(client, "aclose", None)
        if close:
            await close()

    app = FastAPI(
        title="Silo reviewer", docs_url=None, redoc_url=None, openapi_url=None, lifespan=lifespan
    )

    def token_id(request: Request) -> str | None:
        """A short id of the token when it is one of ours; every digest is compared."""
        header = request.headers.get("authorization", "")
        scheme, _, given = header.partition(" ")
        given_digest = _digest(given)
        matched = 0
        for known in digests:
            matched |= int(hmac.compare_digest(given_digest, known))
        if scheme.lower() != "bearer" or not given or not matched:
            return None
        return given_digest.hex()[:8]

    @app.get("/healthz")
    async def healthz() -> dict:
        return {"ok": True}

    @app.post("/v1/review")
    async def review(request: Request) -> JSONResponse:
        request_id, started = uuid.uuid4().hex[:8], time.monotonic()
        who = token_id(request)
        if who is None:
            event(id=request_id, status=401)
            return _error(401, "missing or wrong bearer token")
        refused = limits.enter(who)
        if refused:
            event(id=request_id, token=who, status=429, limit=refused)
            return _error(429, "too many requests", **{"Retry-After": "30"})
        try:
            return await _review(request, request_id, who, started)
        finally:
            limits.leave(who)

    async def _review(request: Request, request_id: str, who: str, started: float):
        raw = await _body(request, max_bytes)
        if raw is None:
            event(id=request_id, token=who, status=413, limit=max_bytes)
            return _error(413, f"the request is larger than {max_bytes} bytes")
        try:
            payload = json.loads(raw)
            if not isinstance(payload, dict) or not _depth_ok(payload):
                raise TypeError
            diff, asked = payload["diff"], payload.get("lenses", list(LENSES))
            if not isinstance(diff, dict) or not isinstance(asked, list):
                raise TypeError
            if not asked or any(name not in LENSES for name in asked):
                raise ValueError
            lenses = [name for name in LENSES if name in asked]
        except (ValueError, KeyError, TypeError, RecursionError):
            event(id=request_id, token=who, status=400, bytes=len(raw))
            return _error(400, "expected {diff: object, lenses: [general|packages|code]}")

        async def one(name: str) -> dict:
            answer = await client.complete(SYSTEM_PROMPT, user_prompt(name, diff), RESULT_SCHEMA)
            return parse_answer(answer)

        outcomes = await asyncio.gather(*(one(n) for n in lenses), return_exceptions=True)
        results, failed = {}, []
        for name, outcome in zip(lenses, outcomes):
            if isinstance(outcome, Exception):
                failed.append(name)
                results[name] = {
                    "summary": "",
                    "findings": [],
                    "error": "this lens could not be produced",
                }
            elif isinstance(outcome, BaseException):
                raise outcome  # cancellation is not a failed lens
            else:
                results[name] = outcome
        status = 502 if len(failed) == len(lenses) else 200
        event(
            id=request_id,
            token=who,
            status=status,
            bytes=len(raw),
            lenses=lenses,
            failed=failed,
            findings=sum(len(r["findings"]) for r in results.values()),
            ms=round((time.monotonic() - started) * 1000),
        )
        if status == 502:
            return _error(502, "the review could not be produced")
        return JSONResponse({"lenses": results})

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
    return create_app(
        client,
        tokens,
        int(os.environ.get("REVIEWER_MAX_BYTES", DEFAULT_MAX_BYTES)),
        int(os.environ.get("REVIEWER_RATE_PER_MINUTE", DEFAULT_RATE_PER_MINUTE)),
        int(os.environ.get("REVIEWER_CONCURRENCY", DEFAULT_CONCURRENCY)),
    )
