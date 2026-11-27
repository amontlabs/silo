# Budgets on rates and on usage outside a plan

## The choice

An agent's part, 22 November 2026. You set main to at most 5 dollars in any hour. You also allow at
most 20 dollars a month of usage billed beyond your Max plan.

The options:

- Build it (recommended). A loop at night can spend no more than 5 dollars an hour, and usage beyond
  your plan has its own cap. It costs one more kind of budget to understand. How Silo tells usage
  billed beyond a plan from usage inside it is still to find out.
- Leave it out. Only the total budget limits a runaway. It costs the early catch: the runaway spends
  all of the total first.

Recommended: build it. A rate catches a runaway early, and usage beyond a plan is paid per call like
an API key.

## Notes

From the list of smaller features polarzero put on the roadmap on 21 November 2026. polarzero's
words on the list are in `what-ships-with-silo`.

## An agent's notes

- `budget-as-a-rate` asks whether a budget can be a rate and has no answer. This feature waits for
  it.
- "New conversations per minute" is not spend, and `word-budget` proposes that a budget is a limit
  on what is spent. It may be a plain rule on a count and not a budget.
- A budget on a computer or on a conversation is a rule (`budget-rule-or-precaution`, taken). A rate
  over the same traffic is seen at the same place, so it is exact too.
- How Silo tells a request covered by a plan from one billed outside it is to find out.
- An extension that adds rules could ship this.

## Where a plan's use shows in the traffic (24 November 2026)

An agent's part, 24 November 2026. Read from the two programs' code, not seen on a real
subscription's reply: the programs read these headers, so the providers send them.

- **Claude Code 2.1.331** reads, on each reply from Anthropic, `anthropic-ratelimit-unified-status`
  (`allowed`, `allowed_warning` or `rejected`), the use of the five-hour and seven-day windows as a
  fraction from 0 to 1 (`…-5h-utilization`, `…-7d-utilization`), when each resets
  (`…-5h-reset`, `…-7d-reset`, in Unix seconds), and the use beyond the plan:
  `…-overage-status`, `…-overage-in-use`, `…-overage-utilization` and `…-overage-reset`. When the
  status is `rejected` and the overage status is allowed, it counts the request as paid "by
  credits"; otherwise "by allowance".
- **Codex 0.172.0** reads `x-codex-primary-used-percent`, `x-codex-primary-window-minutes` and
  `x-codex-primary-reset-at`, the same for `secondary`, and `x-codex-credits-has-credits`,
  `x-codex-credits-balance`, `x-codex-credits-unlimited` and `x-codex-rate-limit-reached-type`.
  Over its WebSocket the same comes as a `rate_limits` event.

So the two things this feature leaves "to find out" are in traffic Silo already reads: a plan's
limits and how much of them is used, per window, and whether a request was paid beyond the plan.
Silo reads them from the reply and needs no account of its own. What stays to check is on a real
subscription: that the headers come on every reply and mean what the programs take them to mean.
It answers the same question for `spend-split-and-forecast`.
