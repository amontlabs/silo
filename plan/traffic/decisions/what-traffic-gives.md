# What does Silo learn from model traffic

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Read a field only at its own level.** When reading a structured message, look for envelope
  fields only at the outer level, compare names on their raw bytes, and give up when the structure
  does not balance, rather than guessing where you are. Silo reads spend from model responses. A
  response whose text contains `"usage": {...}` must not change the spend Silo counts.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/rpc/scan.zig#L70))
- **Explicit transport security.** Set TLS versions, certificates and timeouts explicitly instead of
  inheriting system defaults. For the traffic Silo opens to read model calls, the helper's TLS
  settings are written down and tested.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/provider/REVIEW_SUMMARY.md?plain=1#L241))
- **Route before parsing, stream the rest.** Decide where a request goes from its headers before
  parsing its body, and write responses straight to the output. Model responses stream. The helper
  reads what it needs to count spend as it passes and keeps nothing else in memory.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/rpc/response.zig#L1))
- **Outside contracts tested against their own examples.** For each outside contract, name the
  specification that governs it, test against its published examples, and never edit an example to
  make a test pass. Each vendor's API and streaming format is tested against its published examples,
  and against captured traffic with every credential removed. Virtio and Apple's framework are named
  as the authorities for theirs.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/TESTING.md?plain=1#L451))

## The count against the bill (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`. A boundary prototype read 140 calls of Claude Code,
Codex, Gemini CLI and a scripted client, in Anthropic's, OpenAI's two and Gemini's three formats,
against stand-in providers that billed each call.

- The 134 calls that ended with the provider's count were counted at $8.465880; the stand-in billed
  $8.465877.
- Ten calls fell back to another model inside the call. Their top-level counts alone missed 10% of
  their cost: `usage.iterations` has to be read.
- Six calls had no final count: three Chat Completions streams that did not ask for usage, and
  three calls the program dropped while the model reasoned. They were billed $0.20. Their visible
  output, at four characters a token, came to $0.04. The rest was reasoning, which streams as
  nothing.

So "a count the provider does not give is recorded as missing" holds, and the spend of such a call
needs a second figure the app marks as estimated. Visible characters are a poor floor for it.
Seconds elapsed at the model's speed, up to `max_tokens`, is a better one.

## What the token counts leave out (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-spend-budget-edges`.

- **Searches the provider runs.** OpenAI's web search bills $10 per 1,000 calls, outside `usage`:
  each one is a `web_search_call` item of the response. Gemini 3's grounding bills $14 per 1,000
  queries, listed in `groundingMetadata.webSearchQueries`. Counting only tokens missed 83% of a run
  of five calls with searches ($0.018 for $0.104). Counting the items and the queries made it
  exact.
- **Context the provider keeps.** A Responses request that names `previous_response_id` or a
  `conversation` sends only its new input, and the provider bills the whole chain again as input
  (OpenAI's conversation-state guide says so). Estimated from its size, the input of each such
  call was 99.9% short; from the counts of the response it names, it was exact.
- **A reply that says the work goes on.** A response with `background: true` answers `queued` with
  no usage. Read as a whole reply, it counted $0; its counts come from `GET /v1/responses/{id}`
  later. See `budget-and-work-out-of-sight`.

## The model that really answered, for its price (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-model-prices`. The price of a call
follows the model that answered it, which this answer already records. Two current cases show the
request's model is not enough. `gemini-flash-latest` and `gemini-pro-latest` point at whichever
model Google moves them to; LiteLLM's table kept the old models' prices for them for 7 to 12
months after a move. And from 3 December 2026, Google sends every request for
`gemini-omni-1.1-flash` to `gemini-omni-1.2-flash` and bills it at that model's price. The
research's boundary prices the settled call by the model the reply names.
