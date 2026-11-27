# Step 4: the way back

Part of the build specs in this folder ([README](README.md)). It builds
`checkpoints-taken-on-their-own`, `thinning-old-checkpoints` and `restore-to-a-checkpoint`
(#348). It needs step 2, not step 3, and can be built beside step 3. The formats are in
[formats.md](formats.md), "Checkpoints".

## What you can do when it is done

```
$ silo checkpoints
main · 301 checkpoints · 9.4 GB held only by checkpoints · 188 GB free on Macintosh HD

  06:55 06:50 06:45 … every 5 min since Mon 07:00                       (288)
  Mon 06:00 Mon 05:00 … one per hour for 2 days                          (8)
  Sun · Sat · Fri · Thu                                                  one per day (4)
  Mon 22:58  taken by you · "3 agents tonight"
  Tue 07:02  saved before the restore to 02:40 · kept until you remove it

$ silo restore 02:40
Restore main to 02:40?
This ends every program running in main, and puts its whole disk back to 02:40.
What main wrote after 02:40 leaves main. Silo first saves where you are, so you can come back.
The record, and what you granted, stay as they are now.
Restore? [y/N] y
  stopping main         done in 6 s
  saving where you were checkpoint 07:02, kept until you remove it
  putting back 02:40    done in 2 s
  starting main         ssh main answers after 41 s
Main is at 02:40. `silo restore 07:02` goes back to where you were.
Take a conversation up again with `claude --resume` or `codex resume`.
```

The counts of what a restore leaves ("3,944 files changed since") come in step 5, once file facts
exist.

## The pieces

| Piece | Where | What it does |
|---|---|---|
| Checkpoint rows | `core/checkpoints.zig` | Every checkpoint of every computer: when, why, what it descends from, what keeps it |
| The schedule | `core/checkpoints.zig` | When the core asks for the next checkpoint |
| Thinning | `core/thinning.zig` | Which automatic checkpoints the rule removes. Pure, over times only |
| Restore | `core/operations.zig` | One operation of steps, as `setup` in step 2 |
| Space | `core/space.zig` | The floor of free space, and what happens under it |
| The catalog driver | `drivers/catalog.zig`, from step 2 | Takes, removes and restores checkpoints on the disk, as #365's catalog |
| The space driver | `drivers/space.zig` | Reads the volume's free space; deletes the reserve on the core's effect |

### What a checkpoint is in the first build

`checkpoint-per-system` (open) answers: an APFS clone of the disk image, with the auxiliary storage
of the same moment (#352 file 07), taken from outside. The specs build that, as the strategy
`clone` of #365:

1. The helper is asked to make the moment (below).
2. The catalog driver makes `ckpt/.tmp-<id>/`, clones `live/disk.img` and `live/aux.img` into it
   with `clonefile`, writes `meta`, flushes the folder with `F_FULLFSYNC`, and renames it to
   `ckpt/<id>/`. A checkpoint is whole or absent.
3. The result comes back to the core as an input: taken, with the clone's time, or failed with a
   named reason.

Whether the clone of a running disk is a moment in time depends on how the helper attaches the
disk, which #359's test 04 measures per mode. The specs build both answers, and test 04 picks one:

| Test 04 says | The helper does at step 1 |
|---|---|
| The clone is a moment in time in the chosen mode | Nothing. The clone is taken while main runs |
| It is not | `pause()`, the clone, `resume()`. Main stops for as long as the clone takes |

Two more Mac results can change the strategy, without changing anything outside the catalog driver
and the helper's disk attachment:

| Result | Then |
|---|---|
| #359 test 03 and 15: a write in main waits longer than the target (below) during a clone, once the image has fragmented | #365's `delta` strategy, behind the NBD attachment if test 32 shows a macOS computer boots from it. Each checkpoint is then a file of the blocks it copied, and a restore applies them |
| #359 test 31: cloning a file that shares blocks with the live image makes the live image wait on APFS | `delta`, not `cbw` |

The catalog's `meta` names the strategy, so a computer can carry checkpoints of both kinds while
the strategy changes.

### When checkpoints are taken

`automatic-checkpoints` (taken) says Silo takes them at intervals and at moments the engine sees.
The first build has no boundary, so the moments it sees are few:

| Reason in `meta` | When |
|---|---|
| `interval` | Every 5 minutes while the computer runs, counted in awake time |
| `start` | Just before a computer starts, from its stopped disk |
| `stop` | Just after it stops |
| `before-restore` | Step 2 of a restore |
| `before-put-back` | Before step 5's put-back |
| `you` | `silo checkpoint [-m note]` |

- **One at a time per computer.** A checkpoint never starts while the one before is still being
  made. When one is due and the last is not done, the due one is skipped, and the record says so.
  That is the minimum gap #352 file 07 asks for.
- **The clock never waits for the recorder.** A checkpoint the interval starts is taken even when
  the recorder is down and the engine's ring holds little room: the core keeps room in the ring for
  the events of interval checkpoints for 3 days (#352, the combined position on
  `events-until-the-recorder`). Without it, a recorder that stops at 01:00 leaves no checkpoint for
  the night.
- **Paused computers** are not checkpointed by the interval: nothing changed.

### Thinning

`thinning-checkpoints` (open) keeps every automatic checkpoint of the last 2 hours, one per hour
for 2 days and one per day for 30 days. Three drafts ask for changes, and #364 (section 10)
combines them. The specs build:

- **Kept by age:** every automatic checkpoint of the last 24 hours, then one per hour for 2 days,
  then one per day for 7 days. In each hour or day, the one kept is the oldest still there, so
  thinning often, rarely or after a crash keeps the same set (#365's test, over 300 random
  histories). The three lengths are settings, and their values are polarzero's: 24 hours is what
  keeps the night whole until the morning (#352), and #365 measured that 30 days of dailies hold
  247 to 413 GB beyond the live disk under a model of agents rewriting 2 GB an hour, against 72 to
  105 GB for 7 days.
- **Kept whatever their age:** a checkpoint you took; the save before a restore or a put-back,
  until you remove it (#352 file 07, #354); a checkpoint whose comparison is not written yet (step
  5); the newest checkpoint of each computer.
- **Removed by you:** `silo checkpoint remove <time>`, which asks first: removing a checkpoint is
  destructive (`kinds-of-other-actions`).

Thinning runs after each checkpoint is taken. It is a rule on facts, age and space, so it may
remove (`opinion-never-chooses-what-is-kept`).

With a checkpoint every 5 minutes, a computer holds at most 288 + 48 + 7 automatic checkpoints, and
the core's `checkpoints` table has 1,024 rows per computer allowed by `limits.zig` (8,192 in all).
When the table is full anyway, the interval checkpoint is refused with `table_full`, which is a
control fact and an alert in `silo` (`engine-limits`, as #352 file 03 proposes).

### Space

`space-the-engine-keeps` (#348), with #365's finding that on a full volume an overwrite of a block
a clone shares fails inside the computer:

| Free space on the volume | What Silo does |
|---|---|
| Above the floor | Nothing |
| Under the floor | Interval checkpoints stop. `silo` and `silo status` say so, with how much the checkpoints hold. Thinning goes on |
| The engine's own write fails with `ENOSPC` | The space driver deletes `reserve`, the record says so, and the engine can still write, remove a checkpoint you choose, and restore |

The floor is 20 GB and the reserve 4 GB until #359's tests 06 and 33 and #365's full-volume test on
APFS measure them. The space driver reads free space with `statfs` every minute and after each
checkpoint; the core decides from the value.

What Apple's framework does when the Mac refuses a write of the computer, an I/O error inside or a
stopped computer, is #365's Mac test 5, added to the kit's test 31.

### Restore

`silo restore [computer] <time>` sends one `restore` request. The engine carries it out as an
operation of explicit steps, as `setup`:

| Step | What happens | After a crash |
|---|---|---|
| 1. Check | The checkpoint exists and is whole; nothing else runs on this computer | |
| 2. Stop | `stop` with 30 seconds' grace | Seen as it is, and stopped again if needed |
| 3. Save | A checkpoint of the stopped disk, reason `before-restore`, kept until you remove it. If it fails, as on a full disk, the restore stops here and says why; `silo restore --no-save` is your choice to go on without it (`restore-saves-first`) | Taken again |
| 4. Prepare | `live.new/` from clones of the checkpoint's `disk.img` and `aux.img`, with `live/`'s `machine-id.bin`, `hardware-model.bin` and `config.json` | `live.new/` removed and made again |
| 5. Swap | `live.new/` and `live/` swapped in one call (`renamex_np`, `RENAME_SWAP`) | Read from which of the two holds the checkpoint's disk |
| 6. Clear | the old disk in `live.new/` removed | Done again |
| 7. Start | if the computer was running before step 2 | Reconciled as any start |

- The save is taken after the stop, so it holds what the agents wrote up to the end (#352 file
  07).
- The key Silo uses to reach main never changes, so `ssh main` works after any restore (step 2).
  A file of a grant kept inside the computer, if one is ever added, is written again before step 7
  (`what-a-restore-leaves`, as #352 file 07 proposes).
- The computer's `parent` becomes the checkpoint restored to. The next checkpoint descends from it,
  and step 5 compares with it, not with the checkpoint before in time (#352 file 08).
- A restore puts back an old login file. Codex documents that as a reason to sign in again, and
  #359's test 02 checks both programs after a restore. Until it does, the restore's last lines say
  you may have to sign in again.

### What the core holds for a checkpoint

| Field | Values |
|---|---|
| `id` | from the counter, also the folder's name |
| `computer` | |
| `taken` | wall time and awake time |
| `reason` | as above |
| `parent` | the checkpoint the computer's disk descends from when it was taken |
| `boot` | the computer's boot at that moment |
| `state` | `taking`, `kept`, `removing` |
| `kept_by` | none, or the first of: `you`, `save`, `uncompared`, `newest`, `alert` (later) |
| `compared` | step 5 |

### The record of step 4

Control facts: each checkpoint taken, with its reason, its time to make and the longest pause of
main when there was one; each skipped or refused with its reason; each removed, by thinning or by
you; the floor crossed and the reserve deleted; each step of a restore, with you as its author.

## What happens on a crash

| What stops | When | What happens |
|---|---|---|
| The engine | while a checkpoint is made | `.tmp-<id>/` is removed at start. The core sees the effect timed out, observes no checkpoint, and the next interval takes one |
| The engine | during a restore | The operation goes on from its step, as the table above says |
| The helper | during a pause for a clone | Main stops with it, as in step 2. The clone is still a moment of a stopped disk |
| The Mac | power cut during a checkpoint | `.tmp-<id>/` removed. A checkpoint renamed before the cut was flushed first |
| The volume | full | As "Space" says |

## Cost targets

| Measure | Target |
|---|---|
| The longest a write in main waits because of a checkpoint (`speed-budgets`, as #352 file 07 names it) | 100 ms at the 99th percentile, after a week of checkpoints. #352 proposes it; the number is polarzero's |
| Main paused for a clone, if test 04 needs it | the same 100 ms |
| A checkpoint, from due to renamed | under 5 s |
| A restore, from `y` to SSH answering | under 2 minutes |
| A removal by thinning | no write in main waits longer than 100 ms |

## Tests

| Test | Checks | Where |
|---|---|---|
| #365's catalog crash test, on the engine's catalog driver: killed at each of its first 300 steps, then recovered | A checkpoint is whole or absent; disk and auxiliary storage of one moment; no id twice | CI on Linux, and polarzero's Mac with `F_FULLFSYNC` and `RENAME_SWAP` |
| The same with the two planted bugs of #365 (two renames instead of one swap; a checkpoint built in place) | The test catches each | CI |
| Thinning over random histories, run often, rarely and after crashes | The same set is kept; a save, a checkpoint you took and an uncompared one are never removed | CI |
| The simulator with checkpoints, restores, a recorder down for days, the table full, the volume full | `crash-finishes-or-rolls-back`, `no-effect-twice`, `restore-changes-only-the-inside`, `checkpoint-never-changes` | CI |
| A full APFS volume: a 2 GB APFS disk image made with `hdiutil`, an image with a checkpoint, a reserve, a file filling the rest | Writes fail, the reserve is deleted, a restore still works | polarzero's Mac |
| #359 tests 03, 04, 15, 17, 31, 32, 33 | Clone waits on APFS, a moment in time per mode, a real computer under agent-like work, how often the newest APFS checkpoint is broken, the prototype on APFS, NBD boot, TRIM | polarzero's Mac, before step 4 |
| A restore to a checkpoint taken during `npm install` | main boots, `claude --resume` shows the conversation up to that moment | polarzero's Mac |
| #359 test 02, after a restore | Claude Code and Codex signed in, or asking again | polarzero's Mac |
| The longest write in main during checkpoints, measured with the kit's `writewait.pl` after a week of checkpoints | The cost target | polarzero's Mac |

## Done when

- the longest write in main during a checkpoint stays under its target after a week of
  checkpoints;
- a restore to a checkpoint taken during `npm install` boots, and `claude --resume` shows the
  conversation up to that moment;
- filling the volume does not stop the engine: it stops checkpoints, says so, and can still
  restore;
- the save made before a restore is never thinned.
