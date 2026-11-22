# Is the engine its own program

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Check settings where they enter.** Check settings read from the environment or a file against
  their allowed values where they enter, not where they are used. The engine, the helper and the
  command line refuse a bad setting at start with a message naming it.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/build.zig.md?plain=1#L426))
- **A setting that does nothing is removed.** Every setting changes behaviour or is removed. A
  timeout that is exposed is enforced. A field in a recipe or a manifest that Silo ignores reads as
  a promise.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/provider/REVIEW_SUMMARY.md?plain=1#L50))
- **The safe choice is the default.** The default of every setting is the safe one, and anything
  unchecked needs an explicit choice. "No rule means deny" sets this for requests at the boundary.
  This extends it to every setting.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/bytecode/REVIEW_SUMMARY.md?plain=1#L103))
- **Startup and shutdown in order.** Set up logging before anything else starts, let help and
  version flags exit before any setup, and stop services in order on a termination signal.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/docs/context/%5Bpass%202/5%5D%20phase-10-runner%20%28Runner%20%28Entry%20Point%20%2B%20CLI%29%29.md?plain=1#L39))
- **An empty configuration is a valid one.** Every setting has a default, so an empty configuration
  is a valid production one, and a test pins each default.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/txpool/pool.zig#L6))
