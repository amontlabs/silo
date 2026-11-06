# Silo hosted reviewer service

The service behind `silo review` (a Pro feature). It receives the diff of a fork, asks an LLM to
read it through up to three lenses (general, packages, code) and returns findings. It is advisory:
it reads the diff only and can miss things.

Not deployed from this repository's automation. The steps for the owner are below.

## API

`POST /v1/review` with `Authorization: Bearer <token>`:

```json
{ "diff": { "...": "the diff document" }, "lenses": ["general", "packages", "code"] }
```

`lenses` is optional (all three). The answer:

```json
{ "lenses": { "general": { "summary": "...", "findings": [
  { "severity": "info|low|medium|high", "file": "~/app/run.sh", "message": "..." } ] } } }
```

Errors: `401` wrong or missing token, `413` body over the limit, `400` malformed request, `502` the
provider failed or answered something unusable. `GET /healthz` answers `{"ok": true}`.

## Choice of stack

AGENTS.md asks for established tools. This is FastAPI (MIT, widely deployed, maintained) on uvicorn
(BSD) with httpx (BSD) calling the Anthropic Messages API directly. Considered and left out:

- The `anthropic` SDK: the service needs one POST; a direct call keeps three dependencies instead of
  a larger tree, and the pluggable `LLMClient` seam (`app/llm.py`) is where another provider goes.
- A Rust axum binary: the CLI is Rust, but the service is a thin prompt layer, so Python's smaller
  code and easier prompt iteration win. Nothing is shared with the CLI except the JSON above.

Dependencies are pinned in `requirements.txt`.

## Data handling

- **Retention: none.** The diff lives in memory for the length of the request. It is not written to
  disk, a database or a log. The provider receives it as the prompt; its own retention terms apply
  to that call.
- Logs are one JSON object per request on stdout: request id, status, byte size, lenses, number
  of findings, duration. Never the diff or the answer. Uvicorn access logs are off in the image.
- The CLI sends only the diff document (no crontab or launch agent text, credential file changes,
  home directory name or credential values), and filters the answer's wording before showing it.
  The prompts also forbid verdicts and the same words, but the CLI does not rely on that.

## Configuration

| Variable | Meaning |
| --- | --- |
| `REVIEWER_TOKENS` | Comma-separated bearer tokens. Required; the service refuses to start without one. |
| `ANTHROPIC_API_KEY` | The provider key. Required. |
| `REVIEWER_MODEL` | Default `claude-sonnet-5-5`. |
| `REVIEWER_MAX_BYTES` | Request body limit, default 262144 (the CLI's own cap). |
| `ANTHROPIC_BASE_URL` | Default `https://api.anthropic.com`. |

## Tests

They run offline: the real HTTP layer on localhost with a fake LLM client.

```sh
python3 -m pip install -r requirements-dev.txt
python3 -m pytest -q
```

The CLI's end-to-end test (`cargo test -p silo-cli --test hosted`) starts this service with the
fake client and runs `silo review` against it; it skips when the dependencies are missing.

## Deploy (for the owner)

1. Build: `docker build -t silo-reviewer reviewer-service`.
2. Pick a host that terminates TLS (the CLI requires `https://` except on localhost) and keeps
   secrets in its secret store, such as Fly.io, Cloud Run or a small VM behind a reverse proxy.
3. Set `REVIEWER_TOKENS` (generate one per customer with `openssl rand -hex 32`) and
   `ANTHROPIC_API_KEY` as secrets; optionally `REVIEWER_MODEL`.
4. Run the container on port 8080 and check `GET /healthz`.
5. Give each user the URL and a token: `SILO_REVIEWER_URL` (or `reviewer.url` in
   `.silo/silo.yaml`) and `SILO_REVIEWER_TOKEN`.
6. Add a rate limit at the proxy; the service has none of its own.

Revoke a token by removing it from `REVIEWER_TOKENS` and restarting.
