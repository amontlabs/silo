# The engine after a crash

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Crash-prone tests run in their own process.** Run each case that can crash or hang in its own
  process, with a deadline, so one crash is pinned to its case and the run goes on. Silo needs this
  to test that the engine stops and restarts from saved state. Guillotine forks each test but waits
  without a deadline, so one hang blocks the whole run.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/specs/CLAUDE.md?plain=1#L84))
