# A plan's window across devices

Research of 25 November 2026, by an agent, for polarzero. #374 capped a plan's 5-hour window at
50% and found the cap held while a share per computer did not, because the provider reports the
whole account. This asks the next questions: what each provider's replies and errors say about a
plan's use, whether Silo can know the true share of one computer, and how a budget on a window
behaves when several Silo computers on several Macs and devices outside Silo share one plan.

It builds on #355 (`research/2026-11-24-gap-fixes/6-window-budgets-and-traffic-alerts.md`, in
#375), which already read Anthropic's and OpenAI's window headers, found Codex's per-conversation
figure, and measured four estimators of a computer's share (the best, E2, learns the price of a
point of the window from the computer's own calls). That work is not repeated here. What is new:
Google, today's builds, the case #393 opened (a computer whose Claude traffic Silo passes on
unread), Claude Code's own report of its spend, and a reserve shared by several Macs.

| File | Holds |
|---|---|
| [`prototype/fake_anthropic_usage.py`](prototype/fake_anthropic_usage.py) | #355's fake Messages API with a plan's window, plus a delay per reply |
| [`prototype/otlp_collector.py`](prototype/otlp_collector.py) | An OpenTelemetry collector standing in for Silo listening to a program's own report |
| [`prototype/run_otel.sh`](prototype/run_otel.sh), [`compare.py`](prototype/compare.py) | Real Claude Code on a made-up plan token, its report against what the fake served |
| [`prototype/sim_reserve.py`](prototype/sim_reserve.py) | A reserve on the week across up to 6 Silo computers, with devices outside Silo |
| [`prototype/results/`](prototype/results/) | The output of every run reported here |

## What was found

- **No provider says how much of a window one computer used, except OpenAI on a separate call.**
  Anthropic sends the account's figures on every reply to a plan sign-in and nothing below the
  account anywhere, including `/api/oauth/usage` (Claude Code 2.1.332 read today, as #355 read
  2.1.331). OpenAI sends the account's figures on replies and has a per-conversation figure only
  behind `POST /wham/usage/thread_usage/query_v2`, called with the user's sign-in (Codex source at
  `e8fb104`, 25 November). Codex names each request's conversation in a `thread-id` header, so a
  boundary that reads Codex knows which conversations are its computer's.
- **Google counts requests, so its share per computer is a fact.** A Google sign-in in Gemini CLI
  draws on a daily number of model requests per user (1,500 on Google AI Pro, 2,000 on Ultra),
  shared between Gemini CLI and Code Assist's agent mode on every device
  ([quotas](https://developers.google.com/gemini-code-assist/resources/quotas)). The boundary
  counts its computer's requests exactly. Gemini CLI 0.71.1 itself fetches the account's
  remainder per model (`retrieveUserQuota`: `remainingAmount`, `remainingFraction`, `resetTime`)
  after a model reply whenever its copy is older than 30 seconds, so a boundary reading it also
  sees the account's figure, at most 30 s old, without any call of its own.
- **Where Silo passes a Claude plan's traffic on unread (#393), it sees no window at all.** No
  header, no 429, no reset time. Neither #376's reserve nor #355's share can be set there, and
  `plan-limit-reached` cannot fire.
- **Claude Code reports its own spend exactly, on a plan, if asked to.** With its OpenTelemetry
  export pointed at a local collector, a real Claude Code signed in with a plan token sent one
  `api_request` event per model call with input, output, cache read and cache write tokens (by
  lifetime), the model that answered and a price: 7 of 7 and 31 of 31 calls matched what the
  provider served, token for token. No event carries a window figure. A failed call sends
  `api_error` with its status code but no reset time (read in the bundle, not run). With a real
  sign-in its events also carry the account and organization ids (bundle), which would tell Silo
  which computers share one plan.
- **That report is the program's word, not a fact.** Killed hard, Claude Code lost the calls it
  had not exported: 2 of 10, 1 of 20 and 2 of 29 calls, 7% to 20% of output tokens. One variable
  in the program's environment, which an agent in the computer can set, turns it off: 0 of 7.
- **A reserve across several Macs needs no ledger.** Every Silo computer on the account reads the
  same figure on its own replies, so telling each other the newest figure changed nothing in any
  case simulated. What sets the overshoot is the number of calls under way and how late the
  provider counts. With 6 computers of 8 calls each and a delay of up to 120 s the week ended at
  79.5% for a reserve at 70%. Holding each call under way and counting each of Silo's own calls
  that ended within the delay, at the weight #355's E2 gives it, ended at 70.0 and 70.1% with one
  and three computers, and stopped the six at 62.6%, early rather than late.

## What each provider says, by what Silo can see

| | Anthropic, plan read | Anthropic, plan passed unread (#393) | OpenAI, ChatGPT sign-in | Google, Google sign-in |
|---|---|---|---|---|
| Unit of the window | a hidden weight; a point moved from $4.45 to $2.31 in a month (#355) | same | a hidden weight; per-conversation figure on a separate call | a model request |
| Windows | 5 hours, 7 days, 7 days per model family, extra usage | same | 5 hours (300 min), week (10080 min), credits; more families named `x-<limit>-…` | a day, per model, and a rate per minute |
| On every reply | `anthropic-ratelimit-unified-{5h,7d}-utilization` and `-reset`, `-status`, `-representative-claim`, `-overage-*`, `-surpassed-threshold` | nothing | `x-codex-primary-used-percent`, `-window-minutes`, `-reset-at`, same for `secondary`; `x-codex-active-limit`, credits; `codex.rate_limits` events on the socket | nothing; the program calls `retrieveUserQuota` itself |
| When the limit is reached | 429, `anthropic-ratelimit-unified-status: rejected`, the reset, which window | nothing; a closed or failed connection | 429 `{"error": {"type": "usage_limit_reached", "resets_at", "plan_type", "limit_window_minutes"}}` with the headers | 429 `RESOURCE_EXHAUSTED`, `QuotaFailure` with a `quotaId` (`PerDay`), `ErrorInfo` reason `QUOTA_EXHAUSTED`, `RetryInfo` |
| The computer's own use | exact in dollars and tokens, read | only as Claude Code reports it | exact in tokens, read; in points only from `thread_usage/query_v2` | exact, in requests |
| A reserve on the account (#376) | a rule | not possible | a rule | a rule, the figure up to 30 s old |
| A share per computer | a precaution, from #355's E2 | not possible; dollars as the program reports them | a precaution from E2, or a rule if Silo may call `query_v2` | a rule |

## Claude Code's own report against the bill

`prototype/run_otel.sh` runs Claude Code 2.1.332 with `CLAUDE_CODE_OAUTH_TOKEN` set to a made-up
`sk-ant-oat01-…` token, against #355's fake on `ANTHROPIC_BASE_URL`, with
`CLAUDE_CODE_ENABLE_TELEMETRY=1` and its OTLP export (HTTP, JSON) pointed at the collector. The
environment is cleared first; nothing reached a real provider.

| Run | Calls served | Calls reported | Output tokens served / reported | Results |
|---|---:|---:|---|---|
| 6 tool steps, the last two answered by another model | 7 | 7 | 1,050 / 1,050 | [1](prototype/results/1-whole-run.txt) |
| 30 tool steps | 31 | 31 | 9,300 / 9,300 | [2](prototype/results/2-long-run.txt) |
| 30 steps, `SIGKILL` after 5 s | 10 | 8 | 3,000 / 2,400 | [3](prototype/results/3-killed-after-5s.txt) |
| the same after 9 s | 20 | 19 | 6,000 / 5,700 | [3](prototype/results/3-killed-after-9s.txt) |
| the same after 13 s | 29 | 27 | 8,700 / 8,100 | [3](prototype/results/3-killed-after-13s.txt) |
| `CLAUDE_CODE_ENABLE_TELEMETRY=0` in its environment | 7 | 0 | 1,050 / 0 | [4](prototype/results/4-switched-off.txt) |

The event names the model that answered (`claude-opus-5` where `claude-opus-5-5` was asked); the
`claude_code.token.usage` and `cost.usage` metrics label all of it with the model asked. Of the
calls lost to a kill, one is the call under way, which the provider bills for what it generated.

## A reserve across Macs

`prototype/sim_reserve.py`: a week at 55%, a reserve at 70%, Silo computers each keeping a number
of calls under way (20 to 120 s each, about 0.09% of the week each), devices outside Silo spending
as much as Silo or nothing, and the provider counting each call within a random delay. The figure
is the true week, Silo's own spend only, when Silo's last call ended; "never reached" means Silo
stopped short of 70. Full table, with outside devices and a delay of 900 s, in
[`results/5-reserve-across-macs.txt`](prototype/results/5-reserve-across-macs.txt).

| Computers × calls | Delay | Each reads its own replies | The newest figure shared | Also holding each call under way | Also counting own calls the figure may not hold yet |
|---|---|---|---|---|---|
| 1 × 4 | 120 s | 70.8 | 70.8 | 70.5 | 70.0 |
| 3 × 4 | 120 s | 72.3 | 72.3 | 71.5 | 70.1 |
| 6 × 8 | 0 s | 75.1 | 75.1 | 70.9 | 70.9 |
| 6 × 8 | 120 s | 79.5 | 79.5 | 75.7 | never reached (stopped at 62.6) |
| 6 × 8, outside devices equal | 120 s | 79.1 | 79.1 | 77.3 | 73.9 |

The last column needs each call's weight in points, which is #355's E2 rate times its dollars.
With that rate half the truth the 6 × 8 case at 120 s ended at 71.2%; with it twice the truth, at
59.6% (the table's two last columns in the results file). Outside devices that spend in the
provider's delay are what no boundary can hold back.

## What it changes in the plan, by entry id

- **New open decision `program-reports-its-own-spend`**, placed after
  `program-that-cannot-be-read`: may Silo count spend that a program reports about itself, for
  traffic it passes unread? Recommended: yes, marked as the program's report, for Claude Code on a
  computer set for a plan, driving only a precaution budget in dollars.
- Notes added on `how-spend-is-shown` (what can be shown per computer, provider by provider),
  `plan-limit-reached` (each provider's error, and the computers where it cannot fire),
  `program-that-cannot-be-read` (the program's own report), `subscription-and-key-alike`
  (Google counts requests).
- For entries that live only on other drafts (to be added when they merge):
  - `budgets-in-dollars-or-a-window` (#376): the reserve is a rule only where the provider's
    figure is read; on a computer set for a Claude plan under `claude-key-or-plan` there is none.
    Google's share per computer is a fact, so for Google a share can be a rule. Across Macs the
    reserve needs nothing from #388's `pooled-budget-ledger`; a share does.
  - `claude-key-or-plan` (#393): "A budget cannot be set on a computer whose Claude traffic passes
    unread" holds for budgets read from traffic; `program-reports-its-own-spend` would allow a
    precaution there. The app's sentence for such a computer should also say that the plan's
    window is not shown.

No verdict, answer of a taken entry or list was changed.

## How to run it

On Linux with Python 3 and Claude Code 2.1.332 on the path (no root, no network beyond loopback):

```
research/2026-11-25-plan-windows/prototype/run_otel.sh research/2026-11-25-plan-windows/prototype/scenario.json /tmp/out
python3 research/2026-11-25-plan-windows/prototype/compare.py /tmp/out
KILL_AFTER=9 research/2026-11-25-plan-windows/prototype/run_otel.sh research/2026-11-25-plan-windows/prototype/scenario_long.json /tmp/out2
python3 research/2026-11-25-plan-windows/prototype/sim_reserve.py
```

## Not verified

- Every provider is a stand-in or a source read. No real plan was used: the provider's accounting
  delay, the weight of a call and whether Anthropic's figure is to the point or finer are not
  known (#355 found both reported). The reserve's overshoot rests on the delay.
- Codex's `thread_usage/query_v2` and Gemini's `retrieveUserQuota` were read in source, not called.
- Claude Code's OpenTelemetry through #378's closed network, and on the native build main gets.
- Whether Google counts a request the same whatever its size, and per model or for the family:
  the docs say "model requests", the bundle keeps one bucket per model.
- Codex's and Gemini CLI's own telemetry, which might report spend the same way.
