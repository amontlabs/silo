# Budgets on rates and on usage outside a plan

## For the owner

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

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `silos-own-extensions`.

## An agent's notes

- `budget-as-a-rate` asks whether a budget can be a rate and has no answer. This feature waits for
  it.
- "New conversations per minute" is not spend, and `word-budget` proposes that a budget is a limit
  on what is spent. It may be a plain rule on a count and not a budget.
- A budget on a computer or on a conversation is a rule (`budget-rule-or-precaution`, taken). A rate
  over the same traffic is seen at the same place, so it is exact too.
- How Silo tells a request covered by a plan from one billed outside it is to find out.
- An extension that adds rules could ship this.
