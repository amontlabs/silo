# Which costs of Silo's own processes have a target

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- `speed-budgets` covers fork, start, checkpoint and an idle computer's memory. This covers the
  processes Silo adds on top, as `silo-is-light` asks.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Pay fixed costs once per batch.** Add up known costs for a whole batch and handle variable costs
  apart. Gather several statistics in one pass. The recorder writes lines in batches, compresses in
  blocks and counts sizes as it goes.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/GAS-METERING.md?plain=1#L594))
- **Hot fields together, cold ones apart.** Group the fields read on every step, put rarely read
  ones after them, and mark only truly rare branches as cold. `engine-shape` leaves memory layout
  out of the core, rightly at its size. This belongs in the helper and the recorder, where data
  flows.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm.zig#L134))
- **Memory by lifetime, with a cap on what is kept.** Use an arena for allocations that end
  together, give a nested call its own, clear while keeping capacity, and cap what is kept for
  reuse. For the helper, the recorder and the client, which allocate. The core allocates nothing
  after start.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm.zig#L211))
- **Allocate nothing until it is needed.** An empty container allocates nothing until used, and an
  empty source is never copied. An idle computer's helper holds no buffers it is not using. The core
  is the exception: it allocates everything at start.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/storage/journal.zig#L45))
- **Bounded caches that report their hit rate.** A cache of external fetches has a fixed size, and
  exposes hit and miss counts so its size can be tuned.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/src/storage_injector.zig#L9))
