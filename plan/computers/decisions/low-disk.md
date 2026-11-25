# What Silo does when disk runs low

## The choice

An agent's part, 22 November 2026. Your Mac has 18 GB free. Main takes 120 GB, its checkpoints 60
GB, and two forks in the bin 30 GB. An agent starts a build that writes 20 GB.

The options:

- Alert, then pause, and remove nothing (recommended). Under 20 GB free you get an alert that opens
  Disk on what takes the space. Under 5 GB every computer pauses, so the build stops before your
  disk is full. You empty the bin or remove checkpoints, then resume. It costs the agents' time
  until you act.
- Remove what Silo kept on its own first, as macOS does with Time Machine's local snapshots. The
  oldest checkpoints taken on their own, and the bin, go before anything pauses. It costs a way back
  you may have wanted, removed without a word from you.
- Alert only. It costs a full disk: the build fails in the middle of a write, and your Mac itself
  stops working well.

Recommended: alert, then pause. Checkpoints and the bin are your way back, and a disk is quick to
free once you see what fills it. How early it alerts is a setting with three presets.

## Notes

How early is part of the question.

polarzero's other first thoughts, rough: for disk, what many apps already do; how early is a
setting with a few presets.

To settle with the product and its design.

## What removing a checkpoint frees

Agent, 25 November 2026, from `research/2026-11-25-techniques-from-two-tools/`. On a Mac with a
local Time Machine snapshot, deleted data stays on the disk for up to a day, or until macOS needs
the space. Removing a checkpoint may then free nothing at once, and the private size APFS reports
for a clone reads 0. An open-source tool that keeps APFS clones counts what they hold by clone ids
and blocks instead. So the second level must not count on space freed by removing checkpoints in
the same minute, and Disk should say when a local snapshot holds freed space
(`tmutil listlocalsnapshots /`).

## Checkpoints under their limit (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-checkpoint-retention/`. The answer of
`thinning-checkpoints` as #376 writes it sets a limit on checkpoints as a share of the free space.
When the Mac fills with other files, that limit shrinks and thinning removes the oldest
checkpoints: in the simulation, a 40 GiB download on a 512 GB Mac removed one day of checkpoints.
This entry recommends against Silo removing what it kept on its own when the disk runs low, so the
two entries need one reading. Recommended: thinning by its limit is a rule on facts that runs long
before this entry's first level and keeps agents working; this entry covers what thinning may not
free, which is the live disks, the bin, and the checkpoints the limit never removes (the save before
a restore, one an open alert points to, one not yet compared, and those taken by hand). In 30
simulated nights with that limit, free space never fell under 50 GiB, so neither level of this
entry was reached; without a limit, the free space ran out within days at 2 GiB written an hour.
