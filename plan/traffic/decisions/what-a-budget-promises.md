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
