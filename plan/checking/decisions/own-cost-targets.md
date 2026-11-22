# Which costs of Silo's own processes have a target

## For the owner

An agent's part, 22 November 2026. Claude Code in main makes 200 model calls in an hour, and each
one passes through Silo, which reads it. Silo's own programs on your Mac use memory and processor
all the while, even when every computer is idle.

The options:

- Targets that hold (recommended). Each program Silo adds gets a target for its memory, processor,
  disk writes and the wait it adds, written before it is built and measured on a named machine. A
  change that misses a target does not go in. When a target cannot be met without you seeing less or
  waiting longer, you choose. It costs slower work on Silo, and features that wait until they meet
  their target or are redesigned.
- Targets as a guide. The same targets are measured and shown, and a change that misses one goes in
  with a note, to be tuned later. It costs weight that grows a few milliseconds at a time: each
  model call and each idle hour cost a little more, and nobody chose it.

Recommended: targets that hold. You asked on 21 November that anything of Silo's be as light as
possible, and a target holds the weight down only if missing it stops the change.

## Notes

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
