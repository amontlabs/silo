# What a budget on a conversation promises

## The choice

An agent's part, 22 November 2026. You give one conversation in main a budget of 5 dollars. It
reaches the limit, and the agent program starts a new conversation, which can spend 5 dollars
again.

The options:

- Budgets on a conversation, with that worst case said (recommended). A conversation that runs away
  stops at its limit while main's other agents go on. The budget says plainly that a new
  conversation starts again at zero, and that the limit nothing gets around is a budget on the
  computer. It costs a limit that an agent can step around by starting over.
- Budgets on computers only. A limit nobody steps around. It costs the narrow cap: to stop one
  runaway conversation you limit the whole of main, and every agent in it stops with it.

Recommended: budgets on a conversation, with the worst case said. Both limits are useful, and each
says exactly what it holds.

Split on 22 November 2026 from `recognising-a-conversation`, which keeps how a conversation is
recognised in the traffic.

## A conversation taken up in a fork (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-spend-budget-edges`. Main has a
budget of $5, and $0.50 on each conversation. A conversation in main spends $0.28 in three turns.
Main is forked, and the fork takes the same conversation up, with no budget of its own, so it
spends from main's (`budget-of-a-fork`). Main goes on with it too. Each turn costs about $0.09.

#373 proposes for `recognising-a-conversation` that requests from two computers are never joined,
and that a fork starts its own conversations. Built three ways and run:

| A fork's copy of a conversation | Turns, main then fork then main | Spent on that conversation, both computers |
|---|---|---|
| Starts at zero (#373's text) | 3, 6, 3: nothing refused | $1.10, $0.55 in each |
| Starts at what main's had spent, then counts apart | 3, 3, 3 | $0.83: the fork stopped at $0.55 |
| Is main's own conversation | 3, 3, 0 | $0.55: both stopped |

The options:

- Starts at what its parent's had spent, then counts apart (recommended). A fork cannot double a
  conversation's budget, and main's own copy goes on to its own limit, as a fork's file changes do
  not touch main's. It costs a total across the two copies of up to twice the budget less what was
  spent before the fork.
- Starts at zero. It costs the budget: each fork of a conversation can spend it again, the same
  worst case as a new conversation, but from a conversation the user set a limit on.
- One conversation across both. The tightest limit. It costs main a conversation that its fork
  can stop: work in a fork, meant to be thrown away, ends main's own.

Recommended: start at what the parent's had spent. The computer's budget is the limit that no
fork gets around in any of the three: main's ledger held every turn of both computers.
