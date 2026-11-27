# What the engine saves of its own state

The suggested choice is the state plus the inputs since the last save. It is still open.

What you can do with your computers while a damaged save keeps the engine from starting is part of
another choice. The question is put to polarzero in `when-the-engine-stops`, under "The choice".

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The specs build #352's proposed
text (file 02): a log of 4,096 slots, two places for the state, `F_FULLFSYNC` on a Mac, a stop on
any failed write, and a check against the recorder at start. The byte layout is in `formats.md`
there. Under the other option, a save of the whole state after each input, the log goes and nothing
else in the specs moves.

## The alternatives, and the check (27 November 2026)

An agent's part. A second agent, as judge, read this entry on its own and found it is about how
Silo is built, so agents may take it once the entries it needs are taken (`owners-part-on-engine`).

- The whole state saved after each input. It costs a full write and flush per input.
- SQLite or LMDB. It costs a dependency and the file of fixed size made in advance.
- Nothing saved, the state rebuilt from the drivers at start. It costs call ids and event numbers across a restart.
- Check: the simulator crashes at every step and restarts to the same state and the same event numbers, with fault tests for a torn tail and a failed write.
- Found by the judge: the answer and the first build's spec in these notes disagree on which failed write stops the engine. The answer also says what the engine starts from and what a newer engine does with an older format, which belong to `when-the-engine-stops` and `formats-carry-versions`.
