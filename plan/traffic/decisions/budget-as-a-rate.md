# Can a budget be a rate

## The choice

An agent's part, 22 November 2026. Main has a budget of 50 dollars. At 3 in the morning an agent
loops on a failing test and spends 5 dollars every 20 minutes.

The options:

- A total and a rate (recommended). You can also give main 5 dollars in any hour. The loop gets no
  more model calls within its first hour, and new model calls pass again once main's last hour is
  under 5 dollars. It costs one more kind of budget to understand, and calls that pass again without
  you may surprise you.
- A total only. It costs the whole 50 dollars before anything ends the loop, and no way to say "at
  most 5 dollars an hour, 50 in all".

Recommended: a total and a rate. A rate catches a runaway early, and Silo knows when each call
happens as exactly as what it costs.

## Notes

polarzero put budgets on rates on the roadmap on 21 November 2026: spend per hour, new conversations
per minute. The feature is `budgets-on-rates`. polarzero did not answer this question.

## Windows on a clock that does not jump

Agent, 25 November 2026, from `research/2026-11-25-techniques-from-two-tools/`. An open-source
gateway with a spend window over the last 24 hours found that a wall clock moved forward makes all
spend look old, so the window admits everything, and that pruning old entries by the same clock
deletes them for good. It now measures windows on a monotonic clock. On a Mac that sleeps, the
window is measured on `mach_continuous_time`, which counts sleep, and the record is never pruned
by a wall-clock cutoff. The same gateway refuses a call when its ledger cannot be read at all: for
a budget, "I don't know" means no.
