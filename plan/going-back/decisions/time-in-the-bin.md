# How long a dropped computer can be brought back

## The choice

An agent's part, 22 November 2026. On Monday you drop the fork where an agent tried Node 24. On
Thursday you remember a script it wrote there.

The options:

- 7 days, then gone (recommended). On Thursday the fork is still in the bin, and you bring it back.
  After 7 days it is removed with its checkpoints, and only its record stays. It costs disk on your
  Mac for each dropped computer during that week, and a fork you remember after the week is gone.
- Until you empty the bin, as with the Trash on your Mac. Nothing goes until you say so. It costs
  disk: dropped forks pile up, and each grows as main moves on, since it keeps its own copy of
  whatever main changes afterwards.

Recommended: 7 days, then gone. Most second thoughts come within days, and a bin that empties itself
never fills your disk without your knowing.

## Notes

The property's sentence holds two claims: that a dropped computer comes back while it is in the
bin, and that nothing of it but its record remains afterwards. It is copied as the design record
gives it.

## The bin's size against the disk (26 November 2026)

An agent's part, 26 November 2026, from `research/2026-11-25-disk-budget/`, a simulation on Linux. Two forks a day that each
rewrite 3 GiB hold 42 GiB in a 7-day bin; four of 6 GiB hold 168 GiB, more than a 512 GB Mac's
checkpoints and recordings together. A fork's disk is an APFS clone, which #411 found cannot be
compressed, so after compression the bin is the largest thing Silo keeps. A challenge to "Silo
never empties the bin early on its own", on the heavy case: with the bin untouched, a 512 GB Mac
paused every computer for 238 hours in 30 nights; with the oldest dropped computer removed once
no automatic checkpoint older than 24 hours is left, with a fact naming it and an alert, there
was no pause. Proposed: the bin counts in Silo's part of the disk, and its oldest computer goes
second, after old checkpoints and before anything pauses.
