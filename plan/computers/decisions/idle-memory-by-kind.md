# Does an idle computer keep the memory it was given

## The choice

An agent's part, 25 November 2026, from the challenge of #352 (file 05) to `idle-memory`, which polarzero
took, and to its property `idle-computer-gives-memory-back`.

The case: in the first build main is a macOS computer and the only one. Apple's framework has only a
traditional balloon, and whether a macOS guest answers it is unknown; one forum post says macOS has
no balloon driver (not verified). If it gives nothing back, the property "A computer that is idle
gives memory back to the Mac" is the first thing polarzero's Mac shows to be false.
`memory-given-back` already sends that result to polarzero; this entry is the answer ready for it.

The options:

- The property for Linux computers, and a macOS computer's memory a size you choose until it is
  shown to give memory back (recommended). libkrun returns freed memory on a Mac since 1.19.8. It
  costs a macOS main holding its whole size all night, which the first run shows.
- Keep the property for every computer, as polarzero took it, and wait for the measure. It costs a
  property in force that may already be false for main.

Recommended: the property for Linux computers now. If the Mac shows that a macOS guest answers the
balloon, a later decision sets the property for every computer again.
