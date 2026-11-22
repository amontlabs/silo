# What a budget on a computer or on a conversation promises

Proposed, as a property checked by a hostile test.

- The design record's sentence goes on: "A budget on a run is a precaution and promises nothing." That part is the answer of `budget-rule-or-precaution`, in force.
- Whether a budget denies or holds at each threshold: `budget-denies-or-holds`, open.
- Whether a program can get around a budget on a conversation by starting a new one: `recognising-a-conversation`.

## For the owner

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
