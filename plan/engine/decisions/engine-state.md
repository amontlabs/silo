# What form does the engine's state take

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

The answer's last sentence was added on 22 November 2026, from these lessons.

- **Group state by lifetime.** Keep durable state, the baseline at the start of an operation, and
  scratch state apart, each cleared at its own boundary. In the engine's tables: device-wide rows,
  per-computer rows, and per-call scratch, each with its own clear point.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/STATE-MANAGEMENT.md?plain=1#L7))
- **Derive it, don't store it twice.** Derive a value from the data instead of keeping a second
  copy. When two fields must say the same thing, check they agree before using either. The engine's
  rows hold no count or flag that can be computed from other rows.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/stack/stack.zig#L303))
- **The owner is part of every key.** Include the owning entity in every key, encode composite keys
  unambiguously, and test keys that differ only in the first or the last byte. Every row that
  belongs to a computer names it, so ids local to two computers can never be confused. Device-wide
  rows stay apart.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/README.md?plain=1#L281))
- **Different spaces, different types.** Keep two coordinate spaces apart, with a mapping between
  them, and never mix them. A path in the computer and a path on your Mac, a record position and an
  event number, a table index and an id: each gets its own Zig type (`enum(u32) { _ }`), so mixing
  them does not compile.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/ARCHITECTURE.md?plain=1#L495))
- **Deleting an owner deletes what it owns.** Deleting an entity removes the data it owns in the
  same step, after every step that may still read it. When a computer is removed for good, after the
  bin, its rows go in the same input and nothing is left that points at it. A drop only moves it to
  the bin (`drop-and-bin`).
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/docs/context/%5Bpass%201/5%5D%20phase-2-world-state%20%28World%20State%20%28Journal%20%2B%20Snapshot/Restore%29%29.md?plain=1#L27))

## The alternatives, and the check (27 November 2026)

An agent's part. A second agent, as judge, read this entry on its own and found it is about how
Silo is built, so agents may take it once the entries it needs are taken (`owners-part-on-engine`).

- Objects linked by pointers. It costs saving and replaying the state byte for byte.
- An embedded database. It costs a dependency and allocation in the core.
- One integer type for every id. It costs mix-ups that compile.
- Check: a test that must fail to compile when two kinds of id are mixed, and a source check that no row holds a pointer or a slice.
