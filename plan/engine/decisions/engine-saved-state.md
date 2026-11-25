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
