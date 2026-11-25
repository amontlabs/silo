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
