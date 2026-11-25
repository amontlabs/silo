# Checkpoint retention under a disk limit, and simulators per agent in main

An agent's research, 25 November 2026. Two subjects from the backlog:

1. How checkpoints are kept when the disk sets a limit, simulated over 30 nights
   (`thinning-checkpoints`, `low-disk`; it continues #365, and reads `thinning-checkpoints` as #376
   writes it).
2. How many simulators fit inside main, and how long agents wait for one (`sizing-target`,
   `simulators-for-parallel-agents` of #361).

Both are simulations in Python on Linux, in [`sim/`](sim/). Nothing here was measured on a Mac.
The write rates, the sizes and the memory figures are inputs chosen to bracket the plan's own
guesses (#365's model of agents rewriting 0.5 to 5 GiB an hour), not measurements.

## What it found

1. **On a 512 GB MacBook, the way back reaches 2 to 4 days, not 30.** With 200 GiB free and
   checkpoints allowed half of it, an agent rewriting 2 GiB an hour day and night leaves 2.3 to
   4.3 days of checkpoints; at 0.5 GiB an hour, 17 days; at 5 GiB an hour, about one day. A macOS
   computer whose TRIM does not reach the image keeps 2.0 to 2.4 days at 2 GiB an hour. 30 days
   fit only light work on a 1 TB Mac (159 GiB). Without a limit, 30 days of dailies at 2 GiB an
   hour hold 454 GiB, and #370's 7 days hold 282 GiB, which also overflows a 512 GB Mac.
2. **Removing the oldest first is the right order.** Two other orders were tried under the same
   limit. Thinning evenly by age keeps a longer reach (13 days instead of 4) but leaves holes of
   up to 6 days in it, and removes 35 times more checkpoints (1,459 against 41 in 30 nights), each
   of which makes the computer's writes wait (#365: 0.08 to 0.19 s on XFS). Removing what frees
   the most per hour of precision lost ended up close to oldest first (the same 41 removals at
   2 GiB an hour), so it adds nothing for its cost.
3. **The night is kept whole under every limit tried, with oldest first.** At 08:00 every
   checkpoint of the night was still there on 30 mornings of 30 in every workload, because the
   limit takes nothing from the last 24 hours while something older remains. The earlier answer
   (2 hours whole) lost the night on every morning.
4. **An alert on each removal younger than 30 days would come every day.** At 2 GiB an hour on a
   512 GB Mac the limit removed 41 such checkpoints in 30 nights, about one each night once it
   starts.
5. **A checkpoint kept whatever its age costs up to the whole disk.** It holds every block
   rewritten since its moment, until that covers everything the computer rewrites. With the two
   saves before a restore never set aside and one alert in four forgotten (7 such checkpoints after
   30 nights), the reach on a 1 TB Mac fell from 14.3 to 8.3 days at 2 GiB an hour.
6. **A checkpoint by hand every night overflows any limit.** `thinning-checkpoints` thins only
   automatic checkpoints. At 5 GiB an hour, one taken by hand at 22:00 each night held 720 GiB
   after 30 nights; the limit could remove nothing else, so the disk filled.
7. **Checkpoints at each turn cost removals, not disk.** One a minute instead of one every 5
   minutes held the same 100 GiB and the same reach, and raised the most removed in one hour from
   15 to 63.
8. **Simulators: a lease per use, kept warm, and a bigger Mac.** Main's memory is fixed when it
   starts. On a 32 GB Mac, 16 agents with three Xcode builds at once do not fit before any
   simulator. With a simulator held per use and kept booted until another agent needs the place,
   16 agents need about 6 booted to keep waits under 30 s for 95 uses in 100; held per session,
   every agent beyond the maximum waits all night.

## What it changes in the plan

| Entry | State | What this adds |
|---|---|---|
| `thinning-checkpoints` | open | Confirms oldest first and 24 hours whole (#376's answer). Challenges the alert on each removal younger than 30 days. Proposes that checkpoints kept whatever their age and those taken by hand count against the limit, never removed by it, with Silo asking you to set them aside. Notes appended |
| `low-disk` | open | A limit as a share of free space removes checkpoints when the Mac fills from other files, which is the option this entry does not recommend. Notes appended with the two ways to make them agree |
| `sizing-target` | open | The heavy load does not fit an iOS developer on 32 GB; proposes 16 agents with 6 booted simulators on 64 GB, and 4 agents with 2 on 32 GB. Notes appended |
| `simulators-for-parallel-agents` (#361) | open, not on main | "For as long as it uses them" should mean per use, kept warm; a note to add when #361 lands is in "Simulators per agent" below |

No verdict is written, no list changes, and no new entry is needed: both choices are open
entries already.

## The simulation of checkpoints

[`sim/retention.py`](sim/retention.py). One computer, main, with a 96 GiB disk image of 1 MiB
blocks, 40 GiB of them used. Each write ends a block's previous version. A checkpoint holds every
version that was current at its moment, so the space checkpoints take beyond the live disk is the
number of ended versions that at least one kept checkpoint holds. This is what clones that share
blocks keep (#365's `full`, `cbw`) and what chained deltas keep (`delta`).

A day: agents and you work from 09:00 to 18:00, agents alone from 22:00 to 06:00, almost nothing
is written the rest of the time. 80% of writes land in a working set, the rest anywhere in the
used part (#365's model). A macOS computer whose TRIM does not reach the image writes each new
block to the next free place, so its rewrites keep landing on blocks older checkpoints still
hold. On nights 4, 11, 18 and 25 a reinstall rewrites a block of the working set (2 to 12 GiB).
A restore happens at 02:47 on nights 7 and 19, and Silo saves first; you set the save aside the
next evening. Alerts point to a checkpoint on 8 nights, and you set each aside at 10:00. On day
12 you download 40 GiB to the Mac, and delete it on day 20.

A checkpoint is taken every 5 minutes when something was written. Every 15 minutes Silo thins by
age (every checkpoint of the last 24 hours, the first of each hour for 2 days, the first of each
day for 30 days, as #376 writes the answer), then removes more while checkpoints hold more than
the limit. The limit never reaches into the last 24 hours while something older can go, and never
removes the newest checkpoint.

Two Macs: a 512 GB one with 200 GiB free before any checkpoint, and a 1 TB one with 500 GiB
free. Each run covers 30 nights from a new main; three runs go to 90 nights, and two other seeds
check the spread (reach 3.3 and 3.3 days against 4.3 for medium, 512 GB, oldest first).

### The policies

| Name | Thinning by age | Limit | Removed first over the limit |
|---|---|---|---|
| `earlier-2h-no-limit` | the plan before #376: 2 hours whole | none | |
| `30d-no-limit` | #376 | none | |
| `7d-no-limit` | #370's spec: dailies for 7 days | none | |
| `30d-half-oldest` | #376 | half of (free space + checkpoints), keeping 50 GiB free | the oldest |
| `30d-half-even` | #376 | the same | the one whose removal leaves the smallest gap for its age |
| `30d-half-cheapest` | #376 | the same | the one that frees the most per hour of precision lost |
| `30d-100g-oldest` | #376 | 100 GiB | the oldest |
| `30d-floor-only-oldest` | #376 | everything above 50 GiB free | the oldest |
| `30d-floor-only-even` | #376 | the same | as `even` |
| `30d-half-even-morning` | #376, with the day starting at 07:00 | half | as `even` |

### Results after 30 nights

"Reach" is the age of the oldest automatic checkpoint at 08:00 on the last morning. "Gap" is how
far before a moment 7, 14 or 28 days back the nearest kept automatic checkpoint is; "gone" means
none is that old. A negative free space means the policy would need more than the disk.

#### 512 GB Mac, 200 GiB free

| Workload | Policy | Most held (GiB) | Least free (GiB) | Reach (days) | Nights whole at 08:00 | Gap at 7 / 14 / 28 days (h) | Removed under 30 days by the limit | Most removed in an hour |
|---|---|---|---|---|---|---|---|---|
| 0.5 GiB/h | `30d-no-limit` | 159 | 41 | 29.3 | 30/30 | 8 / 8 / 8 | 0 | 12 |
| 0.5 GiB/h | `30d-half-oldest` | 100 | 80 | 17.3 | 30/30 | 8 / 8 / gone | 17 | 14 |
| 0.5 GiB/h | `30d-half-even` | 100 | 80 | 26.3 | 30/30 | 7 / 8 / gone | 466 | 25 |
| 2 GiB/h | `earlier-2h-no-limit` | 451 | -251 | 29.3 | 0/30 | 8 / 8 / 8 | 0 | 13 |
| 2 GiB/h | `30d-no-limit` | 454 | -254 | 29.3 | 30/30 | 8 / 8 / 8 | 0 | 13 |
| 2 GiB/h | `7d-no-limit` | 282 | -82 | 6.3 | 30/30 | gone | 0 | 13 |
| 2 GiB/h | `30d-half-oldest` | 100 | 80 | 4.3 | 30/30 | gone | 41 | 15 |
| 2 GiB/h | `30d-half-even` | 100 | 80 | 12.8 | 30/30 | 138 / gone / gone | 1,459 | 25 |
| 2 GiB/h | `30d-half-cheapest` | 100 | 80 | 4.3 | 30/30 | gone | 41 | 15 |
| 2 GiB/h | `30d-100g-oldest` | 100 | 60 | 4.3 | 30/30 | gone | 35 | 14 |
| 2 GiB/h | `30d-floor-only-oldest` | 150 | 50 | 7.3 | 30/30 | 8 / gone / gone | 31 | 15 |
| 5 GiB/h | `30d-no-limit` | 921 | -721 | 29.3 | 30/30 | 8 / 8 / 8 | 0 | 12 |
| 5 GiB/h | `30d-half-oldest` | 100 | 80 | 1.2 | 30/30 | gone | 3,203 | 66 |
| 5 GiB/h | `30d-half-even` | 100 | 80 | 1.2 | 21/30 | gone | 4,379 | 269 |
| 5 GiB/h | `30d-floor-only-oldest` | 150 | 50 | 1.8 | 30/30 | gone | 768 | 28 |
| macOS, no TRIM, 2 GiB/h | `30d-no-limit` | 1,006 | -806 | 29.3 | 30/30 | 8 / 8 / 8 | 0 | 12 |
| macOS, no TRIM, 2 GiB/h | `30d-half-oldest` | 100 | 80 | 2.4 | 30/30 | gone | 71 | 14 |
| macOS, no TRIM, 2 GiB/h | `30d-half-even` | 100 | 80 | 6.4 | 30/30 | gone | 651 | 38 |

#### 1 TB Mac, 500 GiB free

| Workload | Policy | Most held (GiB) | Least free (GiB) | Reach (days) | Nights whole at 08:00 | Gap at 7 / 14 / 28 days (h) | Removed under 30 days by the limit | Most removed in an hour |
|---|---|---|---|---|---|---|---|---|
| 0.5 GiB/h | `30d-half-oldest` | 159 | 341 | 29.3 | 30/30 | 8 / 8 / 8 | 0 | 12 |
| 2 GiB/h | `30d-half-oldest` | 250 | 230 | 14.3 | 30/30 | 8 / 8 / gone | 21 | 14 |
| 2 GiB/h | `30d-half-even` | 250 | 230 | 22.3 | 30/30 | 8 / 104 / gone | 669 | 22 |
| 2 GiB/h | `30d-floor-only-oldest` | 450 | 50 | 28.3 | 30/30 | 8 / 8 / 8 | 1 | 13 |
| 5 GiB/h | `30d-half-oldest` | 250 | 230 | 4.3 | 30/30 | gone | 35 | 14 |
| 5 GiB/h | `30d-floor-only-oldest` | 450 | 50 | 11.3 | 30/30 | 8 / gone / gone | 25 | 15 |
| macOS, no TRIM, 2 GiB/h | `30d-half-oldest` | 250 | 230 | 6.3 | 30/30 | gone | 32 | 14 |

Every run, with every policy, is in [`results/tables.md`](results/tables.md); the raw lines,
with the reach and the space held on each morning, are in `results/part*.jsonl`.

#### Variants

| Run | Most held (GiB) | Reach (days) | What it shows |
|---|---|---|---|
| 2 GiB/h, 512 GB, oldest, a checkpoint each minute | 100 | 4.3 | the same disk and reach; up to 63 removed in an hour instead of 15 |
| 2 GiB/h, 1 TB, oldest, saves never set aside, 1 alert in 4 forgotten | 250 | 8.3 | 7 such checkpoints cost 6 days of reach (14.3 without) |
| 5 GiB/h, 512 GB, oldest, a checkpoint by hand each night | 721 | 0 | the limit cannot remove them, and the disk fills |
| 2 GiB/h, 1 TB, oldest, 90 nights | 250 | 15.3 | the reach settles at 15.3 days from night 40 on |
| 2 GiB/h, 1 TB, even, 90 nights | 250 | 27.3 | the reach swings between 23 and 28 days, with holes |

### What the numbers mean for the answer

- **The disk sets the reach, and the reach is short.** The answer's 30 days is a ceiling most Macs
  never reach with agents working through the night. The Timeline showing how far back the kept
  checkpoints reach, as #376 adds, is what tells you the truth; the 30 days in the answer should
  read as "up to".
- **Oldest first.** It gives one simple fact to show ("you can go back 4 days"), keeps no holes,
  and removes the fewest checkpoints. Thinning evenly buys reach with holes of days, and a hole a
  week back is worth little when you want the moment before a mistake.
- **The alert.** One alert each night that "a checkpoint younger than 30 days was removed" says
  nothing new after the first. A proposal: alert when the reach first falls under a length you
  set (7 days by default), and when it falls by more than half in a day, as on day 12 here when a
  40 GiB download shrank the limit by 20 GiB.
- **Checkpoints the limit cannot remove.** The save before a restore, a checkpoint an open alert
  points to, one not yet compared, and those you take by hand all count against the limit. The
  limit never removes them. When they alone hold more than half of it, Silo asks you to set some
  aside and shows what each one holds, since each grows towards the whole disk.
- **The first of each day.** With agents working through the night, the first checkpoint after
  midnight is the middle of a run. Starting the day at 07:00 makes the daily the end of the night,
  at no cost in disk (`30d-half-even-morning` held the same). It is a small change to the answer,
  and only polarzero can say which moment people want.

### What a limit as a share of free space does when the Mac fills

`low-disk` recommends that Silo remove nothing by itself when the disk runs low: it alerts, then
pauses. A limit set as a share of free space removes checkpoints when the Mac fills with other
files: on day 12 here, a 40 GiB download made it remove a day of checkpoints. That is the option
`low-disk` does not recommend ("remove what Silo kept on its own first"), reached by another
route. Two ways to make the entries agree:

- **The limit follows free space (recommended).** Thinning is a rule on facts, which
  `low-disk-or-disk` and `thinning-checkpoints` allow, and it acts well before `low-disk`'s first
  level, so the computers keep working and a short reach is shown, not a paused night. `low-disk`
  then covers what thinning cannot free: the live disks, the bin and the checkpoints the limit may
  not remove. Its notes should say so.
- **The limit is a fixed size, set when Silo is installed from the space free then.** Silo never
  removes a checkpoint because the Mac filled; it alerts and pauses as `low-disk` says. It costs
  agents paused at night while the oldest checkpoints sit there, which is the case these
  agents are there for.

## Simulators per agent

[`sim/simulators.py`](sim/simulators.py). Apple's framework gives a macOS computer a fixed memory
size when it starts; whether it gives any back through the traditional balloon is still to find
out (`memory-given-back`). So main's memory is set before the night, and every simulator the
agents boot must fit in it.

### What fits

Inputs, each a guess except where a source is named: the Mac keeps 6 GB for itself on 16 GB, 8 on
32 GB, 10 on 64 GB; one Linux computer beside main takes 2 GB; main's macOS with nobody on its
screen 3 GB; each agent program 0.6 GB; sourcekit-lsp 1 GB per worktree; one Xcode build 4 GB at
its peak (xcfs measured 21 to 26 GB to warm three); the app under test 0.3 GB; a booted simulator
0.9 GB (a vendor's claim, [SimSlim](https://www.ithinkdiff.com/?p=356585)), 1.5 GB, or 3.4 GB
(xcfs, quoted in #361). Apple's forums report a simulator growing with each UI test run, and the
iOS 26 simulator taking most of a 16 GB Mac
([1](https://developer.apple.com/forums/thread/736714),
[2](https://developer.apple.com/forums/thread/787337)), so 3.4 GB is not a ceiling.

Booted simulators that fit in main:

| Mac | Main gets | 4 agents, 1 build | 8 agents, 2 builds | 16 agents, 3 builds |
|---|---|---|---|---|
| 16 GB | 8 GB | 0 | 0 | 0 |
| 24 GB | 15 GB | 0 to 2 | 0 | 0 |
| 32 GB | 22 GB | 2 to 8 | 0 to 2 | 0 (the rest takes 28.6 GB) |
| 48 GB | 38 GB | 7 to 22 | 5 to 16 | 2 to 7 |
| 64 GB | 52 GB | 10 to 33 | 8 to 27 | 6 to 19 |

(The range runs from 3.4 GB to 0.9 GB a simulator.) [`results/sim-memory.jsonl`](results/sim-memory.jsonl).

### How long agents wait

Each agent thinks and edits for 3 minutes on average, builds for 1, and on half its turns runs
the app for 1.5 minutes in its own simulator. A boot takes 25 s. One night of 8 hours:

| Agents | Booted at most | Held per session: agents who never get one | Held per use, kept warm: mean wait | p95 wait | Share of the night spent waiting |
|---|---|---|---|---|---|
| 4 | 2 | 2 | 23 s | 55 s | 4% |
| 4 | 3 | 1 | 9 s | 25 s | 2% |
| 8 | 3 | 5 | 27 s | 71 s | 5% |
| 8 | 4 | 4 | 18 s | 25 s | 3% |
| 16 | 4 | 12 | 40 s | 108 s | 7% |
| 16 | 6 | 10 | 22 s | 27 s | 4% |
| 16 | 8 | 8 | 17 s | 25 s | 3% |

With UI tests of 5 minutes instead of 1.5, 16 agents on 6 simulators wait 120 s on average (p95
361 s); running the app on every turn, 39 s (p95 97 s).
[`results/sim-wait.jsonl`](results/sim-wait.jsonl).

### What it means

- A simulator held for the whole session leaves every agent beyond the maximum without one all
  night. The answer of `simulators-for-parallel-agents` says an agent has its simulator "for as
  long as it uses them". It should say per use: the agent holds a place only while it runs the
  app; its simulator, a clone of its own, stays booted afterwards and is shut down only when
  another agent needs the place and it is the one used longest ago. Shutting it down keeps its
  data, so the cost is a boot (about 25 s here, a guess).
- For an iOS developer, the heavy load of `sizing-target` (16 agents on 32 GB) does not fit even
  without simulators, because three Xcode builds at once take most of main. The proposal written
  in `sizing-target`'s notes: 16 agents with 6 booted simulators on a 64 GB Mac, and 4 agents with
  2 booted on 32 GB. Builds may need a maximum at once too, as simulators do.
- Mac test 25 of #361's kit measures the memory of a booted simulator in a computer. These tables
  say which numbers it has to confirm.

Note to add to `simulators-for-parallel-agents.md` when #361 lands:

> An agent's part, 25 November 2026, from `research/2026-11-25-checkpoint-retention/`. Simulated
> over a night of 16 agents: with a simulator held for the whole session, 10 of 16 agents never
> get one when 6 may be booted. Held per use and kept booted until another agent needs the place,
> 6 booted keep 95 waits in 100 under 30 s. "For as long as it uses them" could read "while it
> runs the app; its simulator stays booted until another agent needs the place".

## What was not verified

- Every write rate, working set and memory figure is an input, not a measurement. A real day of
  agents on a Mac (#365) and Mac test 25 (#361) replace them.
- The model counts blocks, not APFS's own costs: the metadata of clones, the time to remove one
  (#365 Mac test 2), or `delta`'s merges.
- A macOS computer without TRIM is modelled as writing each new block to the next free place.
  Whether TRIM reaches the image is #365's Mac test 6.
- The bin and forks are left out; both take space from the same disk and would shorten the reach.
- The wait model treats boots as 25 s and agents as independent. Agents that start together after
  a shared prompt would wait longer.

## Running it

```
cd sim
python3 retention.py --part=0/4 > ../results/part0.jsonl   # four parts run side by side, about 30 minutes
python3 retention.py medium/mac-512                         # only the runs whose name contains this
python3 tables.py > ../results/tables.md
python3 simulators.py memory; python3 simulators.py wait
```

Python 3 with NumPy. The runs are seeded and give the same numbers each time.
