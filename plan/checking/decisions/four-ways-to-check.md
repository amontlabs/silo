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
part of the core. The owner agreed on 21 November 2026: "Yes we go full zig." The table's third
row and the answer were reworded on that day. The owner has not taken the entry yet.

Then the owner asked why not use the simulator alone. The simulator draws its sequences of calls
at random, so nothing guarantees that it draws the one order a fault needs. The test of every
sequence is certain, and can only afford short ones. In the trial both found both faults at
once. The owner, 21 November 2026: "we always use simulator and when we have a reason to believe
certain order we do that in addition". The answer and the
table were reworded to say so: the simulator always, and this test where a property depends on
the order of calls.

The plan's lists named this way of checking `model`. Its name is now `bounded exhaustive test`,
the standard term: it tries every case, and only up to a bound. "Exhaustive test" alone would
read as a test of everything.
