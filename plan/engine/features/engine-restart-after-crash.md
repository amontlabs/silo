# The engine after a crash

## The choice

An agent's part, 22 November 2026. The engine crashes halfway through a restore of a fork you asked
for.

The options:

- Build it (recommended). The engine starts again from what it saved. The restore is finished or
  undone, never left halfway, and you are told which. Any call the engine had accepted is carried
  out or fails with a reason. It costs two answers nobody has yet: what exactly the engine saves,
  and whether it can find its running computers again, which a trial has to show. The engine also
  stops on purpose when it finds a bug in itself, so you will sometimes see it restart.
- Leave it out. After a crash the engine starts empty and you set things right by hand. It costs
  half-made forks and restores to clean up, and calls that vanish without a word.

Recommended: build it. Everything Silo promises about crashes rests on it.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Crash-prone tests run in their own process.** Run each case that can crash or hang in its own
  process, with a deadline, so one crash is pinned to its case and the run goes on. Silo needs this
  to test that the engine stops and restarts from saved state. Guillotine forks each test but waits
  without a deadline, so one hang blocks the whole run.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/specs/CLAUDE.md?plain=1#L84))
