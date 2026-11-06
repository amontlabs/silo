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

Errors: `401` wrong or missing token, `413` body over the limit, `429` over the per-token limits,
`400` malformed request, `502` every lens failed. When only some lenses fail, the answer is `200`
and a failed lens carries `error` with an empty `findings`. `GET /healthz` answers `{"ok": true}`.

## Choice of stack

AGENTS.md asks for established tools. This is FastAPI (MIT, widely deployed, maintained) on uvicorn
(BSD) calling the Anthropic Messages API through the official `anthropic` SDK (MIT), which handles
retries and `retry-after`. Answers use structured outputs (`output_config.format` with a JSON
schema), not a forced tool choice, which the default model rejects. A `refusal` or `max_tokens`
stop reason makes that lens fail. The `LLMClient` seam (`app/llm.py`) is where another provider
goes. A Rust axum binary was considered; the service is a thin prompt layer, so Python's smaller
code and easier prompt iteration win. Nothing is shared with the CLI except the JSON above.

Direct dependencies are in `requirements.in`; every version, transitive ones included, is pinned in
`requirements.txt` (the set the tests ran with).

## Data handling

- **Retention: none.** The diff lives in memory for the length of the request. It is not written to
  disk, a database or a log. The provider receives it as the prompt; its own retention terms apply
  to that call.
- Logs are one JSON object per request on stdout: request id, a short hash of the token (never the
  token), status, byte size, lenses, failed lenses, number of findings, duration. Never the diff or the answer. Uvicorn access logs are off in the image.
- The CLI sends the diff document (names of changed files, packages, settings and databases) and,
  for the code lens, the text of the code changes. Before sending it removes credential, key and
  environment files by name, binary changes, crontab and launch agent texts, file hashes and sizes,
  the home directory name, and redacts token-shaped strings (cloud keys, `ghp_`/`github_pat_`,
  `sk-`, `xox` tokens, private key blocks, assignments to secret-like names, long random strings).
  The redaction is pattern based and can miss a value; the provider receives whatever remains.
- The answer is checked on the user's device for wording the product does not claim; the prompts also
  forbid verdicts and the same words, but the CLI does not rely on that.
- The prompt puts the diff in `<diff>` and `<code_diff>` blocks, tells the model they are data, and
  gives the lens instruction after them. The answer is forced through a tool schema, so it is
  structured rather than parsed out of free text.

## Configuration

| Variable | Meaning |
| --- | --- |
| `REVIEWER_TOKENS` | Comma-separated bearer tokens. Required; the service refuses to start without one. |
| `ANTHROPIC_API_KEY` | The provider key. Required. |
| `REVIEWER_MODEL` | Default `claude-sonnet-5-5`. |
| `REVIEWER_MAX_BYTES` | Request body limit, default 262144 (the CLI's own cap). It is also the input cap per request. |
| `REVIEWER_RATE_PER_MINUTE` | Requests per token per minute, default 30; over it the answer is `429`. |
| `REVIEWER_CONCURRENCY` | Requests per token at once, default 4. |
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

1. Pin the base image by digest. Nothing here contacts the network, so the digest is yours to read:
   `docker buildx imagetools inspect python:3.12-slim --format '{{.Manifest.Digest}}'`.
2. Lock the dependencies with hashes; the image build stops without `requirements.lock`:
   `pip-compile --generate-hashes --output-file reviewer-service/requirements.lock reviewer-service/requirements.in`
   (run it where the network is available, and commit the file). The Dockerfile installs it with
   `--require-hashes`.
3. Build: `docker build --build-arg PYTHON_IMAGE=python:3.12-slim@sha256:<digest> -t silo-reviewer reviewer-service`.
4. Pick a host that terminates TLS (the CLI requires `https://` except on localhost) and keeps
   secrets in its secret store, such as Fly.io, Cloud Run or a small VM behind a reverse proxy.
5. Set `REVIEWER_TOKENS` (generate one per customer with `openssl rand -hex 32`) and
   `ANTHROPIC_API_KEY` as secrets; optionally `REVIEWER_MODEL`.
6. Run the container on port 8080; the image has a health check on `GET /healthz`.
7. Give each user the URL and a token. They set `SILO_REVIEWER_URL` and `SILO_REVIEWER_TOKEN` in
   their own environment, or `url:` in `~/.config/silo/reviewer.yaml`. A URL in a project's
   `.silo/silo.yaml` is ignored, so a cloned repository cannot send the token elsewhere.
8. The service limits each token (30 requests a minute, 4 at once by default); put a coarser rate
   limit at the proxy as well, since limits here are per process.

Revoke a token by removing it from `REVIEWER_TOKENS` and restarting.
