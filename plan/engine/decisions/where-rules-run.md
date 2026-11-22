# Which processes may a rule depend on

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **One owner per counter.** Each counter has one owner that increments and checks it. A counter
  held for a scope is given back with a `defer` placed right after it is taken. A count a rule
  depends on, such as spend or the hosts seen, lives in the engine or in the helper, never both.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/BUG_HUNT_REPORT.md?plain=1#L614))
- **Prepare once, then check cheaply.** Turn input that is used often into a ready-to-run form once,
  and reuse it while its identity and settings are unchanged. When rules change, the helper compiles
  them into a lookup structure keyed by the rule set's version. Checking a connection is then a
  lookup, with no allocation and no walk through the rules.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/performance/DISPATCH.md?plain=1#L54))
- **Compact sorted arrays for small sets.** For a small set, a sorted array or bitset that fits in a
  few cache lines beats a hash map. Fixed tables are built at compile time. For per-computer lists
  such as allowed hosts, if they stay short. Measure the real sizes before choosing.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/preprocessor/dispatch_jump_table.zig#L8))
- **A rule sees only facts.** Give each user an interface with only what it may use. A rule receives
  a type that exposes facts only, and the check that reads the source refuses handing a rule
  anything else.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/docs/context/phase-0-db.md?plain=1#L50))
- **Destructive operations are not on shared interfaces.** A destructive operation lives on the
  concrete type that owns it; a shared interface refuses it by name, and a flag that permits it is
  named for what it permits and defaults to refusing. The engineering guide's own example: a
  precaution's type has no granting or destructive action in it. This is the same rule for every
  shared interface.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/db/read_only.zig#L183))
