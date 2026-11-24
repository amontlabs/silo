# How old automatic checkpoints are thinned

## The choice

An agent's part, 22 November 2026. Main has run for three weeks with a checkpoint at each turn of
three agents, tens of thousands of checkpoints in all.

The options:

- By age, with a disk limit (recommended). You keep every checkpoint of the last 2 hours, one per
  hour for 2 days, and one per day for 30 days. If they still take more disk than the limit, the
  oldest go first and you are told. It costs precision further back: from last week you get one
  moment per day.
- The last 500. It costs history: with three agents at work, 500 checkpoints can cover less than an
  hour.
- Everything, until you remove it. It costs disk on your Mac, until it is full and no new checkpoint
  can be taken.

Recommended: by age, with a disk limit. Time Machine thins backups the same way, because people want
many recent moments and few old ones.

## Notes

That old automatic checkpoints are thinned, and that removing them really frees space, comes from
the plan's notes. That a rule does it, on their age and number, is a proposal.

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The spec of step 4 keeps every
automatic checkpoint for 24 hours, then one per hour for 2 days and one per day for 7 days, and
never removes a checkpoint you took, the save before a restore or a put-back, a checkpoint not yet
compared, or the newest. It combines #351, #352 and #354 as #364 proposes. The three lengths are
settings, and their values are polarzero's: #365 measured that 30 days of dailies hold 247 to 413 GB
beyond the live disk under its model of agents rewriting 2 GB an hour, and 7 days 72 to 105 GB.
