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

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The specs build sessions, as #352
(file 04) proposes: `register` opens a session, each request carries the session and a number one
higher than the last, and the table of sessions is in the engine's saved state. A `silo` command
whose engine restarted under it repeats its request with the same session and number, and gets the
first answer. Random ids instead would remove `register` and change the session table into a table
of ids.
