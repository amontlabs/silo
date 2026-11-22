# Is the core deterministic

## Found

A trial on 21 November 2026 built a small core in Zig 0.17.1. It is described in the notes of
`core-simulator`. The proposed answer holds up: time is a field of every input, and the core
starts no thread.

The trial also asked what keeps a core from reading the clock behind the simulator's back. A
clock read was put into the core on purpose, in nine ways.

| Check | Refused |
|---|---|
| The core is given no `std.Io`, the interface Zig code does I/O through since 0.16 | 1 of 9 |
| The core is built for a target with no operating system, and linked | 8 of 9 |
| A test reads the core's source and refuses anything but `std.debug.assert` | 9 of 9 |

The first check is weak because Zig's standard library still gives a clock without an `std.Io`:
through `std.c`, `std.posix.system`, `@extern` and `std.Io.Threaded.global_single_threaded`.

The nine ways, and what each check did with them:

| Put into the core | No `std.Io` given | Built for no operating system | Test that reads the source |
|---|---|---|---|
| A clock read through an `std.Io` the core does not have | refused | refused | not run: it does not compile |
| A clock through `std.Io.Threaded.global_single_threaded.io()` | compiles | refused | refused |
| `std.c.clock_gettime` | compiles | refused | refused |
| `std.posix.system.clock_gettime` | compiles | refused | refused |
| `@extern` of `clock_gettime`, declared by the core | compiles | refused, but only once linked | refused |
| The global clock reached through `const s = std;` | compiles | refused | refused |
| The system's randomness through the global `std.Io` | compiles | refused | refused |
| `std.Thread.spawn` | compiles | refused | refused |
| A counter in a variable outside the state, kept between calls | compiles | compiles | refused |

The build for no operating system catches whatever reaches the operating system. It has to be
linked, not only compiled. The test that reads the source looks for names, so a spelling nobody
thought of can get past it. Its check for a variable outside the state reads Zig's own parse tree
and does not have that weakness. The two are used together.

The ninth way is not a clock. It is a counter kept in a variable outside the state, so the same
inputs stop giving the same outputs. Only the test that reads the source caught it.

## The property this entry proposes

`core-touches-nothing` was shown to the owner on 21 November 2026, before the trial, with its
check left to the trial. The owner said to write it into the entry. Its check is a test, made of
the last two rows of the table.

A second property, `core-keeps-nothing-between-calls`, says that the core keeps nothing between
two calls outside its state. The ninth way above is the case for it. The owner said yes on
21 November 2026 to writing it into the entry.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **One encoding per value.** Decide once whether zero and absent are the same, and give every state
  exactly one representation. The simulator compares states byte for byte. Two encodings of one
  state would read as a divergence that is not there. Padding bytes are undefined in Zig, so rows
  have no padding, or the comparison goes field by field.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/src/storage.zig#L327))
- **Shared logic stays pure.** Shared code takes values and returns values, holds data rather than
  one consumer's policy, and has no consumer-specific code. The core is proposed as a pure function
  (`deterministic-core`), and `mechanism-or-use` keeps one use out of the engine. This extends the
  rule to every shared Zig module: values in, values out.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/.claude/commands/codesharing.md?plain=1#L40))
- **Monotonic durations, UTC times.** Measure durations with a monotonic clock and record times in
  UTC. The clock input to the core carries a monotonic value for limits and UTC for the record. A
  monotonic clock stops while the Mac sleeps, so which clock a hold's limit uses is still to name.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/test_runner.zig.md?plain=1#L318))
