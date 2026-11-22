# Speed measured against budgets

## For the owner

An agent's part, 22 November 2026. Before the code that forks a computer is written, the fork gets a
target, such as 300 ms at the 99th percentile and 1 s at worst. Then forks of main are timed
hundreds of times, as a spread of results rather than one run, and set against that target. The same
is done for starting a computer, taking a checkpoint and the memory of an idle computer, for what
keeping the record costs a build and a test suite, and for the time Silo takes to recover after a
crash.

This is what `speed-budgets` and `how-speed-is-measured`, which you took, already say. Taking it is
a yes to building it when its stage comes.

The options:

- Build it (recommended). It costs machine time at each release, on a Mac and on a Linux host, and
  results that will sometimes show Silo slower than its targets.
- Leave it out. It costs the one way to know that Silo stays fast, and it goes against decisions you
  took.

Recommended: build it. People who run many agents already wait on their disk, and Silo must not add
a wait they can feel.
