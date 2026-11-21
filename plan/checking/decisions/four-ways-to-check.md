# Where each way of checking fits in Silo

The answer is worded from the last column of this table. The four ways are not the same thing.

| Way | What it lets you say | Cost | Where it fits in Silo |
|---|---|---|---|
| Assertion | "The engine checked this while running, and would have stopped." | Low | Everywhere in the core |
| Simulation | "This held in every one of the random histories we ran, crashes included." | The simulator, built once | The real Zig core against fake drivers |
| Checking a model | "This holds in every history of a small formal model of the design, up to a size." | A model of a few hundred lines, kept in step | Access: who can get what |
| Proof | "This holds for this code, for every input." | Very high: the proof is many times larger than the code, and every change to the code breaks it | At most one small function, later |

The core is a pure function over small tables. That is the shape these tools need, which is why
they are within reach here and are not for most software.

## The real core can be tried on every history up to a size

An agent's challenge, 21 November 2026, from the trial described in the notes of `core-simulator`.
The table above gives "every history of a small formal model, up to a size" to a separate model.
In the trial the real Zig core was run over every sequence of calls at the bounds of the small
model of access: 2 computers, 2 secrets, 2 hosts, 3 rules and 6 calls in a row. That took 44 s
and 509 MB for 10.5 million states, on a loaded Mac. One more call in a row, or a third computer,
was no longer practical.

So for access, trying the real core this way may be enough once the core exists. A separate model
would then serve only before the first engine code. The trial of `model-tool` compares the two.
