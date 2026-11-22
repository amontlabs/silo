# What the engine gives up when a failure forces a choice

The record checks `accepted-call-never-forgotten` with the simulator, with crashes.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A fallback default never loses data.** When an optional query falls back to a default, choose
  the default that cannot lose data, and say when the fallback is lossy. Matches
  `engine-under-failure`: take longer, never lose state.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/src/host.zig#L69))
