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

## A hostile test of `budget-stops-what-it-meters`

Agent, 25 November 2026, from `research/2026-11-25-untrusted-parsers/` (#387). On #385's boundary,
a computer with a $50 budget had 10 model calls served, $240 at the host's counts, and 3 counted.
The other seven were sent to `/v1/messages` spelled as Anthropic's host also answers it
(`//v1/messages`, `/v1/./messages`, absolute form, and three more), and two calls went inside the
body of a request the boundary does not count, framed with a bare LF or a folded header line that
the hosts read and the boundary does not. A patch there makes the boundary read requests strictly
and refuse the rest with a 400; with it, the same test gives 3 served, 3 counted. The property
holds only if the boundary reads every request exactly as the host will.
