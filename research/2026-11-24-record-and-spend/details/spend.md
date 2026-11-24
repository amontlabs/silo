# Counting model spend and enforcing a budget at the boundary

Agent C, 24 November 2026. Every provider page below was read today unless marked "(not checked
today)". Source code of Codex was read today from `openai/codex` on GitHub, branch `main`. The
simulator is in [`../sim/budget_sim.py`](../sim/budget_sim.py) and its output in
[`../sim/output.txt`](../sim/output.txt).

## In short

- **Every provider gives the final count only at the end of a call.** Anthropic sends input tokens
  in `message_start` and the cumulative counts in the last `message_delta`. OpenAI's Responses API
  sends `usage` only in `response.completed`, `response.incomplete` or `response.failed`. OpenAI
  Chat Completions sends no usage at all in a stream unless the request set
  `stream_options.include_usage`, and Silo may not change the request. A call the program cancels
  has no final count anywhere.
- **The top-level `usage` of an Anthropic reply undercounts.** With server-side fallbacks and the
  advisor tool, other models run inside the same request and are billed at their own rates. Only
  `usage.iterations[]` lists them. The price also depends on fields the request may not carry:
  the workspace's `default_inference_geo` can apply the 1.1x multiplier with no `inference_geo` in
  the request, and `usage.speed` says whether fast mode ran.
- **The plan's "running calls finish" is right for an ordinary night and wrong in its tail.** In
  the simulator, with a $50 budget on main, Opus 5.5 and 8 parallel subagents, the median
  overshoot is $0.67 and the worst of 20 nights is $1.80. With Fable 5.2 in fast mode and 16
  subagents the worst night is $12.60. The worst case the plan allows is much larger: one Fable 5.2
  fast-mode call with 1M input tokens written to the 1-hour cache and 128k output costs $42.24, so
  16 of them leaving at $49.99 end the night at $725.
- **A cheap fix keeps the plan and removes the worst of the tail.** Before a request leaves, Silo
  can bound its input cost from the body, the model, `speed` and the cache markers. Admitting a
  request only when that input cost fits in what remains caused, on average, 0.1 false stops a
  night in one of six simulated settings and none in the other five. Reserving `max_tokens` for every call, as LiteLLM does by default, stopped 25 to
  136 conversations a night that would have fit, and left up to $46 of a $50 budget unspent.
- **Cutting streams at the limit trades overshoot for waste, and does not reach zero.** With
  thinking streamed as empty text (`display: "omitted"`, the default on current Anthropic models),
  the boundary cannot see 40 percent of the output it would cut on. The cut policy still overshot by
  up to $3.12 and threw away up to $5.96 of paid half-answers a night, on average.
- **The error at the limit decides whether the agent program stops or retries.** Codex stops,
  without retry, on an HTTP 429 whose body has `error.code` set to `project_spend_limit_exceeded`
  (and on several other codes, listed below). Claude Code stops without retry on a 429 marked
  `x-should-retry: false`, which is what Anthropic's own Claude apps gateway sends at its spend cap.
  A plain 429 is retried 10 times by Claude Code, 4 times by Codex, 2 times by both official SDKs.
- **Anthropic already ships the design Silo needs, for its own gateway.** The Claude apps gateway
  meters each reply after it ends, bills a cancelled stream at about four characters per output
  token, prices an unknown model at $5/$25 per million rather than refusing it, adds
  `anthropic-ratelimit-unified-*` headers so that Claude Code warns at 75 and 95 percent, and
  answers `429 billing_error` with `x-should-retry: false` at the cap. Its overshoot is the calls
  already running, as in Silo's plan.
- **For a subscriber, a dollar budget limits work that costs nothing extra.** One community
  measurement puts a Max 20x week at about $3,400 to $3,900 at API prices. What a subscriber can
  lose is the plan's own weekly limit, burned overnight, and usage credits billed beyond the plan.
  Both are readable at the boundary: `anthropic-ratelimit-unified-7d-utilization` for Claude, and
  `x-codex-secondary-used-percent` and `x-codex-credits-balance` for Codex under a ChatGPT sign-in.
- **The $453 case was a batch runner.** A batch request leaves at once and is billed hours later,
  through a different request. OpenAI's `background: true` responses also keep running after the
  client disconnects. No decision of the plan says how a budget counts either.
- **Prices change on a calendar.** Read today: Gemini 3.8 Flash doubles on 1 January 2027, OpenAI
  Ultrafast goes from 3x to 6x on 1 March 2027, and fast mode on Opus 4.8 ends on 30 November 2026.
  A price table needs dates of effect and service tiers, not one number per model.

## 1. What the plan says today

The decisions this report tests, in their own words, shortened:

| Entry | State | What it says |
| --- | --- | --- |
| `word-budget` | answered | A limit on what a computer, a conversation or a run may spend on models. |
| `budget-rule-or-precaution` | taken | A budget on a computer or a conversation is a rule. A budget on a run is a precaution. |
| `what-a-budget-promises` | answered | Once reached, no further metered request leaves. Calls under way finish, even past the budget. A model with no price does not leave under a budget. |
| `budget-denies-or-holds` | answered | Alert once at 80 percent. At the limit, deny with an error that says so. Never hold. |
| `budget-as-a-rate` | answered | A budget can also limit spend in any hour or any day. |
| `budgets-together` | answered | Every budget that covers a request applies. |
| `budget-of-a-fork` | answered | A fork spends from its parent's budget unless given its own. |
| `what-a-conversation-budget-promises` | answered | A new conversation starts at zero. The limit nothing gets around is the computer's. |
| `how-spend-is-shown` | answered | Dollars at API prices, also on a subscription, with the plan's share as the provider reports it. Unknown is never zero. |
| `what-traffic-gives` | answered | Token counts, cache reads and writes, the model that answered, plan limits with reset time. A count the provider does not give is recorded as missing. |
| `what-silo-changes-in-traffic` | answered | Silo changes one thing: it swaps a secret's real value in. A denied request is not sent, and the program gets an error. |
| `plan-limit-reached` | open feature | A checkpoint and an alert at the plan's limit, and a second alert at the reset time. |
| `spend-split-and-forecast`, `budgets-on-rates` | open features | Spend split by effort, speed and cache. A budget on usage billed outside a plan. |

Two gaps run through all of them. The plan does not say how a budget counts a call whose count is
missing, though `what-traffic-gives` says such calls happen. And the plan does not say where prices
come from (`spend-per-conversation` notes this itself).

## 2. Where each provider puts the counts

### 2.1 Anthropic Messages API

Sources: [streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
[pricing](https://platform.claude.com/docs/en/about-claude/pricing),
[rate limits](https://platform.claude.com/docs/en/api/rate-limits),
[errors](https://platform.claude.com/docs/en/api/errors),
[refusals and fallback](https://platform.claude.com/docs/en/build-with-claude/refusals-and-fallback).

A stream goes `message_start`, then content blocks, then one or more `message_delta`, then
`message_stop`, with `ping` events anywhere. The docs say: "The token counts shown in the `usage`
field of the `message_delta` event are *cumulative*."

`message_start` carries the input count before any output:

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_01G...","model":"claude-opus-5-5",...,
       "usage":{"input_tokens":2679,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":3}}}
```

The last `message_delta` carries the final counts. In the docs' web search example the input count
grows from 2,679 at the start to 10,682 at the end, because search results become input during
the call:

```
event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
       "usage":{"input_tokens":10682,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,
                "output_tokens":510,"server_tool_use":{"web_search_requests":1}}}
```

So the input count in `message_start` is final only when no server tool runs. Silo must settle on
the last `message_delta`.

The fields that change the price of one call:

| Field | What it changes |
| --- | --- |
| `input_tokens` | Tokens after the last cache breakpoint, at the base input price. The total input is `cache_read_input_tokens + cache_creation_input_tokens + input_tokens`. |
| `cache_creation_input_tokens`, split in `cache_creation.ephemeral_5m_input_tokens` and `cache_creation.ephemeral_1h_input_tokens` | A 5-minute write costs 1.25x the input price, a 1-hour write 2x. On Opus 5.5: $5 and $8 per million, against $4 for plain input. |
| `cache_read_input_tokens` | 0.1x on most models, 0.05x on Opus 5.5 and Sonnet 5.5 ($0.20 and $0.10), 0.025x on Fable 5.2 and 5.1. |
| `output_tokens` | Includes thinking. "Thinking happens and is billed the same under every setting" of `display`. |
| `server_tool_use.web_search_requests` | $10 per 1,000 searches. A failed search is not billed. |
| `server_tool_use.web_fetch_requests` | No charge beyond tokens. |
| `server_tool_use.code_execution_requests` | $0.05 per container-hour after 1,550 free hours a month, 5-minute minimum. Free when `web_search_20260209` or `web_fetch_20260209` is in the request. Billed by time, which no count in the reply gives. |
| `usage.speed` | Fast mode: $8/$40 on Opus 5.5, $16/$80 on Fable 5.2. Cache multipliers apply on top. |
| `usage.inference_geo` | `"us"` multiplies every category by 1.1 on Claude 4.6 and later. When the request omits `inference_geo`, "the workspace's `default_inference_geo` is used", so the request alone does not tell the price. |
| `usage.service_tier` | Priority Tier, on the models that still have it. Not on Opus 5.5, Sonnet 5.5, Fable 5.2. |
| `usage.iterations[]` | Every attempt inside the request: entries of type `message`, `fallback_message`, and, from agent E's reading of the [advisor tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/advisor-tool) page, `advisor_message`, each with its own `model` and counts. |

Long context: Claude 4.6 and later models, except Haiku 5.5, bill the whole 1M window at the
standard rate. Haiku 5.5 bills a request whose prompt is over 100,000 tokens at $0.50/$2.50
instead of $0.10/$0.50, counting cache reads and writes toward the threshold. Batch is half price.

**`usage.iterations` is the record of what is billed.** The refusals page says: "The `usage.iterations`
array is the per-attempt record of what you're billed. The top-level `usage` counts describe only
the attempt that produced the returned message." In its example, a request to `claude-fable-5`
declines and `claude-opus-4-8` answers. The top level shows 412 input and 264 output tokens. The
iterations show a first attempt of 535 input tokens on Fable 5 as well. Whether that first attempt
is billed depends on its refusal category: since September 2026 a refusal before any output is
billed only for `bio`, `frontier_llm` and `reasoning_extraction`, and "the billed categories may
change". A mid-stream decline is always billed. So exact spend needs the iterations, the category
of each declined one, and a table of billed categories with dates.

Rate-limit headers on every reply with an API key: `anthropic-ratelimit-requests-limit`,
`-requests-remaining`, `-requests-reset`, `anthropic-ratelimit-tokens-*`,
`anthropic-ratelimit-input-tokens-*`, `anthropic-ratelimit-output-tokens-*` (each `-limit`,
`-remaining`, `-reset` in RFC 3339), `retry-after`, `anthropic-priority-*` on Priority Tier, and
`anthropic-fast-*` for fast mode. `anthropic-workspace-id` names the workspace a key resolved to.

With a Claude subscription sign-in, replies carry the unified headers instead. They are not in
Anthropic's documentation. A reply captured in
[claude-code#12829](https://github.com/anthropics/claude-code/issues/12829) (1 December 2025)
shows:

```
anthropic-ratelimit-unified-status: allowed
anthropic-ratelimit-unified-5h-status: allowed
anthropic-ratelimit-unified-5h-reset: 1764554400
anthropic-ratelimit-unified-5h-utilization: 0.018416969696969696
anthropic-ratelimit-unified-7d-status: allowed
anthropic-ratelimit-unified-7d-reset: 1764615600
anthropic-ratelimit-unified-7d-utilization: 0.7370692663445869
anthropic-ratelimit-unified-representative-claim: five_hour
anthropic-ratelimit-unified-fallback-percentage: 0.2
anthropic-ratelimit-unified-reset: 1764554400
anthropic-ratelimit-unified-overage-disabled-reason: org_level_disabled
```

A reverse-engineering page adds `anthropic-ratelimit-unified-overage-status`, with values
`allowed`, `allowed_warning` and `rejected`
([RevEng101](https://apoorva87.github.io/RevEng101/UsageDashboard/Claude/docs/claude_oauth_guide.html)).
Claude Code's own docs confirm the headers exist and matter: it tells a temporary throttle from
a plan limit "by the absence of the unified quota headers a real limit response carries"
([errors](https://code.claude.com/docs/en/errors.md)), and its status line receives
`rate_limits.five_hour.used_percentage`, `rate_limits.seven_day.used_percentage` and their
`resets_at` ([status line](https://code.claude.com/docs/en/statusline.md)).

### 2.2 OpenAI Responses API

Sources: [streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events.md),
[pricing](https://developers.openai.com/api/docs/pricing.md),
[prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching.md),
[spend limits](https://developers.openai.com/api/docs/guides/spend-limits.md).

`response.created` and `response.in_progress` carry no usage. Usage arrives once, in the
`response` object of `response.completed`, `response.incomplete` or `response.failed`:

```
"usage": {
  "input_tokens": 0,
  "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0,
                           "cache_write_tokens_details": {"ttl_30m_tokens": 0, "ttl_12h_tokens": 0}},
  "output_tokens": 0,
  "output_tokens_details": {"reasoning_tokens": 0},
  "total_tokens": 0
}
```

Unlike Anthropic, OpenAI gives no input count before the end. The reply's `service_tier` says what
ran: `default`, `flex`, `priority` (also shown when the request asked `fast`), `scale` or
`ultrafast`. "This response value may be different from the value set in the parameter." So the
price is known from the reply, not the request.

What changes the price:

- **Tier.** Batch and Flex are half of Standard. Fast is about 2x (2.5x for gpt-5.5). Ultrafast is
  3x Standard on GPT-6.2 Astra and Sol until 28 February 2027 and 6x from 1 March 2027.
- **Long context.** Above 272k input tokens, input is 2x and output 1.5x on the GPT-6 and recent
  GPT-5 models. Example: gpt-6.2-sol goes from $2/$10 to $4/$15 per million.
- **Cache writes.** On GPT-5.6 and later, a 30-minute write costs 1.25x input and a 12-hour write
  2.5x. "These are total write rates, not surcharges." Reads are 0.1x, or 0.05x on GPT-6.2 Astra
  and Sol.
- **Regional processing.** A 10 percent uplift for models released on or after 5 March 2026.
- **Tools.** Web search $10 per 1,000 calls plus content tokens. Containers $0.03 to $1.92 per
  20-minute session. File search $2.50 per 1,000 calls plus $0.10 per GB per day of storage. Storage
  appears in no reply.

`POST /responses/{id}/cancel` exists only for `background: true` responses, which keep running
after the client disconnects (agent E's reading, and the reference: "Only responses created with
the `background` parameter set to `true` can be cancelled").

### 2.3 OpenAI Chat Completions

With `stream_options: {"include_usage": true}`, the stream ends with one extra chunk whose
`choices` is empty and whose `usage` has `prompt_tokens`, `completion_tokens`,
`prompt_tokens_details.cached_tokens` and `completion_tokens_details.reasoning_tokens`. Every other
chunk has `usage: null`. Without the option, which is the default, a streamed reply has no count
at all ([OpenAI community announcement](https://community.openai.com/t/usage-stats-now-available-when-using-streaming-with-the-chat-completions-api-or-completions-api/738156);
the reference page itself is too large to read whole). "If the stream is interrupted, you may not
receive the final usage chunk."

Silo may not add the option (`what-silo-changes-in-traffic`). So for a program that streams Chat
Completions without it, every call's count is missing by design, not by accident. The extra chunk
also breaks clients that read `choices[0]` without a guard, which is a second reason not to add it.

### 2.4 Codex under a ChatGPT sign-in

Codex talks to `https://chatgpt.com/backend-api/codex` (`CHATGPT_CODEX_BASE_URL` in
`codex-rs/model-provider-info/src/lib.rs`). It prefers a WebSocket to that host and falls back to
HTTP (`codex-rs/codex-api/src/endpoint/responses_websocket.rs`). The events in the frames are the
Responses events above, plus a `codex.rate_limits` event. So the boundary must read WebSocket
frames to count Codex at all.

Plan-limit headers, from `codex-rs/codex-api/src/rate_limits.rs`:

| Header | Meaning |
| --- | --- |
| `x-codex-primary-used-percent`, `x-codex-primary-window-minutes`, `x-codex-primary-reset-at` | The short window (5 hours on Plus and Pro, not checked today) |
| `x-codex-secondary-used-percent`, `x-codex-secondary-window-minutes`, `x-codex-secondary-reset-at` | The weekly window |
| `x-codex-credits-has-credits`, `x-codex-credits-unlimited`, `x-codex-credits-balance` | Credits bought beyond the plan |
| `x-codex-limit-name`, `x-codex-rate-limit-reached-type`, `x-codex-promo-message` | Which limit, and why it was reached |

The prefix is generic: `x-{limit_id}-primary-used-percent`, so other limit families such as
`x-codex-other-*` exist. At the limit, the backend answers 429 with a body whose `error.type` is
`usage_limit_reached` and which carries `plan_type`, `resets_at` and `limit_window_minutes`.

### 2.5 Gemini

[`generateContent` reference](https://ai.google.dev/api/generate-content),
[pricing](https://aistudio.google.com/docs/pricing). `usageMetadata` holds `promptTokenCount`
(which includes the cached part), `cachedContentTokenCount`, `candidatesTokenCount`,
`toolUsePromptTokenCount`, `thoughtsTokenCount`, `totalTokenCount`, per-modality details, and
`serviceTier` (`standard`, `flex`, `priority`). In `streamGenerateContent`, chunks carry
`usageMetadata` and the last one has the totals (not checked today which chunks carry it).

What changes the price: `gemini-3.1-pro-preview` doubles input above 200k prompt tokens ($2 to $4)
and raises output from $12 to $18. Explicit cache storage is billed by the hour ($4.50 per million
tokens per hour on 3.1 Pro), which no reply reports; Silo could compute it from the `cachedContents`
create request and its `ttl`, until the cache is deleted. Google Search grounding is $14 per 1,000
requests after 5,000 free a month. `gemini-3.8-flash` costs $0.75/$3.75 until 31 December 2026 and
$1.50/$7.50 from 1 January 2027.

### 2.6 OpenRouter

[Usage accounting](https://openrouter.ai/docs/use-cases/usage-accounting): "Full usage details are
always included automatically in every response", in the last chunk of a Chat Completions stream
and in `response.completed` of its Responses API. The usage has a `cost` field in dollars, besides
`prompt_tokens_details.cached_tokens`, `cache_write_tokens` and `reasoning_tokens`. OpenRouter is
the one provider that states the price of each call in the reply. Silo can still record its own
computed spend next to it and show a disagreement.

### 2.7 A cancelled call

No provider documents what it bills when the client closes a stream. What is known:

- OpenAI community: "terminating the connection ends generation", with "2 or 3 tokens in transit"
  billed ([forum](https://community.openai.com/t/api-billing-for-streaming-if-i-close-connection-midway/624323)).
- Google: "you are charged for all tokens that the model had already generated"
  ([forum](https://discuss.ai.google.dev/t/do-i-get-charged-for-generated-tokens-if-client-disconnects-during-a-vertex-ai-streaming-response/89490)).
- Anthropic offers no cancel call, so closing the connection is the only signal. One team measured
  about 180 output tokens billed after each "stop" click
  ([tianpan.co, June 2026](https://tianpan.co/blog/2026-06-03-the-streaming-abort-signal-your-frontend-sent-that-your-provider-billed-for-the-unsent-tokens-anyway),
  secondary source, provider not named).
- TrueFoundry's gateway keeps reading an OpenAI stream to the end after its client leaves, because
  OpenAI's usage then arrives at no extra cost. For Anthropic it estimates: input from
  `message_start`, output at "roughly one token per 3.5 characters of streamed text", with
  reasoning not counted ([TrueFoundry](https://www.truefoundry.com/docs/ai-gateway/stream-disconnect-cost-accounting)).
- Anthropic's own Claude apps gateway: "When a stream ends without the upstream's final usage
  frame, the meter bills a floor estimate of about four characters per output token for the text
  already sent to the client, so aborting requests early doesn't evade a cap"
  ([gateway spend limits](https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md)).

A floor from streamed characters misses thinking. On current Anthropic models thinking streams as
`thinking` blocks with empty text by default, and it is billed. A call cancelled during a long
think has a true cost the boundary cannot bound from the bytes it saw. It can bound it from time:
output tokens are at most the seconds elapsed times the model's top speed, and at most
`max_tokens`.

## 3. Prices

### 3.1 Where they come from

| Source | What it holds | Fit for Silo |
| --- | --- | --- |
| Provider pages | The prices that are billed, with tiers, multipliers and dates of change | The authority. Pages are Markdown for Anthropic and OpenAI (`.md` URLs), so a check can diff them. |
| LiteLLM `model_prices_and_context_window.json` | 7,158 entries today. Opus 5.5 matches Anthropic's page: input `4e-06`, `cache_creation_input_token_cost` `5e-06`, `cache_creation_input_token_cost_above_1hr` `8e-06`, read `2e-07`, output `2e-05`. GPT-6.2 Sol has keys for 12-hour writes above 272k on each tier. | A good second reading. Not a runtime dependency: LiteLLM's PyPI package shipped a credential stealer twice in 2026 (competitor survey). Copy values into Silo's own table, reviewed. |
| OpenRouter `GET /api/v1/models` | 473 models today, each with `pricing.prompt`, `completion`, `input_cache_read`, `input_cache_write`, `input_cache_write_1h`, `web_search`, `internal_reasoning`, and `overrides` such as `{"min_prompt_tokens": 272000, ...}` | Exact for OpenRouter traffic. Its ids differ from the providers' (`anthropic/claude-opus-5.5`, not `claude-opus-5-5`) and it lists aliases such as `~anthropic/claude-opus-latest`. |

### 3.2 How often they change

Changes found on the pages read today, all within a year:

- Sonnet 5's $2/$10 was "introductory pricing through August 31, 2026", then made standard; the
  planned rise to $3/$15 "will not occur".
- `gemini-3.8-flash` doubles on 1 January 2027.
- OpenAI Ultrafast on GPT-6.2 goes from 3x to 6x Standard on 1 March 2027.
- Fast mode on Opus 4.8: "Removal date: November 30, 2026."
- Anthropic's billed refusal categories "may change".
- OpenAI renamed Priority to Fast on its pricing page, while the API still answers
  `service_tier: "priority"`.

So one price per model is not enough. A price row needs: provider, model id, tier or speed, the
context band, each token category, the geography multiplier, and the date it takes effect.

### 3.3 A model with no price

The plan denies it under a budget (`what-a-budget-promises`). Anthropic's gateway prices it at
"the unknown-model tier of $5/$25 per million input/output tokens, so an ID the meter can't place
is never free". Both keep a new model from passing uncounted. They fail in opposite directions:
the plan stops the agent on release day, and the gateway can undercount a model priced above $25.
Fable 5.2 fast mode costs $80 per million output tokens, so the gateway's tier would count it at
less than a third.

Three cases make "no price" more common than a new release:

- An alias in the request. The served model appears only in the reply.
- A server-side fallback. Opus 5.5 is requested and `claude-opus-4-8` answers, as a
  `fallback_message` iteration.
- A program that sends a model id Silo has never seen, such as an OpenRouter id.

## 4. Budget enforcement elsewhere

| Product | How it counts | When it cuts | Overshoot it documents |
| --- | --- | --- | --- |
| Claude apps gateway (Anthropic) | After each reply, from token counts and list prices; a cancelled stream at about 4 characters per output token; unknown model at $5/$25 | Before a request, if period spend is over any cap: `429`, `error.type: billing_error`, `x-should-retry: false`, `retry-after` until the period ends | The calls already running. Store down: fails open by default, `enforcement.fail_closed_on_error: true` to fail closed. |
| Claude Code `--max-budget-usd` | Its own client-side estimate, after each reply | Print mode only; stops spawning subagents with `Budget limit reached` and stops background ones | "Spend can pass the cap by up to the cost of that one response, plus anything that subagents still running at that moment spend" ([agent loop](https://code.claude.com/docs/en/agent-sdk/agent-loop.md)) |
| Codex rollout budget | Units, with reminders injected into the conversation (`codex-rs/core/src/session/rollout_budget.rs`) | Refuses new tools with "rollout budget exhausted"; `budget_reached` from the server ends the turn | Not documented |
| LiteLLM proxy | Reserves "the request's maximum cost from the request body", using `max_tokens` when present, then settles on the actual cost | Before the request | Without reservation, concurrent requests exceed the budget. Without a database, budgets "fail open". |
| OpenRouter | Holds input plus `max_tokens` (or a fixed cap) at endpoint prices while the request runs | 402 `in_flight_budget_exhausted`; 403 for a key or guardrail budget, no `Retry-After` | The hold exists because "many requests running at the same time could commit more than your balance covers" |
| OpenAI project and org limits | Tracked spend | `429`, `error.code` `project_spend_limit_exceeded` or `organization_spend_limit_exceeded` | "Enforcement is not instantaneous, so recorded spend can slightly exceed the configured amount" |
| Anthropic Console limits | Tracked spend | A limit you set: `400 invalid_request_error`, "You have reached your specified API usage limits". The tier cap: `429 rate_limit_error`, `error.details.error_code: enforced_spend_limit_reached`, no `retry-after` | A Default Workspace limit "might not count or stop some usage" |
| Coder AI gateway | Recorded spend | When spend "meets or exceeds the budget" | "concurrent in-flight requests can cause a brief overage" |
| Cloudflare AI Gateway | Per-request cost from model pricing, "in real time" | Blocks further requests | Not documented; alerts "planned" |
| Tailscale Aperture | Quota buckets per user, group, agent or run, with overdrafts | 429 | Overdraft is the design |

Every one of them sees only traffic pointed at it. Silo's difference is not the counting. It is that
the count covers every process in the computer and every provider Silo reads.

## 5. Concurrent calls and overshoot

### 5.1 The worst case the plan allows

Under "running calls finish", the worst overshoot is the cost of every call that left just before
the limit. Each call's cost is bounded by its input (all of it written to the dearest cache) plus
`max_tokens` of output. From the simulator's table:

| Model | Input | `max_tokens` | One call | 4 at once | 16 at once |
| --- | ---: | ---: | ---: | ---: | ---: |
| Opus 5.5 | 200,000 | 64,000 | $2.88 | $11.52 | $46.08 |
| Opus 5.5 | 200,000 | 128,000 | $4.16 | $16.64 | $66.56 |
| Opus 5.5 | 1,000,000 | 128,000 | $10.56 | $42.24 | $168.96 |
| Fable 5.2 | 1,000,000 | 128,000 | $21.12 | $84.48 | $337.92 |
| Fable 5.2, fast | 200,000 | 128,000 | $16.64 | $66.56 | $266.24 |
| Fable 5.2, fast | 1,000,000 | 128,000 | $42.24 | $168.96 | $675.84 |

The input part is known, or bounded, before the request leaves. The model, `speed`, the cache
markers and their `ttl`, and the body's size are all in the request. The output part is not known
until the end.

How tight a bound Silo can put on the input before the request leaves:

- No tokenizer for Claude 4.7 and later is public, and the newer tokenizer "produces approximately
  30% more tokens for the same text". A client-side count is an estimate.
- Text in UTF-8 has at most one token per byte, so the body's text bytes are an upper bound.
  That bound is about four times too high.
- A tight estimate comes from the conversation. The previous reply in the same conversation gave
  the exact size of the prefix (`cache_read + cache_creation + input`). The new request is that
  prefix plus the previous output plus the new tool results, whose bytes Silo sees.
- `count_tokens` is exact and free, but it is a second request sent with the user's credential.
  `forwarded-byte-for-byte` says Silo's connection does not imitate the program, and
  `anthropic-reads-intermediate` keeps Silo from using a subscription's sign-in for its own calls.
  Not recommended.

### 5.2 The simulator

`sim/budget_sim.py` replays 20 synthetic nights of 8 hours. Each night has:

- one main conversation all night, a call about every minute, its prefix in the 1-hour cache,
  compacting at 400k tokens;
- bursts of N parallel subagents about once an hour, 5 to 30 calls each, 25k tokens of context to
  start, 5-minute cache;
- a call at 01:30 that reads 250k new tokens into the 1-hour cache;
- a runaway loop from 03:00: the same 180k context every 25 seconds;
- output lengths drawn from a long-tailed distribution (median 1,500 tokens), capped at
  `max_tokens`; 40 percent of output is thinking, invisible to the boundary while it streams;
- a client-side input estimate off by -10 to +15 percent.

Without any budget a night costs a median $214 on Opus 5.5 with 8 subagents. The runaway loop is
$33 of that, because every one of its calls is a cache read at $0.20 per million.

A conversation that gets a denial stops, as Claude Code and Codex do on the right error (section 7).
A false stop is a denied request that would have fit had the boundary known every true cost.

The policies:

- `finish` is the plan. A request leaves when what is counted (settled calls, plus the input and
  the visible output of running ones) is under the budget.
- `admit_input` is the plan plus one check. The request's own input, priced at the dearest rate it
  can incur, must also fit in what remains.
- `reserve_max` is LiteLLM's default. Each running call holds its input at the dearest rate plus
  `max_tokens` of output, and a request leaves only if its own hold fits.
- `reserve_input` holds the input as the conversation predicts it plus 12,000 output tokens, the
  simulator's 95th percentile.
- `cut` admits like `finish`, and cuts every running call once what is counted reaches the budget.
  About 180 tokens past the cut are billed.

Results for a $50 budget on main (20 nights each; "left" is what remained unspent when the first
false stop happened, median):

| Night | Policy | Mean spend | Overshoot, median | Overshoot, worst | False stops a night | Left at first false stop | Paid for cut calls |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Opus 5.5, 8 subagents, 64k | finish | 50.79 | 0.67 | 1.80 | 0 | – | 0 |
| | admit_input | 50.62 | 0.59 | 1.37 | 0 | – | 0 |
| | reserve_max | 48.57 | 0 | 0 | 55.0 | 16.68 | 0 |
| | reserve_input | 49.95 | 0 | 0.94 | 28.5 | 1.89 | 0 |
| | cut | 50.34 | 0.22 | 1.79 | 0 | – | 0.90 |
| Opus 5.5, 16 subagents, 128k | finish | 51.88 | 1.53 | 5.02 | 0 | – | 0 |
| | admit_input | 51.79 | 1.48 | 5.02 | 0 | – | 0 |
| | reserve_max | 47.14 | 0 | 0 | 122.4 | 46.09 | 0 |
| | reserve_input | 50.36 | 0 | 2.10 | 23.6 | 4.80 | 0 |
| | cut | 50.47 | 0.46 | 1.02 | 0 | – | 1.93 |
| Fable 5.2 fast, 16 subagents, 128k | finish | 55.67 | 5.53 | 12.60 | 0 | – | 0 |
| | admit_input | 55.36 | 5.09 | 12.44 | 0 | – | 0 |
| | reserve_max | 39.42 | 0 | 0 | 135.6 | 44.83 | 0 |
| | reserve_input | 50.25 | 0.02 | 3.06 | 44.9 | 16.90 | 0 |
| | cut | 51.60 | 1.59 | 3.12 | 0 | – | 5.96 |

The rows for 4 subagents and for 8 subagents on Fable are in `output.txt`; they fall between these.

What the numbers say:

1. **On an ordinary night the plan's overshoot is small.** Calls finish at a median of 1,500
   output tokens, not at `max_tokens`. Opus 5.5 overshoots by 1 to 3 percent of the budget at the
   median.
2. **The tail grows with price and parallelism.** Fable 5.2 in fast mode with 16 subagents
   overshoots by up to 25 percent of the budget on a synthetic night, and by up to 13 times the
   budget in the worst case of section 5.1.
3. **Reserving `max_tokens` is the wrong fix for agents.** It never overshoots, and it stops
   between 25 and 136 conversations a night that would have fit. On Fable in fast mode the first
   of them came with a median $45 of the $50 still unspent, because 16 holds of 128k output at $80
   per million are $164.
4. **`admit_input` costs almost nothing.** In these nights it changed little, because the one large read
   came early. Its value is the case the trace does not contain: a 1M-token request at $49 of $50.
   Under `finish` that call leaves and costs up to $42 more. Under `admit_input` it is denied,
   with an error that says why.
5. **Cutting does not reach zero and wastes money.** It cannot see thinking, so it cuts late. Every
   cut call is paid for and its answer thrown away. Claude Code then re-sends the request, which
   the boundary denies.

A rate catches a runaway early, and also stops other work. With a $150 total, the runaway loop runs
until 06:04, when the total is reached. With $15 an hour on top, it is stopped at 03:29, the night
costs $76 instead of $149, and 48 conversations are denied instead of 23, most of them ordinary
subagents that hit the same hourly limit. A runaway loop on a cached context costs about as much
per hour as ordinary work, so a rate on the whole computer cannot tell it apart. What tells it
apart is that it repeats: the same tool call, the same failing output. That is an opinion, and
under `less-sure-less-power` it can only take protective actions.

## 6. Subscriptions

Most users pay for Claude Code and Codex with a plan, not a key. For them:

- **Dollars at API prices measure work, not money.** A community conversion of three accounts'
  logs puts a Max 5x week at about $1,000 to $1,200 at list prices, and a Max 20x week at about
  $3,400 to $3,900 ([classmethod](https://dev.classmethod.jp/articles/claude-max-20x-weekly-limit-not-4x/),
  a measurement, not Anthropic's figure). A $50 budget is 1 to 5 percent of such a week. It stops
  an agent that would have cost the subscriber nothing more.
- **What a subscriber can lose is the plan itself.** An overnight run that burns 60 percent of the
  weekly limit leaves the subscriber blocked on Thursday. `spend-split-and-forecast` already names
  this as "a common surprise". The 7-day utilization header gives it as a fact, for the whole
  account.
- **And usage credits.** Beyond the plan, Claude Pro, Max and Ultra can draw on usage credits,
  billed in money, with a monthly limit set on claude.ai ([costs](https://code.claude.com/docs/en/costs.md)).
  Codex has credits too, and `x-codex-credits-balance` reports their balance on each reply. That
  is real money. `budgets-on-rates` proposes a cap on it and notes that telling a request inside the
  plan from one billed outside it "is to find out". For Codex the balance header answers it: a
  request that lowered the balance was billed outside the plan. For Claude the
  `overage-status` and `overage-utilization` headers probably answer it, but they are not
  documented (one report says the overage figure stayed at 0.0 across successful calls,
  [claudeissues 45380](https://claudeissues.com/issue/45380-bug-raw-sdk-calls-with-oauth-token-bill-to-plan-allowance-instead-of-extra-usage)).
- **A budget on the plan's share is useful, and it is account-wide.** "Main may use at most 30
  percent of my weekly limit tonight" is what a subscriber means. The header is a fact the provider
  states. But it describes the whole account: a session on the subscriber's laptop moves it too. So
  such a limit is exact about the account and not about main. Said that way, it is a rule on a
  fact, and its worst case is that other use of the same account stops main early.

What "budget" means for a Max subscriber, concretely: main's alert says "Main has used 22 percent
of your weekly Claude limit since 21:00 (account-wide, as Anthropic reports it). At API prices this
work would have cost $164." The limit the subscriber sets is on the share. A dollar budget stays
available for keys and for usage credits.

## 7. At the limit: what the agent program receives

### 7.1 What makes each program stop

**Codex** classifies errors in `codex-rs/protocol/src/error.rs`. These never retry
(`RetryPolicy::Never`), among others: `SessionBudgetExceeded`, `QuotaExceeded`,
`APISpendLimitExceeded`, `UsageLimitReached`, `InferenceHookDenied`, `InvalidRequest`. A plain 429
becomes `RetryLimit`, which retries as the server directs. A stream that drops before
`response.completed` "is a transient error and will automatically retry the turn". Defaults:
`DEFAULT_REQUEST_MAX_RETRIES = 4`, `DEFAULT_STREAM_MAX_RETRIES = 5`.

The body codes that map to those, from `codex-rs/codex-api/src/error.rs` and `api_bridge.rs`:

| `error.code` | HTTP status | Codex shows |
| --- | --- | --- |
| `project_spend_limit_exceeded` | 429 or in-stream | "Your API project has reached its spend limit." |
| `organization_spend_limit_exceeded`, `api_key_spend_limit_exceeded` | 429 or in-stream | the same, for organization or key |
| `budget_reached` | 400 or in-stream | "budget reached" |
| `inference_hook_denied` | 403 or in-stream | the `message` as written |
| `insufficient_quota`, `credit_balance_exhausted` | 429 | "Quota exceeded. Check your plan and billing details." |

Over the WebSocket, an error frame `{"type": "error", "status": 429, "error": {"code": ..., "message": ...}}`
is turned into the same HTTP error and parsed the same way (`responses_websocket.rs`).

**Claude Code** ([errors](https://code.claude.com/docs/en/errors.md),
[env vars](https://code.claude.com/docs/en/env-vars.md)): it retries transient failures "up to 10
times", including "Temporary 429 throttles, but not a gateway's spend-limit `429`". With
`CLAUDE_CODE_RETRY_WATCHDOG` set, it retries 429 and 529 indefinitely, except that it "fails at once
when a standard-speed request gets a `429` that reports a spend limit". The Claude apps gateway
"marks each blocked `429` response `x-should-retry: false`, so in the terminal Claude Code shows
this message without retrying", and it shows "the gateway's `429` message as is". A dropped
connection before any part of the reply is complete is re-sent. A drop after a completed text block
or tool call is not re-sent: Claude Code "runs any tool calls Claude finished" and continues.

**The official SDKs** retry 408, 409, 429 and 5xx twice by default, "unless the `x-should-retry`
header says otherwise".

### 7.2 What Silo should send

For Anthropic traffic, the shape of Anthropic's own gateway:

```
HTTP/1.1 429 Too Many Requests
content-type: application/json
x-should-retry: false
request-id: silo_01J...

{"type": "error",
 "error": {"type": "billing_error",
           "message": "spend limit reached · Silo: main's budget of $50.00 is reached ($50.41 spent since 21:00). Raise it in Silo, then continue the session."},
 "request_id": "silo_01J..."}
```

For a rate budget, add `retry-after` with the seconds until the last hour is under its limit, and
say so in the message. Whether Claude Code treats this exactly as it treats its gateway without
`ANTHROPIC_BASE_URL` pointing at a gateway is to check on a live run: it is the first thing the
check of `reading-each-agent-program` should try. The fallback is Anthropic's own 400 for a limit
the user set (`invalid_request_error`, "You have reached your specified API usage limits"), which
no client retries.

For OpenAI traffic, over HTTP and to `api.openai.com` or `chatgpt.com`:

```
HTTP/1.1 429 Too Many Requests
content-type: application/json
x-should-retry: false

{"error": {"type": "insufficient_quota", "code": "project_spend_limit_exceeded", "param": null,
           "message": "Silo: main's budget of $50.00 is reached ($50.41 spent since 21:00). Raise it in Silo."}}
```

Codex stops on it and prints its own sentence, "Your API project has reached its spend limit", not
Silo's. The code that makes Codex print Silo's sentence is `inference_hook_denied` with a 403. That
code means a policy hook, which is close to what a Silo budget is, but it is OpenAI's word for
another product. The choice is between a precise word in Codex's own text and Silo's text in a
borrowed code. I lean to `project_spend_limit_exceeded`, because the alert from Silo says the rest.
Over a WebSocket, the same body goes in an error frame with `"status": 429`.

For Gemini, a 429 `RESOURCE_EXHAUSTED` is what Google sends for quota, and Gemini CLI's retry rules
were not checked today.

### 7.3 What the person sees

The alert at 80 percent, as `budget-denies-or-holds` says, once per budget:

> Main has spent $40.12 of its $50 budget since 21:00. Most of it went to Claude Code's
> conversation "refactor the parser" ($31.80). At the last hour's pace the budget is reached around
> 02:10. [Raise to $75] [Open main]

At the limit:

> Main reached its $50 budget at 02:07. Claude Code and two subagents got an error that says so
> and stopped. Calls already running finished: main ended at $50.41. [Raise to $75] [Open the record]

After raising it, the person continues the session (`claude --continue`, or a new turn in Codex).
Silo starts nothing by itself, as `plan-limit-reached` says for a plan's limit.

## 8. What the plan does not cover yet

- **A call with a missing count.** `what-traffic-gives` records it as missing. A budget still has to
  count something, or a cancelled call is free and a program can spend by cancelling. Anthropic's
  gateway bills a floor from streamed characters. That floor misses thinking. A bound from time
  (seconds elapsed times the model's top speed, at most `max_tokens`) does not.
- **Chat Completions without `include_usage`.** Every streamed call of such a program has a missing
  count, by design. Under a budget it is either counted at its bound or denied like a model with no
  price.
- **Batches and background responses.** The $453 case in the competitor survey was a batch runner
  started with a key from `.env`. A batch create leaves at once, its cost arrives hours later in a
  results file fetched by another request, and OpenAI uploads the requests as a file first.
  LiteLLM releases its reservation "after the submission response" and records the cost when the
  batch completes. OpenAI's `background: true` responses keep running after the client
  disconnects. Neither fits "the calls already under way finish".
- **Anthropic message threads.** With the `message-threads-2026-08-12` beta, the API keeps the
  conversation and "each later request sends only the new messages". The request then no longer
  holds the whole conversation, which `recognising-a-conversation` relies on, and its size no
  longer bounds its input. The reply's usage still gives the count.
- **Code execution and storage billed by time.** Anthropic's container hours, OpenAI's container
  sessions and file-search storage, and Gemini's cache storage appear in no reply. A budget "on
  models" can name them as left out, or count them from the requests that start them.

## Recommendations for the plan

1. **`what-a-budget-promises` (answered, proposal).** Keep "running calls finish". Add: "A request
   leaves only if its own input, at the highest rate it can be billed, fits in what remains of
   every budget that covers it." The input is bounded from the request alone. In the simulator it
   caused 0.1 false stops a night in one setting of six, and none in the others. It removes the case of a 1M-token request at $49 of $50, which
   under the plan today can cost up to $42 more on Fable 5.2 in fast mode. Also state the worst
   case in numbers, so that the person setting a budget sees it: "with 16 calls running on Opus 5.5
   at 128k output, up to $67 over".
2. **`what-a-budget-promises` (answered, proposal).** Say how a budget counts a call whose count is
   missing: at an upper bound, from the input it is known to have and the output it can have had in
   the time it ran, at most `max_tokens`, shown as "at most $X". Without this, cancelling a stream
   spends without counting. Anthropic's gateway does the same with a weaker floor.
3. **`what-traffic-gives` (answered, proposal).** Name `usage.iterations[]` as the source of a
   call's count when it is present, with each iteration's own model, and the reply's
   `usage.speed`, `usage.inference_geo` and `service_tier` (OpenAI and Gemini) as the source of
   its tier. The top-level counts undercount fallbacks and the advisor, and the request does not
   always state the geography.
4. **New decision: where prices come from.** A table kept by Silo, read from the providers' pages,
   with a date of effect for every row, a row per tier and context band, checked against LiteLLM's
   file and OpenRouter's `/models` and never loaded from them at run time. Its checks diff the
   providers' Markdown pages. Today's pages already hold three dated changes (Gemini 3.8 Flash on
   1 January 2027, Ultrafast on 1 March 2027, Opus 4.8 fast mode on 30 November 2026).
5. **`what-a-budget-promises` (answered, challenge to one sentence).** "A model Silo has no price
   for does not leave": keep it, and add that the price is looked up for the model the request
   names, while the spend settles on the models the reply names. A server-side fallback from Opus
   5.5 to Opus 4.8 is then priced, and an alias in the request is resolved by the reply.
6. **`budget-denies-or-holds` (answered, proposal).** Fix the error per provider, because the wrong
   shape makes Claude Code retry 10 times and the SDKs twice. Anthropic: 429, `billing_error`,
   `x-should-retry: false`, Silo's message in `error.message`, as Anthropic's own gateway sends.
   OpenAI and the ChatGPT backend: 429 with `error.code: project_spend_limit_exceeded`,
   `x-should-retry: false`, and the same body inside a WebSocket error frame. Check each in the
   live checks of `reading-each-agent-program`.
7. **`budget-denies-or-holds` (answered, proposal).** The alert at 80 percent names the
   conversation that spent most and the time at which the budget is reached at the last hour's
   pace. That forecast is an opinion and is shown as one (`kinds-look-different`).
8. **`how-spend-is-shown` (answered, proposal).** For a subscription, show the plan's share first
   and the API-price dollars second, labelled "this work at API prices". A Max 20x week is about
   $3,400 at API prices, so a dollar figure alone reads as an alarm when nothing was paid.
9. **New decision: a limit on a plan's share.** For subscribers, a limit such as "main uses at most
   30 percent of my weekly limit", triggered by the provider's 7-day figure
   (`anthropic-ratelimit-unified-7d-utilization`, `x-codex-secondary-used-percent`). It is a rule on
   a fact, account-wide, and says so: use of the same account elsewhere counts toward it. It
   answers the surprise `spend-split-and-forecast` names.
10. **`budgets-on-rates` (open, proposal).** For Codex, a request billed outside the plan is one
    after which `x-codex-credits-balance` fell. For Claude, the `overage` headers are the lead, to
    confirm on captured traffic. Keep a rate on the whole computer, and say its cost: in the
    simulator a $15 hourly rate stopped the runaway loop 2.6 hours earlier and also stopped 25 more
    ordinary conversations.
11. **New decision: batches and background responses under a budget.** Proposed: under a budget, a
    batch create and a `background: true` response are denied, with an error that says why, until
    Silo can count them, as a model with no price is. This closes the route of the $453 case.
12. **New decision: Chat Completions streams without usage.** Proposed: a streamed Chat Completions
    request without `stream_options.include_usage` is counted at its bound under a budget, and the
    app says that the program's spend is an upper bound. Silo does not add the option, as
    `what-silo-changes-in-traffic` requires.
13. **`model-traffic-read` and `reading-each-agent-program` (open, proposal).** List Codex's
    WebSocket to `chatgpt.com/backend-api/codex` as a transport to read. Without it, Codex under a
    ChatGPT sign-in has no spend, no plan share and no budget.
14. **`budget-on-a-run` (open, agrees with its suggested drop).** The simulator adds a reason: a
    runaway loop on a cached context costs about as much per hour as ordinary work, so neither a
    run budget nor a rate can tell it apart. What tells it apart is repetition, which is an opinion
    for a precaution, not a budget.
15. **`recognising-a-conversation` (answered, note).** Anthropic's message threads beta sends only
    the new messages. A conversation using it cannot be recognised by its content. It needs the
    `thread` field as its link, as OpenAI's `previous_response_id` is already followed.
