# Who can call the engine

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- The app and the command line reach the engine through one client (`engine-client`). Agents
  inside a computer may do what you can do in the app (`what-an-agent-inside-may-do`), so their
  calls reach the engine too, scoped to their computer.
- guillotine-mini's plan for its client makes authentication mandatory on its control port, and
  refuses an unauthenticated request before any handler runs.
