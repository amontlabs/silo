# Who can call the engine

## The choice

An agent's part, 22 November 2026. An agent in a fork may use Silo's tools from inside. It sends the
engine a call that says it comes from the app, and asks for main's rules to be loosened.

The options:

- Every call proves where it comes from (recommended). The engine knows the call came from that fork
  by the way it arrived, whatever the call claims, and treats it as the fork's: it can grant the
  fork nothing more. A call that proves nothing is refused. It costs a step at install, when the app
  and the command line get what they prove themselves with, and a script of yours has to go through
  Silo's command line.
- Trust what runs on your Mac. Calls from your Mac need no proof, and only calls from inside a
  computer are marked. It is simpler, and any script can drive the engine directly. It costs the day
  the engine runs on a Linux host or a shared server, where other people's programs can reach it:
  the proof has to be added then, under tools that were written without it.

Recommended: every call proves where it comes from. It is cheap to build in from the first call and
hard to add later.

## Notes

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- The app and the command line reach the engine through one client (`engine-client`). Agents
  inside a computer may do what you can do in the app (`what-an-agent-inside-may-do`), so their
  calls reach the engine too, scoped to their computer.
- guillotine-mini's plan for its client makes authentication mandatory on its control port, and
  refuses an unauthenticated request before any handler runs.
