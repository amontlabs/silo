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

## Where a fork's copy starts, made exact (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-forked-conversations`, on the traffic of Claude Code 2.1.332 and Codex
0.172.0 in seven kinds of fork. It refines the option recommended in #385's notes, "starts at what
its parent's had spent", without changing the choice.

"What its parent's had spent" needs a moment. #385's prototype takes it when the fork first
calls. A fork of a checkpoint then starts with every turn main made after the checkpoint: one turn
in the runs (/bin/bash.0166 with Claude Code, /bin/bash.0207 with Codex), and a day of work for a checkpoint
taken a day before. Taken at the request the fork's first request continues, the copy starts at
exactly what its own history spent, in all seven cases: a fork, a fork with a new machine id, a
fork of a checkpoint, a fork of a fork (it carries main's turns and its parent's), and a fork that
becomes main (main's conversation goes on from the fork's spend, not from main's old one).
