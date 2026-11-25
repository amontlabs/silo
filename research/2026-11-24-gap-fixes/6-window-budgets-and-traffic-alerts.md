# Budgets in a plan's window, and alerts on what model traffic shows

For polarzero. This answers gaps G7 and G8 of the real-pains research (draft PR #350), and challenge
C2 only where it bears on what Silo measures. How spend is shown on screen is another agent's slice.

The words of the plan are used as `plan/words.json` defines them. Two terms are needed here that
the plan does not have yet:

- **The plan's window**: the allowance a subscription gives for a stretch of time. Claude Pro and
  Max have a 5-hour window and a weekly one; ChatGPT plans with Codex have the same two. A window is
  counted for the whole account, across every device and surface signed in to it.
- **The week's figure** (or the 5-hour figure): the share of a window that the provider says is
  used, as it sends it in the headers of each reply, for example
  `anthropic-ratelimit-unified-7d-utilization: 0.37`.

## Summary

**G7, a budget counted in the plan's window.** Recommendation: two budgets, not one.

1. **A reserve**, a rule: "this computer's model calls stop once the plan's week is 70% used". It
   reads the week's figure from each reply, which is a fact, so it is exact up to the provider's
   rounding and accounting delay. In the simulator it stopped the computer with the true week at
   70.0 to 70.3%. It answers the case behind G7 directly: a background loop cannot take the part of
   the week you keep for yourself. Sure: high.
2. **A share**, a precaution: "this computer may use 30% of the week". Neither Anthropic nor OpenAI
   tells Silo, on a reply, how much of the window that one call used. The computer's share is an
   estimate, an opinion, so this budget is a precaution. The best of three estimators stopped the
   computer with its true share between 24% and 45% for a budget of 30%, when the figure comes as a
   whole percent and you work on your laptop at the same time. With a finer figure it stays within
   about one point. Sure: medium. It is worth building after the reserve, and only with its band
   shown. One estimator that looks obvious, summing the rises of the figure between the computer's
   own calls, is wrong by a factor of two to four as soon as another device is in use.

For Codex, OpenAI's backend does report a share per conversation (`thread_usage/query_v2`, with
`five_hour_limit_percent` and `weekly_limit_percent`), which would make a share budget a rule. Silo
can reach it only by calling that endpoint with your sign-in, which `logins` and
`model-traffic-read` rule out today. I recommend not changing that yet.

**G8, alerts on what the traffic shows.** Recommendation: rules that ship with Silo, switched on,
each one only an alert, in the pattern of `new-host-alert-is-a-rule`. Six triggers, five of them
facts:

| Alert | Triggered by (a fact unless said) | Kind | Sure |
|---|---|---|---|
| 1a. Another model answered | The reply's `model` differs from the request's `model` | rule | high |
| 1b. The program changed model in a conversation | A request continues a conversation's messages but asks another model | rule | high |
| 1c. A new model on this computer | A conversation starts on a model this computer has not asked for in 7 days | rule | medium |
| 2a. The cache lifetime changed | The `cache_control` lifetime in the program's requests changes, for example from `1h` to `5m` | rule | high |
| 2b. A live cache was not read | A prefix cached less than its lifetime ago is not read, and nothing explains it | rule | medium |
| 2c. Uncached input above a level | Over the last hour, more than half of the input was not read from the cache | rule | high |
| 3a. The week ahead of pace | The week's figure is 15 points ahead of an even pace through the week, while this computer spends | rule | high |
| 3b. This computer's share ahead of its pace | The estimated share rises faster than twice its pace over 6 hours | precaution | medium |

Real Claude Code 2.1.331, recorded against a fake API, showed three of these happening: it asks a
1-hour cache with a subscription and a 5-minute one with an API key, it drops to 5 minutes the
moment the account starts drawing on extra usage, and when another model answered it said nothing
and counted the cost at the price of the model it asked for. On five simulated weeks per fault, each
fault was caught by its alert whenever the computer made calls while it lasted, and the normal weeks
raised no false alarm except 3b (in two weeks of five).

**Challenges.** Four decisions should change, each on one case below: `recognising-a-conversation`
(and the word "conversation") ties a conversation to one model, so a model switch is invisible;
`budget-rule-or-precaution` calls every budget on a computer exact, which a share of a window is
not; `word-budget` and `how-spend-is-shown` count only in dollars; `what-a-budget-promises` bounds
the overshoot by the calls under way, and a window budget also overshoots by the provider's
accounting delay.

## What the providers report

Everything below was read from primary sources: the bundle of Claude Code 2.1.331 installed here
(`/opt/node22/lib/node_modules/@anthropic-ai/claude-code/cli.js`), the source of Codex at
`openai/codex` commit `a2c17b8` (24 November 2026), Anthropic's API reference, and replies recorded
from the real Claude Code against a fake API.

### Anthropic, on a Claude subscription

Each reply to a request signed in with a subscription carries these headers. The names and how
Claude Code reads them come from the function that parses them in the bundle.

| Header | Value | How Claude Code reads it |
|---|---|---|
| `anthropic-ratelimit-unified-status` | `allowed`, `allowed_warning` or `rejected` | the state of the account as a whole |
| `anthropic-ratelimit-unified-reset` | Unix seconds | when the limiting window resets |
| `anthropic-ratelimit-unified-representative-claim` | `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`, `overage` | which window is the one that limits |
| `anthropic-ratelimit-unified-5h-utilization`, `-7d-utilization` | a number, read with `Number()` and compared with 0.9, 0.75 and so on, so a fraction of 1 | the 5-hour and weekly figures |
| `anthropic-ratelimit-unified-5h-reset`, `-7d-reset` | Unix seconds | each window's reset |
| `anthropic-ratelimit-unified-{5h,7d,overage}-surpassed-threshold` | a fraction | the provider's own warning that a level was passed |
| `anthropic-ratelimit-unified-fallback` | `available` | whether a fallback model is offered |
| `anthropic-ratelimit-unified-overage-status`, `-overage-reset`, `-overage-disabled-reason` | for example `allowed`, Unix seconds, `out_of_credits` | whether calls past the window draw on extra usage, billed per token |

Three more facts from the bundle:

- **Precision.** Claude Code shows `Math.floor(utilization * 100)` percent. Values seen in the wild
  differ: a header dump of December 2025 shows `0.042598363636363636`
  ([gist](https://gist.github.com/andrew-kramer-inno/34f9303a5cc29a14af7c2e729b676fc9)), while an
  issue of October 2026 says "readings are integers"
  ([#104967](https://github.com/anthropics/claude-code/issues/104967)), and Claude Code's own
  `rate_limit_event` carries `0.42`. Silo cannot count on a precision. It learns the step from the
  values it sees, and the simulator below is run at both a whole point and a hundredth of a point.
- **Calls that only read the figure.** At start, a signed-in Claude Code sends a real model call
  with `max_tokens: 1` and the text `quota` to read the headers. `/usage` calls
  `GET {BASE_API_URL}/api/oauth/usage`, which returns `five_hour`, `seven_day`, `seven_day_sonnet`
  and `seven_day_opus`, each with `utilization` (shown as `Math.floor(utilization)%`, so a percent)
  and `resets_at`, and `extra_usage` with `monthly_limit` and `used_credits` in cents. All of it is
  for the account. No figure below the account exists in the bundle.
- **Claude Code's own warning is a pace check.** When the provider does not send a surpassed
  threshold, Claude Code warns by itself when the week is at least 25% used within its first 15%,
  50% within 35%, or 75% within 60%, and the 5-hour window at 90% within 72%. The recording below
  confirms it: the fake sent `allowed` with the week at 30% and six days to go, and Claude Code turned
  it into `allowed_warning`.

On the pay-per-token API, the headers are a different thing. `anthropic-ratelimit-requests-*`,
`-tokens-*`, `-input-tokens-*` and `-output-tokens-*` give a limit, a remainder rounded to the
thousand and a reset in RFC 3339, for requests and tokens per minute
([rate limits](https://platform.claude.com/docs/en/api/rate-limits)). They are throughput limits,
refilled continuously as a token bucket. They say nothing about money. A monthly spend limit exists
on the Console and answers with a 400 or a 429 once reached; nothing on a reply says how close it is.
So with an API key, Silo's dollar budget is the only limit with a figure before the end.

### OpenAI, on a ChatGPT sign-in (Codex)

From `codex-rs/codex-api/src/rate_limits.rs` and `codex-rs/backend-client/src/client/`:

| Where | What |
|---|---|
| Reply headers `x-codex-primary-used-percent`, `-primary-window-minutes`, `-primary-reset-at`, and the same with `secondary` | a percent as a float ("Can exceed 100", says the type), the window's length in minutes (300 and 10080), the reset in Unix seconds |
| `x-codex-credits-has-credits`, `-unlimited`, `-balance`; `x-codex-rate-limit-reached-type`; `x-codex-active-limit` | credits, which limit was reached, which is in force |
| `codex.rate_limits` events on the persistent socket | the same figures, as JSON |
| `GET /wham/usage` (`/api/codex/usage` on the API path) | the account's windows, `used_percent` and `limit_window_seconds` |
| `POST /wham/usage/thread_usage/query_v2` | **per conversation** (Codex says thread): `five_hour_limit_percent`, `weekly_limit_percent`, `balance_usage_credits`, grouped by model, effort and speed, with `data_status` (`available`, `partial`, `unavailable`), `usage_source` (`included_plan`, `credits`) and `data_as_of` |
| `OpenAI-Model` reply header | the model that served the request. Codex compares it with the model asked and warns (`maybe_warn_on_server_model_mismatch`, event `ModelReroute`) |

The per-conversation figure is used by Codex's own analytics view in its terminal interface. It is
the only per-conversation share of a window either provider gives.

### Recorded from the real Claude Code

`prototypes/window-budgets/fake_anthropic_usage.py` is a fake Messages API. It answers Claude Code
with a scripted loop of six `Bash` calls, computes `usage` from a small model of the prompt cache
driven by the `cache_control` markers in the real requests, returns another `model` from a given
step, and sends the subscription headers from a window in which another device also spends. A
virtual clock puts a 400-second gap before one step. Claude Code ran with a clean environment and a
made-up key or token, and reached nothing but the fake. Three runs, same scenario:

| Run | Cache lifetime asked | The call after a 400 s pause | What Claude Code said of the other model |
|---|---|---|---|
| API key (`ANTHROPIC_API_KEY`) | `5m` on all 3 markers | 13,301 tokens written again, 0 read | nothing; `modelUsage` lists only `claude-opus-5-5`, priced at its rates |
| subscription (`CLAUDE_CODE_OAUTH_TOKEN`) | `1h` on all 3 markers | 13,038 read, 295 written | the same |
| subscription, extra usage from step 3 | `1h` for steps 0 to 2, then `5m` | (the 1-hour entries written before still read) | not tested |

The third run is the case behind the TTL complaint, shown live. In the bundle, the 1-hour lifetime
is asked only when the sign-in is a subscription and `isUsingOverage` is false. The moment the reply
says calls draw on extra usage (`unified-status: rejected` with `overage-status: allowed`), the next
request asks for five minutes. Extra usage is billed per token, so from then on each pause of more
than five minutes costs a full rewrite of the conversation, in dollars.

## G7. A budget counted in the plan's window

### The case

A user's scheduled Claude Code jobs ran about 580 times a day at about 40k tokens each, and a weekly
allowance of 361.6M tokens was gone in 3.83 days
([#110395](https://github.com/anthropics/claude-code/issues/110395)). Nothing told the user, and
nothing kept a part of the week for their own work. A second case shows what the user cannot see: on
an Ultra plan, 84% of the week was counted as "Other" while the local logs explained 5%
([#111837](https://github.com/anthropics/claude-code/issues/111837)).

The subscriber does not pay per token. What they want is pace: a background loop must not eat the
week, and they must not be cut off mid-task.

### Two budgets, because the computer's share is not a fact

The week's figure is the account's. Silo sees it on every reply to the computer, but it includes
the laptop, the phone and claude.ai. Silo sees the computer's own calls exactly. What it cannot see
is how many points of the week one of those calls used: neither provider says so on the reply, and
the weights are hidden and move. One weekly point was worth $4.45 at API prices in September and
$2.31 in October on one account (#104967), and a headless `claude -p` call counts 1.86 times an
interactive one for the same tokens ([#111890](https://github.com/anthropics/claude-code/issues/111890)).

So "how much of the week did this computer use" is an opinion, formed from facts. "How much of the
week is used" is a fact. Two budgets follow, one on each.

#### The reserve (a rule)

You set a level on a window for a computer, or for all computers: "main's model calls stop once the
week is 70% used", or "once the 5-hour window is 90% used".

- **Trigger, a fact.** A reply to a call of that computer carries a week's figure at or above the
  level.
- **What it does.** The computer's next metered requests on that provider are denied, with an error
  that says which level was reached and when the window resets, as `budget-denies-or-holds` says for
  any budget, with its alert before the level (at 80% of it unless you set another: at 56% of the week
  for a reserve at 70%). A metered request is a model call on a host of that provider, signed in with
  that plan; a token count or a read of `/api/oauth/usage` is not one.
- **When calls pass again.** At the reset time the provider gave. The first reply after it carries
  the new figure; if it is still at the level (a reset that moved, as in
  [codex#7354](https://github.com/openai/codex/issues/7354)), the computer is denied again. This is
  how `budget-as-a-rate` already treats a rate: requests pass again once the window is under the
  limit.
- **What it costs.** It counts your own use too. That is what a reserve means: with the laptop at
  60% by Wednesday, the computer stops at 70% even if it used 2% itself.
- **Overshoot.** The calls under way finish (`what-a-budget-promises`), and the provider counts with
  a delay, so the figure that triggers the rule can lag what is spent. In the simulator, with a
  whole-point figure and an accounting delay up to 120 seconds, the true week was at 70.0 to 70.1%
  when the rule fired; with up to 900 seconds of delay, at most 70.3%.

#### The share (a precaution)

You set a share of a window on a computer: "main may use 30% of the week". The computer's share is
estimated by a classifier in the plan's sense: whatever gives an opinion, and here it is Silo's own
window estimator. The budget is a precaution. It may deny the computer's next requests, which is
protective (`what-a-precaution-may-do` lists deny), and it says it rests on an estimate
(`what-a-precaution-says`).

**The estimator that works (E2 below).**

1. For each reply to the computer on a subscription host, the record holds the window figures and
   resets (facts), and the call's price at API prices (`how-spend-is-shown`), computed from its token
   counts.
2. Silo cuts the computer's calls into spans: a run of calls with no pause over 2 hours, closed once
   the figure has moved by 6 of its steps (6 points when the figure is a whole percent), so the
   rounding is at most a sixth of the rise.
3. For each span, the ratio is the rise of the figure divided by the computer's own priced spend in
   it. Other devices can only add to a rise, never take from it, so the clean spans are the ones with
   the lowest ratios.
4. The rate `k` is the 20th percentile of the ratios over the last 48 hours, weighted by own spend.
   Weighting matters: a day of laptop work makes many short, high-ratio spans, and counted one by
   one they outvote the few long, clean spans of the night. Unweighted, the estimate was five times
   too high mid-week in the simulator.
5. The computer's share of the window is `k` times its priced spend since the window's reset. Its
   band is the 10th and 50th percentiles of the ratios times the same spend.
6. Until there are enough spans, Silo uses the last week's `k` for that computer, or that of another
   computer on the same account. With none, the share is unknown, and the budget counts the account's
   whole rise while the computer was calling (E1, which over-counts and so stops early), and says so.

This rate also absorbs what Silo cannot know, because it is learned from the computer's own calls:
if they are all headless and count 1.86 times, `k` comes out 1.86 times higher.

**Enforced means:** once the estimate reaches the share, the computer's next metered requests are
denied, with an error that says the budget rests on an estimate and when the window resets. The
calls under way finish. Requests pass again at the reset, when the window's spend starts at zero.

**Three estimators, measured.** `prototypes/window-budgets/sim.py` simulates one week of one
account: the computer runs a scheduled loop day and night (the #110395 shape) and agent sessions in
working hours; the laptop works in working hours, at an intensity relative to the computer. The
provider counts in hidden weights that differ from API prices per kind of token and count headless
calls 1.86 times, rounds the figure down to its step and accounts each call within a random delay.
Silo's estimators see only the computer's calls and the figure on their replies.

- **E1, rises between own calls**: the sum of the figure's rises between consecutive replies to the
  computer less than 10 minutes apart. It is the obvious one.
- **E2, the spend-weighted low ratio**, as above.
- **E3, per kind of token**: one non-negative weight per kind (input, output, cache read, 5-minute
  and 1-hour writes), fitted over the spans with rises above the fit counting for less.
- **E4, the provider's own per-conversation figure**, five minutes late. Codex's backend has it;
  Anthropic's does not. It is the reference for a share budget that would be a rule.

The table gives the computer's true share of the week at the moment each estimate reached 30%:
median, and lowest and highest over six seeds. Under 30 the budget stopped the computer early;
over 30, late.

| Laptop's use, against the computer's | E1 | E2 | E3 | E4 |
|---|---|---|---|---|
| **Figure to a whole point** | | | | |
| none | 30.0 | 34.1 (32.4 to 37.3) | 33.8 (30.9 to 35.0) | 30.1 |
| half | 16.7 (15.9 to 19.5) | 31.0 (30.5 to 32.9) | 26.4 (21.7 to 30.2) | 30.0 |
| equal | 11.7 (10.6 to 15.0) | 29.8 (23.6 to 44.8) | 21.3 (17.8 to 23.6) | 30.0 |
| twice | 7.9 (6.2 to 10.0) | 31.6 (27.2 to 39.6) | 14.2 (10.8 to 20.2) | 29.9 |
| **Figure to a hundredth of a point** | | | | |
| none | 30.0 | 31.0 (30.8 to 31.3) | 30.6 (30.5 to 30.7) | 30.1 |
| half | 16.7 (15.9 to 19.5) | 30.8 (30.6 to 31.1) | 26.9 (24.4 to 28.3) | 30.0 |
| equal | 11.7 (10.6 to 15.0) | 30.9 (30.5 to 31.2) | 23.9 (22.6 to 26.1) | 30.0 |
| twice | 7.9 (6.2 to 10.0) | 30.3 (27.2 to 30.8) | 21.9 (21.2 to 23.5) | 29.9 |

With an accounting delay of up to 120 seconds the figures move by at most 0.2. A lowest value of 27.2
in the last row is a week in which the computer never reached 30% at all: the share at the week's
end.

What this shows:

- **E1 is wrong as soon as anything else spends.** With the laptop at half the computer's use, it
  stopped the computer at 17% of the week; at twice, at 8%. It is right only when the computer is
  alone on the account. It is still the right fallback when nothing better is known, because it can
  only over-count.
- **E2 is the one to build.** With a figure to a hundredth of a point it stays within about a point
  of the budget. With a whole-point figure its median stays near 30%, but single weeks land between
  24% and 45%: a whole point of rounding over spans of a few points is coarse. A budget of 30% on a
  whole-point figure is a budget of "about a quarter to a half of the week".
- **E3 is worse than E2**, though it models more. The kinds of token are spent together, so the fit
  cannot tell them apart, and it puts the laptop's daytime use on the kinds the computer spends by
  day. Not worth building.
- **E4 is exact to its step.** A provider figure per conversation would make the share budget a rule.
- **A mid-week cut of the plan** (17% on day 3, as in the HN case of August 2026) moves `k`, and
  E2 learns the new one only over its 48-hour look-back. Until then it counts at the old rate, so it
  stops late: at 36.8% (34.5 to 37.9) with the laptop at half, 33.6% (28.4 to 50.3) at equal use. A
  cut shows as a drop in `k` across all computers of the account at once; Silo can then drop the old
  spans instead of waiting.

### How it fits the plan

| Entry | What this design does with it |
|---|---|
| `word-budget` | Changes it: a budget is a limit on what may be spent, counted in dollars or in a share of a plan's window. See the challenge below. |
| `budget-rule-or-precaution` (taken) | Challenges it: a budget in dollars is exact, a share of a window is not. The reserve is the exact form. |
| `budget-on-a-computer` | The reserve and the share are budgets on a computer, set beside a dollar budget. |
| `budgets-together` | Unchanged: every budget that covers a request applies. A computer can have $50, a share of 30% and a reserve at 70%. |
| `budget-of-a-fork` | Unchanged: a fork spends from its parent's share unless you give it its own. A reserve is a level of the account, so a fork has the same one. |
| `budget-as-a-rate` | The reserve and the share pass again at the window's reset, as a rate passes again once the last hour is under its limit. |
| `budgets-on-rates` | Answers its "to find out": a call is billed outside the plan when the reply says `unified-status: rejected` with `overage-status: allowed` (Claude), or when credits are drawn (`x-codex-credits-*`, `usage_source: credits`, Codex). From that reply on, the dollar budget counts it and the window budgets do not. |
| `what-a-budget-promises` | Holds with one change: the overshoot also includes the provider's accounting delay. |
| `budget-denies-or-holds` | Unchanged: alert at 80% of the budget, deny at the budget, never hold. |
| `what-traffic-gives`, `model-calls-in-the-record` | Already record the figures and resets as facts. Add the window's step (precision) Silo has learned, and whether the call drew on extra usage. |
| `plan-limit-reached` | Unchanged; the reserve is the same mechanism set before the limit instead of at it. |
| `spend-split-and-forecast` | The forecast of when the week runs out uses the account's figure (a fact) and its slope (an opinion). |
| `program-account-checked` | Unchanged. Claude Code's own account is not a source for any of this. |
| `logins`, `model-traffic-read` | Respected: Silo calls nothing with your sign-in. |

Each action, filed:

| Action | Trigger | Kind |
|---|---|---|
| Deny the computer's calls at the reserve | the week's (or 5-hour) figure on a reply, a fact | rule; deny is protective |
| Alert at 80% of the reserve | the same fact | rule |
| Deny the computer's calls at its share | the estimated share, an opinion | precaution; deny is protective |
| Alert at 80% of the share | the same opinion | precaution |
| Let calls pass again | the reset time the provider gave, a fact | rule; this is the window ending, as for `budget-as-a-rate` |
| Show what was spent outside Silo's computers | the figure's rise across times when no computer had a call in flight, a fact; the computer's own calls in those rises, an estimate | fact, shown beside an opinion |

### What others do

- **Claude Code** warns on the account's pace, as above, and shows the account's figures in
  `/usage`. Nothing below the account
  ([#29721](https://github.com/anthropics/claude-code/issues/29721), closed as stale, asked for
  per-session shares).
- **Codex** shows per-conversation shares from its backend in its analytics view. It does not limit
  anything with them.
- **CodexBar** (23.3k stars) shows both providers' windows and a burndown "from recorded snapshots",
  read through OAuth APIs, cookies or the CLI
  ([README](https://github.com/steipete/CodexBar)). It shows; it enforces nothing.
- **ccstatusline** (13.7k stars) shows the 5-hour block forecast and weekly use from the status line
  data Claude Code gives it. **Claude-Code-Usage-Monitor** (8.8k) predicts from local logs when you hit
  the cap. Both read the agent's own logs, which miss calls from child processes.
- **LiteLLM** (63.8k) gives each key a budget per day or week, in dollars, on API keys only.
- **A reverse-engineering attempt** logged 25,477 calls through a proxy and fitted the dollar value of
  a window to the rises of the figure over 63 five-hour resets
  ([Claude Code Camp](https://www.claudecodecamp.com/p/i-tried-to-reverse-engineer-claude-code-s-usage-limits)):
  a median of $164 per 5-hour window, from $3 to $705. That spread is the size of the problem E2
  faces, and why the rate is learned per computer and per week.

What differs: Silo enforces, outside the computer, on every call the computer makes, whichever
process makes it. None of these tools stops anything, and the local-log ones do not see a script the
agent wrote.

### Bad paths

| What happens | Outcome |
|---|---|
| The agent finds an API key in `.env` and calls the API with it ([OpenAI forum, $453 in a day](https://community.openai.com/t/my-codex-session-hit-its-subscription-limit-then-wrote-its-own-metered-api-runner-453-in-one-day/1389087)) | Those calls are not on the plan: no window figure on them. The dollar budget on the computer counts them (`budgets-together`). A computer with only a window budget has no limit on them, and the app should say so when the budget is set. |
| The agent signs in to a second subscription inside the computer | The figures on its replies jump between two accounts. Silo keys each figure by the window's reset time, which differs between accounts, and runs one estimate per key. A reserve applies to each account seen. |
| The agent changes its mix to what the provider weights more than Silo's prices (say, many cache reads) | E2's rate follows within its 48-hour look-back; until then the share is under-counted. The reserve is not affected: it reads the provider's own figure. |
| The agent sends calls through a host Silo does not read | `program-that-cannot-be-read` applies: no budget covers them, and the app says so. |
| The provider stops sending the headers, or renames them | No figure: the reserve cannot fire and says so in the app, as a budget that cannot count. The share falls back to the last `k`. The live checks of `reading-each-agent-program` should include the headers. |
| The figure is a whole point and the computer spends little | Spans close slowly; with a light computer, a span can take a day. The share stays on last week's `k`, shown as such. |
| The provider counts late | Each rise lands a little after its call. The reserve overshoots by what is counted late (0.3 points of the week at most, at a 15-minute delay, in the simulator). |
| The provider's meter is wrong, as when Sonnet was counted as Opus ([status incident](https://status.claude.com/incidents/4w17zgs3tflt)) | The reserve follows the provider's figure, wrong or not. The record shows Silo's own count beside it, which is the evidence for support. |
| You are at your laptop and the reserve stops the computer | That is its purpose. You raise it from the alert, as for any budget. |

### What Silo measures (C2, as it bears on mechanism)

How it is shown is another thread's. What Silo has to compute and keep:

- Per call, as facts: the window figures and resets on its reply, whether it drew on extra usage,
  its price at API prices.
- Per window and account: the figure over time, the step learned from the values, the reset.
- Per computer and window: the share estimate with its band and the rate `k` it used (an opinion,
  kept apart from the facts).
- The rise of the week while none of Silo's computers had a call in flight: a fact, except for the
  first call after each pause, which is the computer's own and has to be estimated. In the simulated
  weeks, with the laptop at the computer's level of use, the week rose 23.0 points across the
  computer's pauses, of which 2.9 were the computer's own first calls, against 22.1 points really
  used by the laptop. This is the measure the #111837 user needed: "this much of your week was not
  spent by any of your computers".

### Recommendation for the plan

- `word-budget`, a new answer: "A limit on what a computer, a conversation or a run may spend on
  models, counted in dollars or in a share of a plan's window."
- A new decision in `traffic`, `budget-in-a-window`. Asks: "Can a budget be counted in a plan's
  window?" Answer: "Yes, in two ways. A reserve stops a computer's calls on a plan once the
  provider says a window is used to a level you set; it reads the provider's figure, so it is a
  rule. A share limits the part of a window one computer uses; the providers report windows only for
  the whole account, so the share is estimated and this budget is a precaution. Both let calls pass
  again when the window resets."
- `budget-rule-or-precaution`: add "A budget counted in a share of a plan's window is a precaution,
  unless the provider reports the share for what the budget covers." This is a change to a taken
  decision, so it is polarzero's.
- `what-a-budget-promises`: "…only the calls already under way, and what the provider counts late,
  may end past it." with a measured bound per provider.
- `budgets-on-rates`: replace its "to find out" with the headers that say a call is billed outside
  the plan, from the table above.
- `budget-on-a-computer`: name the reserve as the first window budget to build, the share after it.

## G8. Alerts on what the traffic shows going wrong

All three are rules that ship with Silo, switched on, that only alert (`rule-that-only-alerts`).
Each fires when a condition starts and not again while it lasts, and alerts of one kind close
together reach you as one with a count (`alert-only-on-a-change`). Each opens on the call that
triggered it (`where-an-alert-opens`).

`prototypes/window-budgets/detectors.py` implements them over a log of model calls in the shape the
fake API writes. `tracegen.py` makes up weeks of calls with one fault each, at a known time, and
counts true and false alerts. The fault-free weeks have the shapes that cause false alarms: a nightly
job on Sonnet next to daytime sessions on Opus, compactions, pauses of 5 to 25 minutes.

| Week | Alert that fired | Weeks with it (of 5) | True | Time from the fault to the first alert (median) |
|---|---|---|---|---|
| normal | 3b only | 2 | 0 of 2 | (false alarms) |
| the provider answers with Opus 5 for two hours | 1a | 3 | 4 of 4 | 24 min |
| the program falls back from Opus to Sonnet | 1b | 5 | 30 of 30 | 134 min |
| the cache lifetime cut to 5 minutes | 2a | 5 | 5 of 5 | 60 min (the first call after the cut) |
| cache markers stop moving | 2c | 5 | 8 of 8 | 701 min (the first busy hour) |
| a runaway loop from 02:00 | 3b | 5 | 5 of 5 | 451 min |
| a runaway loop from 02:00 | 3a | 5 | 5 of 5 | 3,005 min |

In two of the five provider weeks the computer made no call during the two hours, so there was
nothing to see.

### 1. The model that answered differs from the model asked

**The cases.** Opus fell back to Sonnet after a cap and the work broke
([#3434](https://github.com/anthropics/claude-code/issues/3434): "I would prefer Claude to stop
responding when Opus is unavailable"). Sessions moved from Opus 5.5 to Opus 5 with no notice
([#111358](https://github.com/anthropics/claude-code/issues/111358)), and from Fable 5.2 to Haiku 4.5
after a sign-in ([#109628](https://github.com/anthropics/claude-code/issues/109628)). People find out
from the model's answer about itself.

**Where each model is.** Two different events hide under "silent switch", and they are in different
places:

- *The provider answered with another model.* The request names one model, the reply's `model`
  (in `message_start` on a stream) names another. With Anthropic's server-side fallbacks, the request
  carries `fallbacks`, the reply a `fallback` content block at each switch, and `usage.iterations` an
  entry of type `fallback_message`; a conversation that fell back is then served by the fallback
  model for about an hour ("sticky routing"). With OpenAI, the reply header `OpenAI-Model`.
- *The program asked for another model.* Claude Code's own fallback: after 3 overloaded errors in a
  row (HTTP 529), with `--fallback-model` set and an API key on an Opus 4 model (or
  `FALLBACK_FOR_ALL_PRIMARY_MODELS`), it changes its main model and sends the same messages to the
  other one. The reply matches the request; the requests change.

**How Silo knows what you set.** What you set inside the computer, `~/.claude/settings.json`,
`--model` on the command line, `ANTHROPIC_MODEL`, Codex's `config.toml`, is a report: the agent can
change any of it. Silo has three facts instead: the model of the conversation's first request, the
models this computer asked for in the last week, and a model you set for the computer in Silo
itself, outside it.

**Triggers.**

- **1a.** A reply's `model` is not the request's `model`, compared without a date suffix (old
  models answer `claude-sonnet-4-5-20250929` to `claude-sonnet-4-5`). The alert says whether the
  request allowed a fallback (`fallbacks` in it, or a `fallback_message` in the reply). Silo prices
  the call at the model that answered, which `what-traffic-gives` already records as a fact.
- **1b.** A request continues a conversation's messages (`recognising-a-conversation`'s prefix test)
  and asks another model than that conversation did.
- **1c.** A conversation starts on a model this computer has not asked for in 7 days, after its
  first day.
- **1d, optional.** A request asks another model than the one you set for the computer in Silo. As
  a rule, it can also deny such requests: the #3434 user's "stop responding when Opus is
  unavailable", enforced outside the computer.

**False alarms.** 1a: none when the names are normalised. 1b fires when you change `/model` in the
middle of a conversation, once; that is a change you made, and the alert costs a glance. 1c fires
the first time you try a new model; the first version, which compared each conversation with the one
before, fired 50 times in a normal week because the nightly job is on Sonnet and the sessions on
Opus. Side calls (titles, summaries on a small model) start their own conversations and are on
models the computer already uses, so 1c stays quiet on them.

**What the recording showed.** When the fake answered `claude-opus-5` to `claude-opus-5-5`, Claude
Code printed nothing and its own account, `modelUsage` and `total_cost_usd` in its final result,
listed every call under `claude-opus-5-5` at that model's prices ($4 and $20 per million, where
Opus 5 is $5 and $25). Codex, by contrast, warns.

### 2. The share of uncached input above a level

**The cases.** The cache lifetime silently cut from one hour to five minutes, priced by one user at
about $2,500 over his logs ([#46829](https://github.com/anthropics/claude-code/issues/46829), HN
552 points). A Bedrock bill of $37,901.73 in which about 6.47B uncached input tokens cost $35.6k and
cache reads $918 ([HN](https://news.ycombinator.com/item?id=47933355)). Cline's cache markers that
never moved forward in tool loops ([cline#15307](https://github.com/cline/cline/issues/15307)).

**The facts.** Each reply's `usage` holds `input_tokens` (after the last marker),
`cache_creation_input_tokens` with its split `cache_creation.ephemeral_5m_input_tokens` and
`ephemeral_1h_input_tokens`, and `cache_read_input_tokens`; their sum is the whole prompt. A write
costs 1.25 times the input price for five minutes and twice for an hour; a read costs 0.1 times, and
0.05 times on Opus 5.5 and Sonnet 5.5. Each request's `cache_control` markers say the lifetime asked
(`{"type": "ephemeral"}` for five minutes, `"ttl": "1h"` for an hour). Codex's replies give
`input_tokens_details.cached_tokens`, and its requests a `prompt_cache_key` and no lifetime.

**Triggers.**

- **2a.** The lifetime in the program's requests changes, for example every marker from `1h` to
  `5m`. When the previous reply said calls now draw on extra usage, the alert says so: that is the
  case the recording showed. The alert can carry what each lifetime would have cost over the last
  day, worked out from the pauses actually seen. That figure is an estimate and is shown as one; it
  can go either way. In the simulated week the cut made the week 6% cheaper ($46.00 against $48.84),
  because writes cost less and pauses over five minutes were rare; in the #46829 user's logs it cost
  17% more. So the alert is about the change, with its price.
- **2b.** A request in a conversation, less than the lifetime after the previous one, reads less than
  half of what that one had cached, with at least 4,096 tokens cached. It is not counted when the
  prompt shrank by more than 30% (a compaction or an edited history) or when the model changed (each
  model has its own cache, and 1 already alerts).
- **2c.** Over the last hour of a computer, more than half of the input tokens were not read from the
  cache, with at least a million input tokens in that hour.

**False alarms, measured.** On five normal weeks, 2c at 50% raised nothing, and at 30% nothing; at
20% it raised alerts in one week, and at 10% in four, on the nightly job, whose short new
conversations write their 40k context each time. The cut lifetime raised 2c in one week of five at
30%, three at 20%: a cut lifetime does not show as a high uncached share (the week's uncached share
went from 9.5% to 11.6%), which is why 2a exists. Cache markers that stop moving raised 2c in every week,
in the first busy hour after the fault (a median of 701 minutes after it started at midnight, so
at the start of the working day).

### 3. A computer's window share moving faster than a pace

**The case.** #110395 again: the week gone in 3.83 days, with nothing to warn the user on the first
night.

**Triggers.**

- **3a, a rule.** The week's figure is at least 15 points ahead of an even pace through the week (for
  example 45% used when 30% of the week is gone), and the computer had a call in the last hour. It is
  the check Claude Code makes for itself, on the account, with one level.
- **3b, a precaution.** The computer's estimated share (E2) rose more than twice its own pace over
  the last 6 hours. Its pace is its share budget spread over the week: a share of 30% is 4.3 points a
  day. With no share budget, the pace is the computer's own median of the last two weeks.

**Measured.** With a runaway loop from 02:00 on day 2, 3b alerted in every week, a median of 7.5 hours
after the loop started; 3a in every week too, but only after a median of 50 hours, once the whole
account was ahead. 3b also alerted in two of five normal weeks, on busy working days; 3a in none. With
#110395's speed (the week gone in 3.83 days), 3b leaves more than three days.

### Bad paths of the alerts

| What happens | Outcome |
|---|---|
| The agent rewrites its program's settings to another model | 1b or 1c fires, from the requests; the settings file is never read. |
| The agent sends calls with no cache markers, to look cheap | 2c fires on the uncached share. |
| The agent spreads a runaway across forks | Each fork's calls count in its parent's share (`budget-of-a-fork`), and 3a reads the account. |
| A program sends a lifetime per request that alternates (some calls `1h`, some `5m`) | The prototype compares each main request with the one before, so it would fire on each change. Claude Code asks one lifetime for all markers of a request, but a side call on another query source may ask `5m` while the main loop asks `1h`; the prototype only reads main requests. The product should compare the set of lifetimes over the last 20 requests. |
| Many conversations switch at once (a provider-wide fallback) | One alert with a count, as `alert-only-on-a-change` says. |
| A provider names a model differently in the reply (a region prefix on Bedrock, `us.anthropic.…`) | Normalised per provider adapter (`adapters-follow-providers`); a name Silo cannot normalise alerts once, and the adapter is fixed. |

### Recommendation for the plan

- A new decision in `traffic`, `alerts-on-model-traffic`. Asks: "What does Silo alert on in model
  traffic by itself?" Answer: "Rules that ship with Silo, switched on, alert you when the model that
  answered is not the one asked, when a program asks another model in a conversation, or a new model
  on a computer; when the cache lifetime a program asks changes, a live cache is not read, or more than
  half the input of the last hour was not read from the cache; and when a plan's week is far ahead of
  an even pace while the computer spends. A precaution alerts you when a computer's estimated share of
  a window rises faster than its pace. Each only alerts, and you can change or switch off each."
- `model-calls-in-the-record`: add the request's model next to the model that answered, the cache
  lifetime asked, and the fallback markers (`fallbacks`, `fallback` blocks, `usage.iterations`).
- A feature in `traffic`, `expected-model-rule`: you set the model a computer may use, and a rule
  denies requests for any other. Name: "The model a computer uses". It answers #3434.

## Challenges

**`recognising-a-conversation`, and the word "conversation".** The word says "a sequence of turns
with one model", and the decision says a request continues a conversation "when it goes to the same
model". Case: Claude Code with `--fallback-model claude-sonnet-5-5` gets three overloaded errors and
sends the same 40 messages to Sonnet. Under the decision, that request starts a new conversation. A
budget on the conversation starts again at zero (its stated worst case), and the switch itself
appears as one conversation ending and another beginning, not as a switch. I would drop "with one
model": a conversation is a sequence of turns, each turn's model is a fact of that turn, and 1b reads
a change of it.

**`budget-rule-or-precaution` (taken).** "A budget on a computer … [is a rule]: the boundary sees
both, so they are exact." True of dollars. Case: main has a budget of 30% of the week; the boundary
sees main's calls exactly but not how many points of the week each used, so the budget rests on an
estimate. The sentence to add is in the G7 recommendation. The reserve keeps an exact window budget
available.

**`word-budget` and `how-spend-is-shown`.** Both count in dollars only. Case: a Max subscriber who
wants main to leave him 30% of the week has no dollar figure that means that, since one point of the
week moved from $4.45 to $2.31 in a month (#104967). The sentence for `word-budget` is above. For
`how-spend-is-shown`, which another thread is designing, only the measurement matters here: a share
of a window is a unit a budget is set in.

**`what-a-budget-promises`.** "Only the calls already under way may end past it." Case: the reserve
fires on the figure of the reply to the call that crossed 70%; a provider that counts with a delay of
a few minutes has not yet counted the calls before it. The overshoot is the calls under way plus what
the provider counts late. The property should say so, with a bound measured per provider.

**Not a challenge, a choice to keep: `logins`.** Codex's per-conversation figure would make the share
a rule for Codex. Getting it means Silo sending its own request with your sign-in, which `logins`
("Silo holds no login of a subscription") and `model-traffic-read` ("the sign-in token is forwarded
and written nowhere") exclude. When Codex itself calls that endpoint, for its analytics view, Silo
can record the answer as a fact; it should not call it. If polarzero wants the exact share for Codex,
that is the decision to revisit, with this as its case.

## Open questions

- The real precision and accounting delay of the figures, today. A short capture of a real
  subscription through Silo's boundary, comparing consecutive figures, would settle both. My own
  numbers come from the source and from public dumps, not from a live account.
- Whether Anthropic will report a share per call. #104967 asks for it, and the server has the number.
  It would turn the share into a rule and E2 into a fallback.
- What Claude Code does when Silo denies a call with a 429 and the subscription headers saying
  `rejected`. Its own handling would show its limit message and stop retrying; a plain 429 makes it
  retry with backoff. Which is right is for `refusals-that-say-so`, per program, by a live check like
  those of `denial-per-program`.
- The account key. Silo keys a figure by its window's reset time. Two accounts with the same reset
  second would merge; rare, and a change of the figure by more than its rise would show it.

## Run it

From `research/2026-11-24-gap-fixes/prototypes/window-budgets/`:

- `python3 sim.py`: the estimator and reserve tables (about two minutes; `--quick` for fewer seeds).
- `python3 tracegen.py`: the alerts on simulated weeks with one fault each, and the reconciliation.
- `python3 detectors.py recorded_calls.jsonl --small`: the alerts on the three recorded Claude Code
  runs (`--small` lowers 2c's hourly minimum to fit a short run, so 2c fires on the first, cold call).
- `ANTHROPIC_API_KEY_FAKE=1 ./run_cc.sh scenario_cc.json <out>` and
  `./run_cc.sh scenario_overage.json <out> CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-fake-not-a-token`:
  record Claude Code again against the fake API. They need `claude` on the path, and reach nothing
  outside this machine.
