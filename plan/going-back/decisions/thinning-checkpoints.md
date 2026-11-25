# How old automatic checkpoints are thinned

## The choice

An agent's part, 25 November 2026, from #351, #352, #354 and #365, as #364 (section 10) reconciles them, and
#370's spec of the first build.

The case: at 02:47 an agent deletes a folder, and the checkpoint before it is the one you want in the
morning. Under the earlier answer it is thinned at 04:47, since only 2 hours are kept whole (#351).
The save before a restore goes the same way two hours later (#354).

What each draft asks:

- Keep, whatever their age, the save before a restore, a checkpoint an open alert points to, and one
  not yet compared (#351, #352, #354). None conflicts with another.
- Keep the first of each hour and of each day, so that thinning gives the same result whenever it
  runs (#365, checked on 300 random histories).
- Keep every checkpoint for 24 hours (#352), or 2 hours as the earlier answer says.
- 30 days of dailies take 247 to 413 GiB beyond the computer's disk, under #365's model of agents
  rewriting 2 GiB an hour; 7 days take 72 to 105 GiB. #370's spec keeps 7 days of dailies.

The options:

- 24 hours whole, the first of each hour and day, the three kinds kept whatever their age, 30 days
  as the reach, a limit as a share of free space, and an alert when it cuts short (recommended). It
  costs the blocks changed in a day, and a Mac with little space reaching back a week, not a month,
  which the Timeline then shows.
- The same with 7 days of dailies, as #370's spec has it. It costs nothing on disk, and promises
  less where the disk could hold more.
- The earlier answer: 2 hours whole. It loses the night's checkpoints before the morning.

Recommended: the first. It keeps the night whole until the morning, and lets the disk, not a fixed
number, decide how far back each Mac reaches. The lengths stay settings.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
