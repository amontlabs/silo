# What the engine gives up when a failure forces a choice

## The choice

An agent's part, 22 November 2026. You ask to restore a fork. The engine accepts, and crashes a
moment later.

The options:

- Slower, and nothing forgotten (recommended). The engine writes each call to disk before it accepts
  it. After the restart it carries the restore out, or tells you it failed and why. When it cannot
  write, for instance with the disk full, it refuses the call instead of accepting it. It costs a
  write to disk on every call, and waiting while the engine recovers.
- Fast, and may forget. The engine answers at once and saves later. It costs the calls of the last
  moments before a crash: the restore never happens, nothing says so, and the app had shown it as
  accepted.

Recommended: slower, and nothing forgotten. Calls to the engine are rare next to what happens inside
a computer, so the write is not what you will wait for.

## Notes

The record checks `accepted-call-never-forgotten` with the simulator, with crashes.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A fallback default never loses data.** When an optional query falls back to a default, choose
  the default that cannot lose data, and say when the fallback is lossy. Matches
  `engine-under-failure`: take longer, never lose state.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/src/host.zig#L69))
