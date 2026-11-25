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

## The rule run over 40 days, and what it costs in disk (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-checkpoint-engine/` (`ckpt thin`): 40
simulated days of 10 hours of work, a checkpoint every 5 minutes, the rule as the answer gives it.

- **Which checkpoint stays for an hour or a day.** The answer does not say. Keeping the oldest
  still there makes thinning give the same result whether it runs every minute, once a day or after
  a crash; a test checks this on 300 random histories. Keeping the newest would remove checkpoints
  the rule wants later. The answer could say "the first of each hour and of each day".
- **How many.** Without checkpoints at each turn: 71 kept at most, 12 removed in an hour at most.
  With three turns a minute (`checkpoint-at-each-turn`): 504 kept and 233 removed in an hour. Each
  removal made the computer's writes wait 0.08 to 0.19 s on XFS; on APFS that is a test for the Mac.
- **How much disk.** Each daily checkpoint keeps its own copy of what the agents rewrote that day.
  Under a model of an agent rewriting 2 GiB an hour into a working set of 4 to 16 GiB, the kept
  checkpoints hold 247 to 413 GiB beyond the computer's disk with 30 days of dailies, and 72 to
  105 GiB with 7. So on most Macs the disk limit, not the 30 days, decides how far back you can go.
  The model's numbers are guesses until a Mac measures a real day of agents.

A challenge to the answer, on that case: a Mac with 200 GB free cannot keep 30 days of dailies of
a computer that rewrites 2 GiB an hour, so "one per day for the last 30 days" promises what the
default limit takes away within a week or two. A proposal: the limit as a share of the free space,
the Timeline showing how far back the kept checkpoints reach, and an alert when the limit removes
a checkpoint younger than 30 days.

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The spec of step 4 keeps every
automatic checkpoint for 24 hours, then one per hour for 2 days and one per day for 7 days, and
never removes a checkpoint you took, the save before a restore or a put-back, a checkpoint not yet
compared, or the newest. It combines #351, #352 and #354 as #364 proposes. The three lengths are
settings, and their values are polarzero's: #365 measured that 30 days of dailies hold 247 to 413 GB
beyond the live disk under its model of agents rewriting 2 GB an hour, and 7 days 72 to 105 GB.

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
