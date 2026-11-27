# What a budget on a computer or on a conversation promises

Proposed, as a property checked by a hostile test.

- The design record's sentence goes on: "A budget on a run is a precaution and promises nothing." That part is the answer of `budget-rule-or-precaution`, in force.
- Whether a budget denies or holds at each threshold: `budget-denies-or-holds`, open.
- Whether a program can get around a budget on a conversation by starting a new one: `what-a-conversation-budget-promises`.

## The choice

An agent's part, 22 November 2026. Main has a budget of 50 dollars. The spend reaches 50 while three
long answers are still coming back from the model.

The options:

- Running calls finish (recommended). The three answers finish, nothing new leaves and nothing is
  cut midway. It costs ending over 50 by what those three calls spend.
- Cut at the limit. The three calls are cut the moment the budget is reached. It costs paying for
  half answers, which the agent loses.
- Stop before the limit. Silo stops new calls early enough that the running ones cannot pass the
  budget, so you never go over. It costs an agent that stops before the amount you set.

Recommended: running calls finish. A cut answer is paid for and useless, and the overshoot is at
most the calls already running.

A second case: Claude Code in main switches to a model released yesterday, and Silo has no price for
it yet, so the budget of 50 dollars cannot count its calls.

The options:

- Deny that model's calls (recommended). They get an error that says why, and other models go on. It
  costs a wait for the price, or taking the budget off main to use that model.
- Let them pass uncounted, and say so. The agent works on. It costs the limit, since main can then
  spend far past 50 dollars with nothing to end it.

Recommended: deny that model's calls. A budget that a change of model gets around is not a limit.

The lesson from Guillotine below admits each call against what remains of the budget. Where that
keeps a call from starting before the budget is reached, it is the third option.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Count as it streams, settle at the end.** Admit work against the budget before it starts, count
  it as it runs, and settle on the final figure. Count a one-time adjustment once. A failed
  operation still reports what it spent. A model call is admitted against what remains of the
  budget, counted as it streams, and settled on the provider's final usage, so two calls at once
  cannot both spend the same remainder. A call that fails midway still counts what it cost.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/CLAUDE.md?plain=1#L669))

## Measured with Claude Code under a budget (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`. This refines the choice above with a working
boundary and real agent programs, where #351 had a simulator.

Main has a budget of $3 and Claude Code on Opus 5.5 starts 8 subagents at once. Each call reasons
for 2,000 to 12,000 tokens before it writes anything, and the reasoning streams as nothing the
boundary can read. Three runs per option, the same task each time:

| How a call is admitted | Spent, worst of 3 | Over the budget |
|---|---|---|
| Running calls finish, counting what is visible (the answer above) | $4.61 | 54% |
| The same, and a running call's output also counts by time at the model's speed | $4.05 | 35% |
| The same, and the call's own input must fit in what is left | $4.05 | 35% |
| Each running call holds its input and its model's typical output (90th percentile) | $2.80 | 7% to 9% under, every run |
| Each running call holds `max_tokens` | $0.53 | stopped after 3 calls: Claude Code asks for 128,000 |

Fable 5.2 under $6 gave 56%, 34%, 34%, 7% to 9% under, and 82% under.

What the table says:

- With visible output only, a third to a half of the overshoot came from calls that left after the
  budget was already spent, because the reasoning of the calls under way was not counted yet.
  Counting output by time while a call runs removes all of it. What is left is the calls under
  way at the limit, which the answer above accepts.
- That rest is large with subagents: a third of the budget here. A promise of "over by the calls
  under way" should say so in the app, with the number of calls running.
- The input check changed nothing on this night, since Claude Code's inputs are mostly cache
  reads. It is for one large call: a 2.4 MB tool result sent with $0.34 left of $1 ended the budget
  at $7.38 without it, and was refused with it.
- Holding `max_tokens` is not usable with Claude Code.

Recommended: keep "running calls finish", count a running call's output by time as well as by
what is visible, and keep the input check. If "never over" matters more than the last 7% to 9% of
the budget, hold the typical output instead: it was never over in six runs.

## One call at a time near the limit

Agent, 25 November 2026, from `research/2026-11-25-techniques-from-two-tools/`. An open-source
gateway lets one call per task be in flight by default, so it overshoots by one call at most. #374
found "calls under way finish" goes 35% to 56% over with 8 subagents, because 8 calls are under way
at the limit. A middle way: admit calls in parallel while what is left exceeds what the calls under
way could spend at their typical output, and one at a time after that. Not measured; #374's
`run/overshoot.sh` can measure it as a fifth policy.

## A budget reached in the middle of an answer (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-spend-budget-edges`. The same
Claude Code run as above (Opus 5.5, 8 subagents, $3), with a fourth way built: cut each call
under way once what is counted, reasoning included by time, reaches the budget. Three runs each:

| How a call is admitted | Spent, each run | Over the budget, worst | Paid for answers that were cut |
|---|---|---|---|
| Running calls finish, output counted by time | $3.71, $4.05, $3.92 | 35% | none |
| Each running call holds its typical output | $2.74, $2.78, $2.80 | 7% under | none |
| Cut at the budget | $3.02, $3.03, $2.99 | 1% | $0.52 to $0.92 a run |
| Hold the typical output, and cut at the budget | $2.75, $2.78, $2.80 | 7% under | none |

When a call was cut, Claude Code retried it within 40 ms, got the 400 refusal, and printed Silo's
sentence. A cut is paid for: here a sixth to a third of the budget went on answers nobody used.

One case the typical hold does not cover: a single call that reasons far longer than the model's
usual. With $0.68 left of $1, a call that reasoned 60,000 tokens was admitted under the typical
hold and ended the budget at $1.52. With the cut as well, it was stopped at $1.00, and the
program got an error event in the provider's own shape: "Silo: this computer's budget of $1.00
was reached while this answer was being written ($1.00 spent), so it was stopped."

So a fourth option for the choice above: hold the typical output, and cut only what passes the
budget anyway. It stopped 7% under the budget like the typical hold in every Claude Code run, and
held the one long call to the budget. It costs a cut answer in that rare case. Recommended, if
"never over" is wanted. If the overshoot is accepted, the recommendation above stands.

## The input check after a pause (25 November 2026)

An agent's part, 25 November 2026. The input check prices a call's input before it leaves. Priced
at the dearest rate, a conversation that reads 63,000 tokens from Anthropic's cache was held at
$0.51 for an input billed $0.013. Priced as "likely", every earlier token at the cache-read rate,
the estimate fell to $0.013, and after a pause longer than the cache's 5 minutes it stayed at
$0.013 while the provider billed $0.316 to write the cache again: 24 times short, the one call the
check is for. Built and measured: the boundary keeps when each conversation's cache was last
used, how much of its input the cache held, and its lifetime (5 minutes, or 1 hour when the reply
wrote a 1-hour cache), and prices that part as a write again once the lifetime has passed. Every
call of the run was then priced at or above its bill, and no more than the dearest price.

## The same rule over several Macs (25 November 2026, by an agent)

`research/2026-11-25-team-ledger/` ran this decision's rule on each Mac of a simulated team, with
the budget shared through a ledger, leases or gossip. The simulator gives #374's numbers on one
Mac ($3, 8 subagents: 50% over counting visible output, 35% counting output by time). Over a team
of five under $15, the calls under way at the limit are a smaller share of the budget: at most 27%
over counting visible output, 15% counting output by time, and from 0.4% over to 2.2% under
holding each call's typical output, through a ledger or leases. Twelve members under $36: at most 13% over counting output by time.
Where the count is kept (one boundary, a ledger 150 ms away, leases) moved these by a point or
two at most.

## Where the drafts of 25 November disagree (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

Three drafts recommend different policies for calls under way when the budget is reached.

- #374: running calls finish. Measured 35% to 56% over with Claude Code and 8 subagents.
- #378: one call at a time near the limit, from an open-source gateway. Not measured.
- #385: hold each call's typical output, and cut a call only when it passes the budget anyway. Measured 7% under in every Claude Code run, and it stopped the one call that reasoned 60,000 tokens at the budget.

#388 shows the same policies over a team of five Macs: the typical hold stays between 0.4% over and 2.2% under.

Recommended: #385's fourth option. It is the only measured policy that is never over, and the cut it costs happens only for a call far longer than the model's usual. The overshoot of "running calls finish" (35% at $3) is larger than anything the cut wastes.

## A cut at the budget over a team (25 November 2026)

An agent's part, from `research/2026-11-25-team-ledger/`.

With #385's cut, every way that shares one count stopped a team of five within 0.6% over $15,
and paid $1.40 to $1.90 a run for cut answers. Under leases, where each Mac cuts on its own slice,
`typical-cut` cut $0.09 a run against $1.85 for plain `cut`.

## A model with no price, a third option (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-model-prices`. The second case above
asks what a budget does with a model Silo has no price for. Two options were given: deny its
calls, or let them pass uncounted. The research measured a third.

- Count it at a bound. The call leaves, and is counted at the dearest price any model of the same
  provider and tier has in Silo's table, times a margin of 2. The app shows the call as counted at
  a bound until the model's price arrives. It costs a budget that runs out early while that lasts.

What was measured. Under a budget of $1, five calls to a model that was in no table were admitted
at a bound of $16 and $80 per million input and output tokens (twice Fable 5.2's price). The
stand-in billed them at $6 and $30. The budget stopped the agent after $0.46 had really been spent,
when the bound had counted $1.22. Under "deny", the same agent stopped at its first call.

How safe the bound is. Of 100 models Anthropic, OpenAI and Google released since January 2025, 7
cost more than the dearest model their provider already sold. The largest gap was 1.6 times
(OpenRouter's catalogue; models retired since are missing from it, so the gap can be larger). A
margin of 2 covered all of them. Pricing a new model like the previous model of its family would
have counted too little for 19 of 55, by up to 8 times.

Why it matters now. Agent programs change their default model when they update, often on the day
the model is released. With "deny", every budgeted computer stops on that day until Silo's table
has the price. LiteLLM's table had a price for a new model of these providers on the day it was
listed for half of them, and within 3 days for 9 in 10 (`where-prices-come-from` proposes a table
of Silo's own).

Recommended: count it at a bound. A budget still holds, since the bound is above what the provider
charges, and a new model does not stop work. This would replace the sentence of the answer on a
model Silo has no price for. The answer is left as it is for polarzero to choose.

## Where the drafts disagree, second pass (integration)

An agent's part, 25 November 2026, written while merging #390 and #392 to #396 onto #391 (`research/2026-11-25-integration-2/`).

The answer says a call to a model Silo has no price for does not leave while a budget covers it (from #373 and #374). #396 measured a third option: count it at twice the dearest price of the same provider and tier. A budget still held, and a new model did not stop every budgeted computer on the day it was released. Recommended: #396's option, with `where-prices-come-from` deciding where the table comes from.

## A budget across a restart

An agent's part, 25 November 2026, from [durability on a Mac](../../../research/2026-11-25-durability-on-a-mac/README.md). In #374's boundary the ledgers
live in memory. Main reaches its budget of 1 dollar after 23 calls; the boundary is killed, as a
crash or a power cut kills it, and starts again: the next call is admitted, since every budget
starts again at zero. The same happens when the boundary stops while a call streams. So
`budget-stops-what-it-meters` holds only until the boundary restarts.

A spend journal fixes it: each call that leaves, and each settlement, is written and flushed
(`F_FULLFSYNC` on a Mac) before the call goes on, and a start puts every ledger back. A call that
left and never settled counts at its reserve, the most it can cost. With it, both cases refuse the
call after the restart. It costs two flushes per model call; the Mac kit's test 50 measures them.

## A hostile test of `budget-stops-what-it-meters`

Agent, 25 November 2026, from `research/2026-11-25-untrusted-parsers/` (#401). On #385's boundary,
a computer with a $50 budget had 10 model calls served, $240 at the host's counts, and 3 counted.
The other seven were sent to `/v1/messages` spelled as Anthropic's host also answers it
(`//v1/messages`, `/v1/./messages`, absolute form, and three more), and two calls went inside the
body of a request the boundary does not count, framed with a bare LF or a folded header line that
the hosts read and the boundary does not. A patch there makes the boundary read requests strictly
and refuse the rest with a 400; with it, the same test gives 3 served, 3 counted. The property
holds only if the boundary reads every request exactly as the host will.

## Codex's WebSocket went past the budget (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-codex-read-in-full/`. #374 and #385
measured Codex over plain requests: their stand-in refused its WebSocket. OpenAI's hosts accept
it, and Codex then sends every model call as a `response.create` message on one socket. #385's
boundary read those messages but admitted none: under a budget of $0.0001 with calls of $0.00016,
Codex made three calls, none was counted, and its tool ran. With a ChatGPT sign-in and with an API
key alike. The same run with the WebSocket refused was counted and stopped at the second call.

A patch admits each `response.create` against the budgets before a byte of it leaves, settles it
on the reply's count, and refuses with an `error` event of status 400 (the one Codex shows once,
`how-a-hold-ends`). Then the warm-up was counted, the budget was reached, the next call was
refused, Codex stopped with Silo's sentence, and the tool never ran. A refused message sent with
a shared compression context ends the socket, since the host could not read the next ones. A
budget reached in the middle of a reply on the socket is not cut: `mid_stream` "cut" covers plain
requests only.
