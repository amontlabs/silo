# How old automatic checkpoints are thinned

## For the owner

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
