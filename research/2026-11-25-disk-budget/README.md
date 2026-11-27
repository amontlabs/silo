# One disk budget for everything Silo keeps on a Mac

An agent's research, 25 and 26 November 2026. #389 gave checkpoints a limit as if they were alone
on the disk. They are not: the record's lines, the text of messages, recordings of computer use,
dropped forks in the bin and #365's reserve file sit on the same volume, and the plan gives each a
separate rule (`thinning-checkpoints`, `record-size-limit`, `time-in-the-bin`, `low-disk`). This
report puts them under one limit, simulates 30 nights on a 512 GB and a 1 TB Mac, and answers
which goes first when the disk is full.

It stacks on #389 and runs #389's checkpoint model unchanged ([`sim/budget.py`](sim/budget.py)
imports [`retention.py`](../2026-11-25-checkpoint-retention/sim/retention.py)). #411's compression
of checkpoints is one knob of it. Nothing was measured on a Mac; the one measurement here (the
size of a recording) ran on Linux.

## What it found

1. **On a 512 GB Mac, what Silo may not remove decides the way back, not the limit.** At 2 GiB
   written an hour, the last 24 hours of checkpoints, which thinning keeps whole, hold about
   34 GiB; two forks a day held for 7 days in the bin hold 42 GiB; recordings 19 GiB; the record's
   lines and text 1.2 GiB; the reserve 4 GiB. With half of the free space as the limit, that
   leaves checkpoints one day back, under every policy tried (1.0 to 2.1 days), against #389's
   4.3 with checkpoints alone. On a 1 TB Mac the same work keeps 10.3 days, against #389's 14.3.
2. **The bin is the second largest thing Silo keeps, and the one nothing may remove.** 42 GiB at
   two forks a day of 3 GiB each, 168 GiB at four forks of 6 GiB. On a 512 GB Mac with heavy work
   (5 GiB an hour, 8 agents), a limit that may not touch the bin left the computers paused for
   238 hours in 30 nights, 168 of them with agents at work, and the disk full for 9 hours. Letting
   the limit remove the oldest dropped computer once old checkpoints are gone: no pause, and free
   space never under 9 GiB.
3. **The record is small next to checkpoints, and recordings are most of it.** In 30 nights the
   lines took 0.4 GiB (3 agents) to 1.1 GiB (8 agents), the text of messages 0.8 to 2.1 GiB, and
   recordings 19 to 67 GiB at 0.75 GiB an hour. Removing every message text kept frees less than
   half an hour of checkpoints at 2 GiB an hour. Removing the record's content before checkpoints
   (`one-share-record-first`) bought 1 to 2 days on a 1 TB Mac for all 30 days of text and
   recordings; on a 512 GB Mac, 0.1 day.
4. **A recording takes 0.2 GiB an hour encoded by quality, 0.75 GiB at a fixed bitrate.** A
   scripted agent filled a form, scrolled, read a table and typed in an editor for 4 minutes at
   1440x900 ([`measure/screen.js`](measure/screen.js), [`results/recording.txt`](results/recording.txt)).
   By quality (x264 CRF 23 or x265 CRF 28) the frame rate barely changes the size, since a still
   screen costs almost nothing; at a constant 6 Mbit/s it is 3.5 times larger. A 20 GB record holds
   about 8 days of 3 hours of computer use a day at the fixed bitrate, and all 30 by quality.
5. **One limit or separate limits matter less than what each may remove.** Read side by side as
   the plan writes them today (checkpoints at half of the free space, the record's own 20 GB, the
   bin uncounted), the limits kept more checkpoints (1.8 days instead of 1.0 at 2 GiB an hour on
   512 GB), because the checkpoints' limit does not see the bin, and kept free space as high (43
   GiB at least). Where they fail is the same place: the heavy load on 512 GB paused for 238 hours
   under both. What one limit adds is one number to show and a place to count the bin.
6. **Half of the free space or everything above a floor.** On a 1 TB Mac at 2 GiB an hour, a
   limit of everything above 50 GiB free kept 24.3 days, half of the free space 10.3. On 512 GB
   both keep one to two days. A limit fixed when Silo is installed kept the same as half on these
   runs; the 40 GiB download of day 12 did not make it remove checkpoints, and did not pause.
7. **With #411's compression, the bin and recordings become most of Silo's part.** Checkpoints
   stored at 24% of what they hold (#411's zstd) reach 29.3 days on 1 TB at 2 GiB an hour under
   half of the free space, and 4.3 days on 512 GB. There, the last morning held 24 GiB of
   checkpoints, 42 GiB in the bin and 19 GiB of recordings. A fork's disk is an APFS clone, which
   #411 found cannot be compressed. With heavy work on 512 GB, compression alone left 150 hours
   paused; only the bin going early removed the pause.
8. **`store-messages-once` is what keeps message text small, and its full lists are what would
   make lines large.** With each request's messages kept whole, text took 130 to 145 GiB in 30
   heavy nights; the record's 20 GB limit then held 4 days of text and no recording. With the full
   list of hashes on every request (#386's finding 4), lines took 8.2 GiB instead of 1.1 GiB on the
   heavy 1 TB run. Lines are never removed (`size-limit-removes-no-line`), so that is about 100 GiB
   a year that no limit can free, and the case for #386's challenge.

## A proposal: Silo's part of the disk

- **One part, shown as one number in Disk.** Everything Silo keeps outside the computers' live
  disks counts: checkpoints, the record (lines, message text, recordings), the bin and the reserve.
  Disk shows the part and its split, as `record-disk-use-shown` and `disk-place` already want.
- **Its size: half of what is free to it, keeping 50 GiB free (recommended).** That is #389's
  limit applied to the whole part. With #411's compression it reaches 30 days on a 1 TB Mac. The
  other option, everything above the 50 GiB, gives 2.4 times the reach on 1 TB without
  compression, but Finder then always shows 50 GB free, since Silo's checkpoints are not
  purgeable the way Time Machine's local snapshots are.
- **Over the part, in this order:**
  1. automatic checkpoints older than 24 hours, the oldest first, as `thinning-checkpoints` says;
  2. the oldest computer in the bin, with a fact naming it and one alert, when nothing older than
     24 hours is left to thin;
  3. nothing else. The record's content goes only by the record's own limit
     (`size-limit-on-the-record`, taken): the part counts it and never removes it. Lines, the last
     24 hours of checkpoints, the save before a restore, a checkpoint an open alert points to, and
     those taken by hand are never removed by the part.
  Then `low-disk`'s levels apply: alert, then pause.
- **Why this order.** Per GiB freed, an old checkpoint loses the least: at 2 GiB an hour one day
  of reach costs about 34 GiB, one day of the bin 6 GiB, one day of recordings 0.75 GiB and one day
  of message text 0.03 GiB. Removing the record first buys almost no reach for its only copy of
  what was said and clicked. The bin comes second because a dropped computer is one you chose to
  drop, and the case it serves (a mistaken drop) is caught early; the night's checkpoints and the
  record are what the morning needs.
- **One reserve.** #365 needs a reserve file the computers write into if the volume fills;
  `record-full-after-bulk` keeps "room for that gap and for your answer" aside. One file of 4 GiB
  serves both: when the volume is full, Silo deletes it, pauses every computer, and writes the gap
  and the alert into the space it freed. The reserve counts in the part. Free space under the pause
  level can come from any app on the Mac; only the reserve is space Silo knows is there.

## What it changes in the plan

| Entry | State | What this adds |
|---|---|---|
| `low-disk` | open | Notes: Silo's part, its order, and a challenge to "Silo removes nothing by itself" on the heavy case: the bin going second removed 238 hours of pause |
| `thinning-checkpoints` | open | Notes: the limit counts the whole part, not checkpoints alone; on 512 GB the reach is 1 day at 2 GiB an hour, 4 with #411's compression |
| `time-in-the-bin` | open | Notes: the bin takes 42 to 168 GiB in 7 days; a challenge to "Silo never empties the bin early on its own", with the part removing the oldest after old checkpoints |
| `record-disk-size` | open | Notes: per agent-hour sizes from #386 carried over 30 nights, and recordings measured |
| `record-size-limit` | open | Notes: recordings are most of the record; encoding by quality is 3.5 times smaller |

No verdict is written, no list changes, and no new entry is needed.

## The simulation

[`sim/budget.py`](sim/budget.py), on #389's model of main: a 96 GiB image, a checkpoint every 5
minutes, thinning by age as #376 writes it, agents from 09:00 to 18:00 and from 22:00 to 06:00, a
reinstall on nights 4, 11, 18 and 25, restores on nights 7 and 19, a 40 GiB download on day 12
deleted on day 20. It adds:

| Part | light | medium | heavy | From |
|---|---|---|---|---|
| Main's writes | 0.5 GiB/h | 2 GiB/h | 5 GiB/h | #389 |
| Agents | 1 | 3 | 8 | a guess |
| Record lines | 0.29 MB an agent-hour | | | #386's heavy night, zstd as closed segments are |
| Message text | 0.55 MB an agent-hour | | | #386's heavy night, each request listing what it added |
| Computer use | none | 1 h a day | 3 h a day | a guess, at 0.75 GiB/h (measured, fixed bitrate) |
| Forks | none | 2 a day, 3 GiB each | 4 a day, 6 GiB each | a guess; 3 GB is `fork-shares-disk-blocks`'s case |
| Main's own disk grows | 0.2 GiB/day | 0.5 | 1 | a guess |

A fork works for 6 hours and goes to the bin for 7 days. The reserve is 4 GiB. Free space under
20 GiB alerts, under 5 GiB pauses every computer (`low-disk`'s normal preset); while paused nothing
is written, and the computers resume once free space is back over 20 GiB. "macos-no-trim" is the
medium load on a macOS computer whose TRIM does not reach the image.

The policies:

| Name | The limit | Over it, in order |
|---|---|---|
| `no-limit` | none | |
| `separate` | the plan's entries side by side: checkpoints at half of (free + checkpoints) keeping 50 GiB free; the record's own 20 GB (recordings, then text); the bin uncounted | checkpoints older than 24 h |
| `one-share` | the whole part at half of (free + the part), keeping 50 GiB free | checkpoints, recordings, text |
| `one-share-own-record` | the same; the record's own 20 GB as well | checkpoints |
| `one-share-own-record-bin` (proposed) | the same | checkpoints, then the bin |
| `one-share-record-first` | as `one-share` | recordings, text, checkpoints |
| `one-share-bin-early` | as `one-share` | checkpoints, recordings, the bin, text |
| `one-fixed` | half of what was free at install, fixed | checkpoints, recordings, text |
| `one-floor` | everything above 50 GiB free | checkpoints, recordings, text |

Nothing of the last 24 hours is removed by any policy, and a checkpoint other than an automatic
one goes only once its protection has ended.

### Results after 30 nights

"Reach" is how far back the oldest automatic checkpoint is at 08:00 on the last morning. The
part's last morning is in GiB. Every run is in [`results/tables.md`](results/tables.md).

| Run | Reach (d) | Text kept (d) | Recordings kept (d) | Checkpoints / lines / text / recordings / bin | Least free (GiB) | Hours paused (agents at work) |
|---|---|---|---|---|---|---|
| medium, 512 GB, `no-limit` | 29.3 | 30 | 30 | 164 / 0.2 / 0.3 / 9 / 1 | -28 | 447 (318) |
| medium, 512 GB, `separate` | 1.8 | 30 | 25 | 59 / 0.4 / 0.8 / 19 / 42 | 43 | 0 |
| medium, 512 GB, `one-share` | 1.2 | 11 | 10 | 37 / 0.4 / 0.3 / 7.5 / 42 | 58 | 0 |
| medium, 512 GB, `one-share-own-record` | 1.0 | 30 | 25 | 33 / 0.4 / 0.8 / 19 / 42 | 43 | 0 |
| medium, 512 GB, `one-share-own-record-bin` | 1.1 | 30 | 25 | 35 / 0.4 / 0.8 / 19 / 33 | 74 | 0 |
| medium, 512 GB, the same with #411's zstd | 4.3 | 30 | 25 | 24 / 0.4 / 0.8 / 19 / 42 | 74 | 0 |
| medium, 512 GB, `one-floor` | 2.1 | 30 | 20 | 67 / 0.4 / 0.8 / 15 / 42 | 50 | 0 |
| medium, 512 GB, `one-fixed` | 1.2 | 30 | 20 | 37 / 0.4 / 0.8 / 15 / 42 | 48 | 0 |
| medium, 1 TB, `separate` | 12.3 | 30 | 25 | 204 / 0.4 / 0.8 / 19 / 42 | 194 | 0 |
| medium, 1 TB, `one-share-own-record-bin` | 10.3 | 30 | 25 | 175 / 0.4 / 0.8 / 19 / 42 | 224 | 0 |
| medium, 1 TB, the same with #411's zstd | 29.3 | 30 | 25 | 102 / 0.4 / 0.8 / 19 / 42 | 313 | 0 |
| medium, 1 TB, `one-share-record-first` | 11.3 | 1.7 | 0.9 | 191 / 0.4 / 0.04 / 0.7 / 42 | 224 | 0 |
| medium, 1 TB, `one-floor` | 24.3 | 30 | 30 | 361 / 0.4 / 0.8 / 23 / 42 | 50 | 0 |
| heavy, 512 GB, `separate` | 0.8 | 30 | 15 | 62 / 0.8 / 1.4 / 18 / 79 | -23 | 238 (168) |
| heavy, 512 GB, `one-share-own-record` | 0.8 | 30 | 15 | 62 / 0.8 / 1.4 / 18 / 79 | -23 | 238 (168) |
| heavy, 512 GB, the same with #411's zstd | 1.0 | 30 | 9 | 20 / 0.9 / 1.7 / 18 / 123 | -28 | 150 (106) |
| heavy, 512 GB, `one-share-own-record-bin` | 1.0 | 30 | 8 | 84 / 1.1 / 2.1 / 17 / 0 | 9 | 0 |
| heavy, 1 TB, `one-share-own-record` | 1.0 | 30 | 8 | 84 / 1.1 / 2.1 / 17 / 168 | 141 | 0 |
| heavy, 1 TB, `one-share-own-record-bin`, #411's zstd | 2.1 | 30 | 8 | 40 / 1.1 / 2.1 / 17 / 168 | 216 | 0 |
| heavy, 1 TB, `one-share-own-record`, messages kept whole | 1.0 | 4 | none | 84 / 1.1 / 19.7 / 0 / 168 | 140 | 0 |
| heavy, 1 TB, `one-share-own-record`, full lists of hashes | 1.0 | 30 | 8 | 84 / 8.2 / 2.1 / 17 / 168 | 136 | 0 |
| light, 512 GB, `one-share-own-record-bin` | 16.3 | 30 | | 88 / 0.1 / 0.3 / 0 / 0 | 78 | 0 |
| light, 1 TB, every policy | 29.3 | 30 | | 152 / 0.1 / 0.3 / 0 / 0 | 337 | 0 |

Seeds 2 and 3 gave the same reach (1.0 and 1.2 days) on medium, 512 GB.

## What was not verified

- Every load, fork size, computer-use time and growth of main's disk is an input chosen to go
  with #389's write rates, not a measurement. A real week of agents on a Mac replaces them.
- The fork's own checkpoints are not modelled; a fork that keeps them in the bin holds more.
- The recording was measured on Linux with Playwright's own capture encoded again. A Mac's
  ScreenCaptureKit at Retina size, and the hardware encoder's bitrate, are a Mac test.
- What "free" means on a Mac is a Mac test: APFS shares free space between the volumes of a
  container, macOS counts purgeable space (local Time Machine snapshots, iCloud copies) as
  available, and #378 saw a local snapshot keep a removed checkpoint's space. Silo has to measure
  what a removal freed, not assume it.
- Whether a computer survives the reserve being used (writes refused inside, or the computer
  stopped by Apple's framework) is #365's Mac test.

## Running it

```
cd sim
python3 budget.py --part=0/4 > ../results/part0.jsonl     # four parts side by side, about an hour
python3 budget.py --grid2 --part=0/4 > ../results/own0.jsonl
python3 budget.py --grid3 --part=0/2 > ../results/zstd0.jsonl
python3 budget.py medium/mac-512                          # only the runs whose name holds this
python3 tables.py ../results/*.jsonl > ../results/tables.md
cd ../measure && node screen.js /tmp/screen 240           # needs Playwright; then ffmpeg, as in results/recording.txt
```

Python 3 with NumPy, and #389's `retention.py` next to it in the tree. The runs are seeded.
