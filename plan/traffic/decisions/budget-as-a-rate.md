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

## What the program does when a rate refuses (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`.

Main has a rate of $0.60 in any 30 seconds, and Claude Code and Codex each work on a 30-step
task. Refused with the 400 that `budget-denies-or-holds` proposes, both programs stopped the task
on the first refusal, after 9 calls each, and printed "Model calls pass again in 13 s". Nothing
started them again. A rate refused this way is a second total, not a rate.

Refused with 429 in the provider's own rate-limit shape and `retry-after` set to the seconds until
the window allows a call again, both waited and went on. Codex finished all 30 steps in 148 s.
Claude Code waited 13 times and kept working until the run's limit. The worst 30 seconds spent
$0.85 against $0.60, from calls under way, as with any budget.

So the choice is now between:

- 429 with `retry-after` for a rate (recommended). The agent slows to the rate and goes on, which is
  what a rate is for. It costs a program that may wait in silence: Claude Code shows a retry, Codex
  shows nothing while it waits.
- 400, like a total. It costs the rate: the agent stops at the first refusal.

A total still answers 400. Neither holds a request: each is refused at once, and the program
decides to wait, so this does not touch "a budget never holds".
