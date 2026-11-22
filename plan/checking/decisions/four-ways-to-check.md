# Where each way of checking fits in Silo

The answer is worded from the last column of this table. The four ways are not the same thing.

| Way | What it lets you say | Cost | Where it fits in Silo |
|---|---|---|---|
| Assertion | "The engine checked this while running, and would have stopped." | Low | Everywhere in the core |
| Simulation | "This held in every one of the random histories we ran, crashes included." | The simulator, built once | The real Zig core against fake drivers |
| Bounded exhaustive test | "This holds after every possible sequence of calls, up to a length." | A test of about 200 lines, and the length stays small | Where a property depends on the order of calls. First for access: who can get what |
| Proof | "This holds for this code, for every input." | Very high: the proof is many times larger than the code, and every change to the code breaks it | At most one small function, later |

The core is a pure function over small tables. That is the shape these tools need, which is why
they are within reach here and are not for most software.

## The real core can be tried on every history up to a size

An agent's challenge, 21 November 2026, from the trial described in the notes of `core-simulator`.
The table above gave its third way, "checking a model", to a small formal model written apart
from the code. In the trial the real Zig core was run over every sequence of calls at the bounds of the small
model of access: 2 computers, 2 secrets, 2 hosts, 3 rules and 6 calls in a row. That took 44 s
and 509 MB for 10.5 million states, on a loaded Mac. One more call in a row, or a third computer,
was no longer practical.

So for access, trying the real core this way is enough, and the first engine code can be that
part of the core. polarzero agreed on 21 November 2026: "Yes we go full zig." The table's third
row and the answer were reworded on that day.

Then polarzero asked why not use the simulator alone. The simulator draws its sequences of calls
at random, so nothing guarantees that it draws the one order a fault needs. The test of every
sequence is certain, and can only afford short ones. In the trial both found both faults at
once. polarzero, 21 November 2026: "we always use simulator and when we have a reason to believe
certain order we do that in addition". The answer and the
table were reworded to say so: the simulator always, and this test where a property depends on
the order of calls.

The plan's lists named this way of checking `model`. Its name is now `bounded exhaustive test`,
the standard term: it tries every case, and only up to a bound. "Exhaustive test" alone would
read as a test of everything.

polarzero took the entry on 21 November 2026.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Each fast path can be switched off.** Every optional optimization has its own switch, on by
  default. Turning it off changes no result, and the check compares both. When the fast path cannot
  handle an input, the plain path does. For any fast path in the helper or the recorder, such as
  compiled rules: the check runs with it on and off and compares.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/ARCHITECTURE.md?plain=1#L939))
- **A plain twin at narrow seams.** Where a fast path exists, keep a deliberately simple version in
  one readable file, run both on the same input, compare at every step, and report the first
  difference with both states. Not for the core, and never a second model of access (`model-tool`).
  Where Silo has a fast path and an obviously right one, both in Zig: the record's index answers
  equal a scan of the files; inputs handled in a batch give the same result one by one.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/ARCHITECTURE.md?plain=1#L49))
- **An optimization is proven equal.** An optimized path gives the same results, errors and visible
  accounting as the plain one, and matches its whole pattern, not the start of it. Its own speed is
  measured apart. Guillotine's bug hunt found merged instructions with wrong operand order and
  swallowed errors.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/opcodes/REVIEW_SUMMARY.md?plain=1#L116))
- **Test one rule with the others off.** A test of one rule turns the other rules off first, so a
  failure names exactly one cause. Each rule of Silo gets a test alone; the bounded exhaustive test
  covers them together.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/txpool/limits.zig#L1215))
