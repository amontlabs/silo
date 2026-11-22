# How forks of a running Linux computer arrive

## For the owner

An agent's part, 22 November 2026. A Linux computer has three agents at work, and you fork it so
that the fork starts with the same programs running, where they were.

The options:

- Help upstream land it (recommended). Silo helps the pull request that adds memory snapshots to
  libkrun, the virtual machine library Silo runs on, with MicroSandbox's code as a reference. Until
  it lands, a Linux fork starts from a copy of the disk and boots cold: the files are all there, and
  the programs start again. It costs the date: upstream put the review off until after libkrun 2.0,
  which has no date, and nobody at Silo controls it.
- Carry it in Silo's own copy. Forks of a running Linux computer come sooner. It costs thousands of
  lines kept in step by hand with an upstream in the middle of a rewrite. MicroSandbox's copy drifted
  that way and was 653 commits behind upstream on 19 November 2026.

Recommended: help upstream land it. A copy that drifts costs more at each release, and a cold boot
from a copied disk already keeps everything the agents wrote.

Split on 22 November 2026 from `libkrun-patches`, which keeps how Silo carries its patches.
