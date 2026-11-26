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

## Under a disk limit, over 30 nights (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-checkpoint-retention/`: a simulation
on Linux of main over 30 nights, agents working from 09:00 to 18:00 and from 22:00 to 06:00, with
the answer as #376 writes it (24 hours whole, the first of each hour for 2 days, the first of each
day for 30 days) and a limit of half the space free for checkpoints. Write rates are inputs, not
measurements.

- **How far back.** On a 512 GB Mac with 200 GiB free: 17 days at 0.5 GiB written an hour, 2.3 to
  4.3 days at 2 GiB, about one day at 5 GiB, and 2.0 to 2.4 days for a macOS computer whose TRIM
  does not reach the image. On a 1 TB Mac with 500 GiB free: 30 days at 0.5 GiB, 14 days at 2 GiB.
  The 30 days are reached only by light work on a large disk.
- **Oldest first holds.** Thinning evenly by age kept 13 days instead of 4, with holes of up to 6
  days, and removed 1,459 checkpoints in 30 nights instead of 41. Removing what frees the most for
  the precision lost came out close to oldest first. With oldest first, every checkpoint of the
  night was still there at 08:00 on all 30 mornings, in every workload.
- **The alert, challenged.** At 2 GiB an hour on a 512 GB Mac, the limit removed a checkpoint
  younger than 30 days about once a night. An alert each time says nothing after the first. A
  proposal: alert when the reach first falls under a length you set (7 days by default), and when
  it falls by more than half in a day.
- **Checkpoints the limit may not remove.** A checkpoint kept whatever its age holds every block
  rewritten since its moment, so it grows towards the whole disk: 7 of them (two saves never set
  aside, alerts forgotten) cut the reach on a 1 TB Mac from 14 to 8 days. A checkpoint by hand each
  night, which thinning never removes, held 720 GiB after 30 nights at 5 GiB an hour and filled the
  disk. A proposal: these count against the limit and the limit never removes them; when they
  alone hold more than half of it, Silo asks you to set some aside and shows what each one holds.
- **The first of each day.** With agents at work through the night, the first checkpoint after
  midnight is the middle of a run. Starting the day at 07:00 makes the daily checkpoint the end of
  the night and cost no disk in the simulation. Which moment people want is polarzero's to say.
- **The limit and `low-disk`.** A limit set as a share of free space removes checkpoints when the
  Mac fills with other files (a 40 GiB download removed a day of them), which `low-disk` does not
  recommend doing on its own. Recommended: keep the share of free space, since thinning is a rule on
  facts that acts long before `low-disk`'s first level and keeps the agents working, and let
  `low-disk` cover what thinning may not free. The other way, a fixed size set at install, pauses
  agents at night while the oldest checkpoints stay.

Checkpoints at each turn (one a minute) held the same disk and reach, and raised the most removed in
one hour from 15 to 63.

## With everything else Silo keeps (26 November 2026)

An agent's part, 26 November 2026, from `research/2026-11-25-disk-budget/`. The limit above treats checkpoints as alone on
the disk. With the bin (two forks a day for 7 days: 42 GiB), recordings (19 GiB in 30 days), the
record's lines and text (1.2 GiB) and a reserve on the same volume, a 512 GB Mac at 2 GiB written
an hour keeps checkpoints 1.0 to 2.1 days back under every limit tried, and 4.3 days with #411's
compression; a 1 TB Mac keeps 10.3 days, and 29.3 with compression. The last 24 hours, kept whole,
hold about 34 GiB at that rate, so on a 512 GB Mac they are most of what the limit allows.
Proposed: the limit counts the whole part (see `low-disk`'s notes), and over it the oldest
automatic checkpoints go first, then the oldest computer in the bin. Everything above a 50 GiB
floor instead of half of the free space kept 24.3 days on 1 TB against 10.3, at the cost of a
Mac that always shows 50 GB free.
