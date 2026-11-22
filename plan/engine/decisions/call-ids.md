# What happens when a caller repeats a call

## The choice

An agent's part, 22 November 2026. You type the command that forks main. The engine makes the fork,
the answer is lost on its way back, and the command line sends the call again.

The options:

- Carried out once (recommended). The second send gets the answer of the first, and there is one
  fork. The same holds for an agent's script that retries. It costs nothing you see: the engine
  remembers recent calls, in a space of fixed size.
- Never sent again. The command line reports that it does not know whether the fork was made, and
  you look. It costs you a check by hand after every lost answer, and a script cannot retry without
  risking two forks.

Recommended: carried out once. Lost answers are ordinary, and a script can only retry when a retry
cannot do the thing twice. Stripe's payment calls work this way.
