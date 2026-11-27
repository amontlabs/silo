# What the engine and the recorder save, and what it costs: second opinions for the first build

An agent's second opinion, 24 November 2026, on six decisions the first build needs:
`engine-saved-state`, `events-until-the-recorder`, `record-speed-cost`,
`sources-number-their-lines`, `own-cost-targets` and `measuring-the-store`. Nothing under `plan/`
was changed. Every quoted answer was read with `plan show <id>` on this date.

The first build's case, used throughout: main is a macOS computer on polarzero's Mac. The engine
takes a checkpoint every few minutes, compares it with the one before, and the recorder writes one
file line per changed file. Claude Code and Codex run inside with every check off. The engine
writes each input to its log before anything it caused leaves the engine.

| id | Recommendation | Confidence | Reason in one line |
|---|---|---|---|
| `engine-saved-state` | Take with an edit | high on the design, medium on the Mac cost | Four holes: Zig's own `sync` does not reach the drive on a Mac; a refused call can still land on disk after a failed flush; a reused log of fixed size holds old valid inputs past its end; a damaged last input is dropped without anyone knowing |
| `events-until-the-recorder` | Take with an edit | high | It holds only if the recorder forces its lines to its disk before it acknowledges, and the answer does not say so. Add a check at start that the recorder holds no number past the engine's |
| `record-speed-cost` | Take with an edit | medium | With the record off, checkpoints must still be taken, or the record gets charged for them. A build time hides an 11 s stall, so add the longest write wait inside the computer and the disk's sync mode |
| `sources-number-their-lines` | Take with an edit | high | Line 1 arriving after line 100 is both "a count that starts again" (a restart) and "a number that arrives twice" (written once). A count id on every line removes the clash, as Kafka and journald do |
| `own-cost-targets` | Take with an edit | medium | On an Apple SSD, the number of forced flushes costs more than the number of bytes, and the engine's idle disk activity has no target. GitHub's macOS runners cannot run a virtual machine, so "not merged" needs a named Mac |
| `measuring-the-store` | Take with an edit (narrowed) | medium | The first build needs one design, measured, not a comparison of several. The size and query targets do not exist yet, so "not merged" cannot apply to them until they do |

The three findings that matter most:

1. **`F_FULLFSYNC` is a choice the engine has to make itself.** Zig's standard library syncs a
   file with `fsync(2)` on macOS (verified in the source, below). That leaves the data in the
   drive's cache. Go, Rust, LMDB, PostgreSQL's `fsync_writethrough` and TigerBeetle all call
   `F_FULLFSYNC` on macOS; SQLite does only with `PRAGMA fullfsync`. Apple's own SQLite uses
   `F_BARRIERFSYNC` there instead, which orders writes but does not make them durable. Reported
   costs on Apple SSDs: about 20 ms per flush on an M1 (58 flushes a second, marcan, 2022) and a
   median of 3.1 to 3.8 ms per write on an M5 MacBook Air (2026). Linux numbers from this
   container are below, but they say nothing about a Mac. The script at the end measures it in
   about five minutes.
2. **When a write fails, stop the engine for every input, calls included.** After a failed
   flush, nobody knows which bytes reached the disk. PostgreSQL has stopped on such a failure
   since 2018 (fsyncgate). If the engine refuses the call and carries on, the next successful
   flush can make the "refused" input durable, and replaying the log then applies a call its
   caller was told never happened. `call-ids` already gives the way out: the caller gets "outcome
   unknown", and repeating the call with the same id after the restart returns its real result.
3. **A full disk is the likeliest write failure in the first build, and it comes at the worst
   moment.** main's disk image grows on the Mac's APFS volume as agents write inside. Time Machine's
   local snapshots make overwrites need new space. When the engine stops on a full disk,
   `when-the-engine-stops` says nothing can change until it is back: no restore and no deleting
   of checkpoints to free space. The engine needs a reserve of space that it alone can release,
   and it should stop taking checkpoints before the volume is full. No decision covers this yet.

---

## `engine-saved-state`: What does the engine save of its own state?

- **Recommendation:** Take with an edit. The design is the known one and fits a single
  deterministic core. Four details would make it lose or invent state on a Mac, and each one is a
  sentence to fix.
- **Confidence:** high on the design, from PostgreSQL's and TigerBeetle's sources and the fsync
  failure studies. Medium on the cost, which nobody has measured on polarzero's Mac.
- **What it decides, on one real case:** at 03:12 the engine takes a scheduled checkpoint of main.
  The driver's result, "checkpoint 41 done", is an input. The engine writes it to the log, forces it
  to the drive, applies it, and sends the control event "checkpoint 41 taken" to the recorder. At
  03:13 macOS panics. At start the engine loads the last saved state, applies the inputs after it,
  and must end up with checkpoint 41 under the same event number the recorder already wrote.

### The four holes

1. **Zig's `sync` is not a forced write on a Mac.** On Codeberg's `master`, `Io.Threaded`'s
   `fileSyncPosix` calls `posix.system.fsync`, and `std.fs.File.sync` calls `posix.fsync`. Neither
   calls `F_FULLFSYNC` (verified, source below). Apple's `fsync(2)` page says the data reaches the
   drive, "the drive itself may not physically write the data ... for quite some time and it may
   be written in an out-of-order sequence". An engine written in Zig that uses the standard
   library's sync passes every test on Linux and loses its last inputs on a Mac. The answer should
   name the call.
2. **A failed write that is refused, but still lands.** The answer says "A call whose write fails
   is refused with a named error and changes nothing." The failure can be in the flush, after the
   bytes went to the page cache. On Linux, after a failed `fsync` the dirty pages may be marked
   clean or dropped, and the error is reported once (PostgreSQL wiki, "Fsync Errors"). Rebello et
   al. (USENIX ATC 2020) found that no application they studied recovers correctly from it. Here is
   the case. The call "restore main to checkpoint 40" is written, and its flush fails. The engine
   refuses it and keeps running. The next input is written and flushed, and that flush also
   carries the bytes of the refused call, with a valid checksum. After the next restart, replay
   applies the restore, though `silo restore` printed that it was refused. What still holds is
   refusing a call before anything is written, for instance because the log has no room until
   the next save.
3. **A log of fixed size holds old inputs past its end.** The answer makes the files at their
   maximum size and reuses them after each save. After the first save, the bytes past the newest
   input are inputs of the previous round, each with a valid checksum and format version. Replay
   that reads "until the checksum fails" applies them. TigerBeetle has the same layout (a ring of
   fixed slots) and handles it by putting the operation number in every header and checking it
   against the slot (`src/vsr/journal.zig`). The fix is to give each input its own number and the
   number of the save it follows, and to stop at the first input whose number is not the next one.
4. **"Cut short at the end" cannot be told from "damaged at the end".** With one copy of each
   input, a last input whose checksum fails may be a torn write, never acknowledged, or a write
   that was acknowledged and then damaged. TigerBeetle drops a damaged input as torn only when it
   lies past the last one it can be sure of, and only within as many writes as could have been in
   flight (`torn_prepares` in `journal.zig`). Otherwise it treats the input as damaged. For Silo:
   only damage inside the last write before the stop may be dropped. The engine can also check
   this from outside. The recorder knows the highest event number it holds. If it holds one past
   the last number the replay gives back, an input that had been acknowledged was lost, and the
   start must stop with a named failure. Without that check, the engine reuses the number for a
   different event, and the recorder, which "writes a number it already holds only once", keeps
   the old line and drops the new one with no error.

### Smaller points

- **The saved state needs two places, not a rename.** Saving with "write a new file, then
  rename" relies on the rename being durable. On APFS that is not documented. Apple's DTS
  engineer, asked exactly this, answered: "strictly speaking, we can't ACTUALLY make a strong
  promise about any of this" (Apple forums, thread 849633). LMDB keeps two meta pages and
  TigerBeetle several superblock copies. They write the older copy, and at start they take the
  newest valid one. This also matches "made at that size when the engine is set up".
- **Group writes always, and never wait to group.** If the rule is that nothing the core puts out
  for an input leaves before that input is written, which `01-engine-internals.md` also proposes,
  then the loop can work like this. It takes every input waiting in the queue, applies them,
  writes them in one write, makes one forced flush, then releases their events and effects. That
  is group commit with no delay: PostgreSQL's default (`commit_delay = 0`), and TigerBeetle's
  batching. It answers "whether writes have to be grouped" with "always, at no cost", so the
  only thing left to measure is the latency.
- **The clock should not make the engine write while nothing happens.** The answer names "a
  tick of the clock" as an input. No entry says how often the clock ticks. With one tick a
  second, an idle engine forces 86,400 flushes a day to the Mac's SSD. Each one drains the whole
  drive queue (`fcntl(2)`: "drains the entire queue of the device and acts as a barrier"),
  including main's own writes. `01-engine-internals.md` proposes that the core say when it next
  needs the time, as quinn-proto does, so that a tick enters only when a deadline is due. I
  agree. The rule belongs in `engine-inputs` or `deterministic-core`, with a cost target in
  `own-cost-targets` (below).
- **`F_BARRIERFSYNC` is not enough for the engine.** Apple recommends it for most apps ("Reducing
  disk writes"). It keeps writes in order but can lose the last ones. An engine that has already
  sent an event to the recorder must not lose the input behind it, or the recorder ends up ahead
  of the engine (hole 4). The engine needs `F_FULLFSYNC`. The recorder may be able to use the
  barrier only if it acknowledges nothing before a full flush (see the next entry).

### Options

| Option | Cost | Who does it |
|---|---|---|
| Log of inputs plus saved state, `F_FULLFSYNC` per write group, stop on any write failure (recommended) | One flush per group: milliseconds on an Apple SSD, and the engine stops on a full disk | TigerBeetle (darwin: `F_FULLFSYNC`, stops on failure), PostgreSQL with `fsync_writethrough` (PANIC on failed flush) |
| As written: refuse a call whose write fails, and carry on | A refused call can come back on replay (hole 2) | Nobody I found does this on purpose. It is what PostgreSQL did before 2018 |
| `F_BARRIERFSYNC` only | Much cheaper per write, but loses the last inputs on power loss or panic, so the recorder ends up ahead of the engine | Apple's system SQLite with `fullfsync=on` (traced by Jonathan Johnson, 2022) |
| Save the whole state after every input, no log | Each save writes the whole state (fixed by `engine-limits`, perhaps megabytes) and flushes it | Small embedded stores. Wasteful here: inputs are bytes, the state is tables |

### Evidence

- Zig, Codeberg `master` (fetched 24 Nov 2026): `lib/std/Io/Threaded.zig`, `fileSyncPosix` calls
  `posix.system.fsync`. On GitHub's mirror, `lib/std/fs/File.zig` `sync` calls `posix.fsync`, and
  `lib/std/c.zig` defines `F.FULLFSYNC = 51` and `F.BARRIERFSYNC = 85`, so the constants are there
  but nothing uses them. **Verified.** https://codeberg.org/ziglang/zig/src/branch/master/lib/std/Io/Threaded.zig
- Go: `internal/poll/fd_fsync_darwin.go`, `Fsync` uses `F_FULLFSYNC` and falls back to `fsync` on
  `ENOTSUP`. **Verified.** https://github.com/golang/go/blob/master/src/internal/poll/fd_fsync_darwin.go
- Rust std: `File::sync_all` and `sync_data` on Apple targets call `fcntl(fd, F_FULLFSYNC)`.
  **Verified** (`library/std/src/sys/fs/unix.rs`).
- LMDB: `#define MDB_FDATASYNC(fd) fcntl(fd, F_FULLFSYNC)` under `__APPLE__`. **Verified**
  (`libraries/liblmdb/mdb.c`, `mdb.master`).
- TigerBeetle: `src/stdx/io/darwin.zig`, `fs_sync` calls `F_FULLFSYNC`. Its comment calls the
  fallback to `fsync` "of dubious safety" unless it is known at start that the disk lacks
  `F_FULLFSYNC`. It also preallocates with `F_PREALLOCATE`. **Verified**, commit `a2a3c706`.
  Torn-write handling in `src/vsr/journal.zig` (`torn_prepares`). **Verified.**
- PostgreSQL: `pg_fsync_writethrough` uses `F_FULLFSYNC` (`src/backend/storage/file/fd.c`). A
  failed WAL flush ends in `ereport(PANIC ...)` (`xlog.c`, "PANIC if failed to fsync"). Docs:
  "On macOS, write caching can be prevented by setting wal_sync_method to fsync_writethrough".
  The default is the first method on the list that the platform supports, so not the writethrough
  one. **Verified.** https://www.postgresql.org/docs/current/wal-reliability.html,
  https://wiki.postgresql.org/wiki/Fsync_Errors
- SQLite: `fullfsync` and `checkpoint_fullfsync` both default to off. `full_fsync()` in
  `os_unix.c` calls `F_FULLFSYNC` only when asked to, otherwise `fsync`. **Verified.**
  https://www.sqlite.org/pragma.html#pragma_fullfsync. That Apple's SQLite maps `fullfsync=on`
  to `F_BARRIERFSYNC` was seen with `dtruss` by Jonathan Johnson, https://bonsaidb.io/blog/acid-on-apple/
  (**not verified by me**).
- Apple, "Reducing disk writes": "Most apps can use the fcntl(_:_:) F_BARRIERFSYNC for this. Only
  use F_FULLFSYNC when your app requires a strong expectation of data persistence." **Verified.**
  https://developer.apple.com/documentation/xcode/reducing-disk-writes
- Apple DTS on `F_FULLFSYNC` and APFS: "F_FULLFSYNC is basically 'always' going to be relatively
  slow", and on many concurrent callers, a slowdown that grows with the number of cores (jj's test
  suite ran about 5 times slower). https://developer.apple.com/forums/thread/800906. On rename:
  https://developer.apple.com/forums/thread/849633. **Verified** (read the threads).
- Costs on Apple SSDs, **not verified by me**: marcan (Hector Martin), Feb 2022, "58 IOPS with
  F_FULLFSYNC. On internal NVMe" and "The 20ms flush penalty is there for Apple's NVMe
  implementation", https://news.ycombinator.com/item?id=30370551, summary at
  https://mjtsai.com/blog/2022/02/17/apple-ssd-benchmarks-and-f_fullsync/. M5 MacBook Air,
  macOS 27.2.1, FileVault on: median 3.1 to 3.8 ms per write with `F_FULLFSYNC`,
  https://github.com/TARUNTEjjjj/pocketkv/pull/12. Apple silicon models differ, and I found no
  published numbers for `F_BARRIERFSYNC`.
- Rebello et al., "Can Applications Recover from fsync Failures?", USENIX ATC 2020,
  https://www.usenix.org/conference/atc20/presentation/rebello. Alagappan et al.,
  "Protocol-Aware Recovery for Consensus-Based Storage", FAST 2018, on telling a torn write from
  damage, https://www.usenix.org/conference/fast18/presentation/alagappan. Pillai et al., "All
  File Systems Are Not Created Equal", OSDI 2014, on what ordering applications wrongly assume,
  https://www.usenix.org/conference/osdi14/technical-sessions/presentation/pillai. Nightingale
  et al., "Rethink the Sync", OSDI 2006, on holding outputs until the write is durable,
  https://www.usenix.org/legacy/event/osdi06/tech/nightingale.html. **Pages verified to exist.
  Content from memory.**

### Measured here, on Linux

These are **Linux numbers**, from this container: 4 vCPU Xeon, ext4 on a virtio disk, and XFS on
a loop device on that disk. The disk is virtual and its host cache is unknown, so a flush here may
not reach any physical medium. They show how costs are shaped, not what a Mac costs. The program
writes one record, then flushes, repeatedly. The figures are medians over three interleaved
rounds, in microseconds per flush.

| Filesystem, file | Call | Record | p50 | p99 | max | records/s |
|---|---|---|---|---|---|---|
| ext4, appended | `fdatasync` | 128 B | 152 | 4,201 | 17,048 | 3,379 |
| ext4, preallocated | `fdatasync` | 128 B | 104 | 1,027 | 24,610 | 4,821 |
| ext4, preallocated | `fdatasync`, 8 per flush | 128 B | 97 | 503 | 4,282 | 72,393 |
| ext4, preallocated | `fdatasync`, 64 per flush | 128 B | 103 | 192 | 489 | 612,176 |
| XFS, appended | `fdatasync` | 128 B | 759 | 9,132 | 114,145 | 906 |
| XFS, preallocated | `fdatasync` | 128 B | 272 | 1,494 | 4,571 | 3,643 |
| ext4, preallocated, another process writing 3 GB | `fdatasync` | 128 B | 153 to 164 | 34,435 to 92,581 | 411,772 to 492,985 | 331 to 475 |
| ext4, preallocated, another process writing 3 GB | `fdatasync`, 64 per flush | 128 B | 160 to 183 | 187,095 to 295,829 | 931,286 to 1,065,217 | 9,186 to 10,446 |

What they show:

1. Grouping is nearly free. On ext4, 64 records per flush cost about what 1 did, so the
   throughput grew about 127 times.
2. Preallocating helped on both filesystems, most of all on XFS (p50 759 to 272 µs), because an
   overwrite does not change the file's size and so needs no metadata in the flush.
3. **Another writer on the same disk sets the tail.** While a second process wrote 3 GB, the p99
   of one flush rose about 10 to 30 times, and the worst flush took up to about 1 s. On a Mac,
   main's disk is that other writer, and `F_FULLFSYNC` drains the whole drive queue by design. So
   the engine's write latency will depend on what the agents are doing inside main. It has to be
   measured with main busy, and the core must never wait on it (which `core-never-waits` already
   asks).

### Risks and what would change this

- If `F_FULLFSYNC` on polarzero's Mac has a p99 over about 50 ms while main writes, a `silo`
  command waits that long for each call. That is still acceptable, because calls are rare. Ticks
  must then not be inputs once a second (above), or they fill the queue.
- If the Mac test shows that an overwrite of a preallocated file takes no new space while it shares
  no blocks, and new space once a clone exists, then preallocating does not protect against a
  full disk while a Time Machine local snapshot exists. The reserve (cross-cutting finding 2) then
  becomes needed, not optional.
- Moving from "refuse and carry on" to "stop" is something a user sees: one more case where
  `silo` prints "the engine stopped". By `owners-part-on-engine`, that makes this edit
  polarzero's to take, not the agents'.

### Proposed text

answer: The state, and after it every input accepted since that save. This is a write-ahead log,
the known practice. Nothing the core puts out for an input, an event, an effect or an answer,
leaves the engine before that input is written through to the disk. Written through means the
drive holds it, not only the system: on a Mac the engine calls `fcntl` with `F_FULLFSYNC`, since
`fsync` there leaves the data in the drive's cache, and on Linux it calls `fdatasync`. Each write
takes every input waiting at that moment, with one flush for all of them, and never waits for
more. The saved state has two places of fixed size, written in turn, and at start the newer one
whose checksum holds is used. A new save is made when the inputs since the last one reach a fixed
count, so the files have a fixed maximum size. They are made at that size when the engine is set
up, so that on a filesystem that writes in place a full disk does not stop a write to them. On a
copy-on-write filesystem, such as APFS while the volume has a snapshot, an overwrite can still
need new space; whether it does on a Mac is to find out. Each input carries its number, the
number of the save it follows, a checksum and a format version. At start the engine loads the
newer saved state and applies the inputs after it, in order, and stops at the first input whose
number is not the next one. The core is deterministic, so this gives the state it had, with the
same events under the same numbers. Applying the inputs again issues no effect: what the drivers
did is learned by reconciling at start, as `multi-step-operations` says. A damaged input inside
the last write before a stop was never accepted, and is dropped. Any other damage stops the start
with a named failure: the engine does not guess a state. When the recorder holds an event number
past the last one the inputs give back, an accepted input was lost, and the start stops with a
named failure too. A call that cannot be written because the log has no room is refused with a
named error and changes nothing. When a write or a flush fails, the engine cannot know what
reached the disk, so it stops with a named failure, whatever the input was. A call caught in that
write gets an error saying that its outcome is unknown. Repeating it with the same id after the
restart gives its real result, as `call-ids` says. To find out: what it then starts from; what a
newer engine does with an older format; and what a write costs on a Mac, at rest and while a
computer writes to the same disk, which is measured.

### For the first build

It blocks `engine-program-and-clients` and everything after it. Nothing in it can be narrowed
away: the first build's promise is "lose nothing", and this entry is where the engine keeps it.
The builder needs: the flush call (named above), the layout of an input's header (number, save
number, length, checksum, format version), the two state places, and the stop on failure. With
that, the "to find out" on cost does not block the build. The Mac script below measures it, and
grouping makes the result change only the latency, never the design.

---

## `events-until-the-recorder`: Where are the engine's events kept until the recorder has them?

- **Recommendation:** Take with an edit. An outbox kept in the saved state is the right
  structure. It holds only if both ends force their writes before they speak, and the answer
  says so only for the engine's end.
- **Confidence:** high. It follows from the answer's own sentence: "the recorder writes a number
  it already holds only once."
- **What it decides, on one real case:** the recorder receives "checkpoint 41 taken" as event 9,012,
  writes it to its file, and acknowledges 9,012. The engine frees its table up to 9,012. Then the
  Mac loses power. If the recorder acknowledged before its line was forced to the SSD, event 9,012
  is gone from both: from the engine, because it was freed, and from the recorder, because it was
  still in the drive's cache. There is no gap line, since neither side knows. That is the
  "control facts never have a gap" aim of the notes, broken without any trace.

### Options

| Option | Cost | Who does it |
|---|---|---|
| Outbox in the saved state, the recorder acknowledges only lines it has forced to disk (recommended) | One forced flush in the recorder per batch, and one input in the engine per acknowledgement | Transactional outbox pattern. Kafka: the broker acknowledges with `acks=all` only after the write is replicated. TigerBeetle replies only after commit |
| As written | The same, but the recorder's flush is left to the builder | |
| A journal of its own for events | A second log to flush and keep in step | The answer already rejects it, rightly |

### Evidence

- The answer itself: replay "gives every event back under its number", and "the recorder writes
  a number it already holds only once". Together these two sentences mean the recorder must never
  hold a number that the engine can give back with a different event. That holds only if the
  engine's flush comes before an event leaves (proposed in `engine-saved-state`) and the
  recorder's flush comes before it acknowledges (proposed here). **Reasoning, not a measurement.**
- Every acknowledgement is an input (`engine-inputs`), so it costs one engine flush, inside a
  group. With checkpoints every few minutes, the first build has few events a minute. The rate
  is set by the clock (`engine-saved-state` above), not by events.
- Kafka's producer `acks` and the transactional outbox are from memory, **not verified here**.

### Risks and what would change this

- If the recorder is down for long, the table fills. Then a call that would leave less room is
  refused, including `silo restore`. That is right by `enforcing-and-recording`, but in the first
  build the recorder is a process of Silo's own with nothing hostile feeding it. Size the table so
  that a day of the first build's control events fits, and have the engine restart the recorder.
  Nothing in the plan says who restarts it. `processes` says only that the others keep running.
- If the recorder later uses `F_BARRIERFSYNC` for its line files, which may be enough for lines
  that carry nothing to acknowledge, it must still use `F_FULLFSYNC` before an acknowledgement.

### Proposed text

answer: In the engine's saved state, as `engine-saved-state` says, and in no journal of their
own. The events the recorder has not acknowledged are a table of fixed size in the core's state,
numbered as `numbered-events` says. An event counts as put out once the input it came from is
written, since applying the saved inputs again gives every event back under its number. The
engine sends an event only once that input is written through. The recorder acknowledges the
highest number whose line it has written through to its disk in the same way, and never a number
it holds only in memory. The acknowledgement reaches the core as an input, and the core frees the
table up to there. After a crash of either one, the engine sends again from the last acknowledged
number, and the recorder writes a number it already holds only once. When the recorder holds a
number past the last one the engine gives back, the engine does not start, as `engine-saved-state`
says. The table holds only what the recorder has not written yet, so it is not history. The core
keeps room in it for what the driver results it may still receive would add: for each effect
still under way, and for each running computer, which can stop on its own. A call that would leave
less room is refused with a named error before the core applies it, and changes nothing, which is
what `enforcing-and-recording` asks. An operation the clock starts, such as a checkpoint taken at
an interval, starts only when the room left after it still holds what is kept for the rest;
otherwise it starts at the first tick that has room. A write that fails is handled as
`engine-saved-state` says. This is the known practice of an outbox kept with the state it comes
from. To find out: what forcing each write to disk costs on a Mac, which the measurement of
`engine-saved-state` gives.

### For the first build

It blocks `record-of-a-computer`. The edit adds no work: the recorder needs a flush before each
acknowledgement anyway. The builder needs the size of the table (one day of control events is
small: a few events per checkpoint, at one checkpoint every few minutes, is a few thousand rows).

---

## `record-speed-cost`: What does keeping the record cost in speed?

- **Recommendation:** Take with an edit. Measuring each source apart is right. Three things are
  missing, and without them the measurement can pass while the user feels the cost.
- **Confidence:** medium. It rests on PR #347's XFS stall and on how Apple's disk modes are
  documented. No macOS computer has been measured.
- **What it decides, on one real case:** in the first build, the only source of lines that costs
  time is the comparison of two checkpoints (`file-changes-from-outside`). A build of 8 jobs runs
  in main. At minute 3 a checkpoint is taken, then compared with the one before.

### What is missing

1. **"Record off" must still take checkpoints.** Checkpoints exist for restore, not for the
   record. If "record off" means "no checkpoints", the record is charged for the clone, which
   belongs to `speed-budgets`' checkpoint budget. If it means "no comparison", it measures only the
   comparison. The answer must say which. The second is the one that isolates the record.
2. **A build time hides a stall.** PR #347 measured, on XFS, an 11 to 23 s stop of the computer's
   writes during a reflink copy of a disk with about 500,000 extents. In a 10-minute build, one such
   stop adds about 2 to 4 percent, and could pass the 5 percent of `record-overhead-limit`. Yet a
   person feels a 15 s freeze of the shell, and `observing-speed` (taken) says observing must not
   slow a computer "in a way a person can feel". Each run should also give the longest wait of
   one small write inside the computer. A tiny program inside writes 4 KB and flushes every
   100 ms, and logs the time. That program is part of the measurement, not part of Silo.
3. **The disk's sync mode changes the baseline.** `VZDiskImageSynchronizationMode.full` "ensures
   the data moves from the disk's internal cache to permanent storage", and `.fsync` "doesn't
   ensure" it (Apple docs, verified). Tart defaults to `.full`. Lima uses `.fsync` with the
   `cached` caching mode, after disk corruption with the automatic mode (UTM issue 4840). In
   `.full` mode, a guest flush costs an Apple SSD flush, milliseconds each. So "a build in a macOS
   computer" differs a lot between modes, and the engine's own flushes add to the same queue.
   Each measurement must name the mode, or two runs cannot be compared.

### Options

| Option | Cost | Who does it |
|---|---|---|
| Each source apart, record off still takes checkpoints, the longest write wait and the disk mode named (recommended) | One small program inside the computer for the measurement | fio-style latency percentiles in storage benchmarks. Firecracker's and Kata's performance tests report p99 I/O latency, not only throughput (from memory, not verified) |
| As written | Can pass while the user sees freezes | |
| One number for the whole record | Rejected in the notes, rightly | |

### Evidence

- PR #347 and `file-changes-from-outside`: builds 1.6 to 2.1 times slower when the engine served
  the folder, and no slower when disks were compared at checkpoints (as quoted in the plan).
- Apple, `VZDiskImageSynchronizationMode.full`, `.fsync` and `.none`.
  **Verified.** https://developer.apple.com/documentation/virtualization/vzdiskimagesynchronizationmode
- Tart, `Sources/tart/VM.swift`: `sync: VZDiskImageSynchronizationMode = .full`. Lima,
  `pkg/driver/vz/vm_darwin.go`: `DiskImageCachingModeCached` and `DiskImageSynchronizationModeFsync`,
  with a comment linking https://github.com/utmapp/UTM/issues/4840. **Verified** (read the source).
- That `.full` costs an `F_FULLFSYNC` for each guest flush is **inferred** from Apple's wording,
  not measured.

### Risks and what would change this

If the macOS computer's checkpoint is an APFS clone that does not stall writes (Mac test 1 of the
first-build proposal), the stall metric reads near zero and costs nothing to keep. If it does
stall, this metric is the one that shows it.

### Proposed text

answer: To find out by measuring, the way `speed-budgets` says: a build and a parallel test suite,
in a macOS computer and in a Linux computer, each with the record on and with it off, and on a
plain machine, as distributions over repeated interleaved runs. With the record off, checkpoints
are still taken at the same interval, so that what a checkpoint costs is not counted as the cost
of the record. Each run gives its time and the longest wait of one small write inside the
computer, since a stop of a few seconds can hide in the time of a long build. Each run names the
Mac, the version of macOS, and the caching and synchronization modes of the computer's disk. The
cost is given for each source apart, because the trials of `file-changes-from-outside` show that
the route for file changes decides most of it: a build ran 1.6 to 2.1 times slower when the engine
served the folder, and no slower when disks were compared at checkpoints. No trial has measured a
macOS computer yet. The numbers go in this entry's notes, and `record-overhead-limit` is checked
against them.

### For the first build

It does not block building, only checking `record-overhead-limit`. Narrowed to the first build,
there is one source to measure (the checkpoint comparison) and one computer (macOS). The Linux
computer can wait for the third build. The builder needs the write-wait program and the mode
named in the computer's driver.

---

## `sources-number-their-lines`: How does Silo know that lines are missing?

- **Recommendation:** Take with an edit. Two of its sentences contradict each other on one case,
  and an id on each count resolves it.
- **Confidence:** high. The contradiction is in the text, and the fix is what Kafka and journald
  do.
- **What it decides, on one real case:** the comparer that writes file lines has sent lines 1 to
  100 for main. It crashes and is started again by the engine, which hands it the same pair of
  checkpoints to compare. It sends line 1 again, without saying it restarted, because it
  crashed. The answer says "a count that starts again without saying so is read as a restart",
  and also "a number that arrives twice is written once". Line 1 is both. Read as a duplicate,
  the second run's lines 1 to 100 are dropped, or reported as differing from the first run's. Read
  as a restart, a real duplicate on the channel after a reconnect is recorded as a restart that
  never happened. The recorder cannot tell which from numbers alone.

### Options

| Option | Cost | Who does it |
|---|---|---|
| Each count has an id given in the handshake, and every line carries it with its number (recommended) | 8 to 16 bytes per line before compression, much less after, since the id repeats | Kafka's idempotent producer: producer id, epoch and sequence (KIP-98). journald: a random 128-bit `seqnum_id`, with `seqnum` counting up from 1 under it |
| As written | The clash above | |
| Numbers that never restart, kept on disk by each source | A forced write per line in each source, which `observing-and-waiting` would not allow | Some log shippers persist offsets. Too costly here |

### Evidence

- journald file format: "All entries carry sequence numbers that are monotonically counted up for
  each entry (starting at 1)", and `seqnum_id` "is randomly generated when the journal daemon
  creates its first file". **Verified.** https://systemd.io/JOURNAL_FILE_FORMAT/
- Kafka KIP-98: a producer id, an epoch, and a sequence per partition. The broker refuses a
  sequence past the expected one as out of order. KIP-360 shows why resetting a sequence without
  a new epoch is unsafe. **Read in search summaries, not in full.**
  https://cwiki.apache.org/confluence/display/KAFKA/KIP-98+-+Exactly+Once+Delivery+and+Transactional+Messaging
- The handshake already exists in the plan: the notes of `events-until-the-recorder` say "Nothing
  crosses before the handshake". If the recorder gives the id, a source cannot reuse an old id,
  even by a bug.

### Risks and what would change this

None that I can see in the first build: its sources (the engine's events and the comparer) are
Silo's own processes on the Mac. For the probe later: a hostile probe can still lie about
anything it reports. An id given by the recorder only makes a restart impossible to hide by
reusing an id. That fits `what-a-report-carries`.

### Proposed text

answer: Every source numbers what it writes, before sending, and sends its lines in order on one
channel. Each count has an id, which the recorder gives the source in the handshake when the count
starts, and every line carries that id with its number. A missing number in a count is a gap,
which the recorder writes as a fact, since it saw the hole itself. A source that restarts starts a
new count, under a new id. A line under a new id is a restart, whether or not the source said so.
The recorder writes the restart as a fact, with the last number of the old count it received:
lines made after that one and lost in the restart cannot be counted, so a restart is never read as
a count with no loss. A source with nothing to say sends its last number at a fixed interval, so
that a loss at the end shows. When a source sends nothing for longer than that, the recorder
writes that it fell silent, as a fact of the kind "silence", and not a gap: a silence loses
nothing by itself, as when a computer is paused or the Mac sleeps. The next line settles it: a
number past the expected one is a gap, and a new id is a restart. A number that arrives twice
under the same id is written once; when the two lines under it differ, the recorder writes that
too, as a fact.

### For the first build

It blocks `record-of-a-computer`. Taken with the edit, it is small to build: one id and one
counter per source, and one table of the last number per id in the recorder.

---

## `own-cost-targets`: Which costs of Silo's own processes have a target?

- **Recommendation:** Take with an edit. Targets that hold are right, and polarzero asked for them.
  Three gaps: what costs most on a Mac has no target; there is no reference machine; and no
  hosted CI can run the Mac part.
- **Confidence:** medium. The Apple SSD flush cost is reported, not measured here. The CI limit is
  from community reports.
- **What it decides, on one real case:** polarzero's Mac sits idle overnight, with main paused.
  The engine, the helper and the recorder run. With a tick of the clock every second written
  through, the engine forces 86,400 flushes of the whole SSD queue a day, and no target in the
  list covers it. The list names "memory and processor of the engine and of the app at idle", not
  disk.

### What to add

1. **Forced flushes, counted apart from bytes.** At 3 to 20 ms each on an Apple SSD, and draining
   the whole drive queue, the number of flushes a process makes per second matters more than the
   bytes it writes. Add it for the engine (idle, and during a large build) and the recorder
   (during a large build). The jj case on the Apple forums (a test suite about 5 times slower
   once it called `F_FULLFSYNC`, worse with more cores) shows what an unbudgeted flush costs.
2. **The engine's disk writes at idle**, beside its memory and processor.
3. **A named reference machine.** The answer says "on a named reference machine", but no entry
   names one. For the first build, the natural one is polarzero's Mac. The model, the memory,
   the version of macOS and whether FileVault is on should go in the notes, since flush costs
   differ between Apple SSDs.
4. **Where the Mac measurements run.** GitHub's hosted arm64 macOS runners cannot start a
   virtual machine (no nested virtualization, per runner-images issues 9254 and 9460 and Apple
   DTS). Apple DTS confirms nested virtualization is not supported for macOS guests. So "a change
   that makes a measurement miss its target is not merged" can be enforced for the targets that
   run on Linux in CI. The ones on a Mac need a Mac that runs them before merging: polarzero's,
   or a dedicated one.

### Options

| Option | Cost | Who does it |
|---|---|---|
| Targets that hold, with flushes, idle disk and a named Mac (recommended) | Someone runs the Mac benchmarks before a merge that touches a measured process | TigerBeetle runs its benchmark on fixed hardware and tracks it per commit (devhub). From memory, not verified here |
| As written | The costliest Mac operation has no target, and the gate cannot run where the code merges | |
| Targets as a guide | Rejected in the notes, for a reason polarzero gave | |

### Evidence

- `fcntl(2)` on macOS, `F_FULLFSYNC`: "drains the entire queue of the device and acts as a
  barrier ... The operation may take quite a while to complete." **Verified** (man page text).
- Apple forums thread 800906 (jj): **verified**, read.
- Hosted macOS arm64 runners lack nested virtualization:
  https://github.com/actions/runner-images/issues/9254,
  https://github.com/actions/runner-images/issues/9460. **Community reports, not GitHub's own
  documentation, and some years old. Check before relying on it.**

### Proposed text

answer: Each process Silo adds has a target for what it costs, written before its code in the
entry that adds the process, as a 99th percentile and a worst case on a named reference machine: a
helper's memory and processor, idle and under load; the wait the boundary adds to one connection
and to one model call; the recorder's processor, memory, disk writes and forced flushes during a
large build; the probe's processor and memory inside a computer during that build; the time the
engine takes to start; the memory, processor, disk writes and forced flushes of the engine at idle
and during that build; and the memory and processor of the app at idle. The reference machine is
named in this entry's notes, with its model, memory and version of macOS. A benchmark measures
each one as `how-speed-is-measured` says, and a change that makes a measurement miss its target is
not merged. A measurement that needs a Mac runs on the reference machine before the change is
merged, since hosted macOS machines cannot start a virtual machine. When a target cannot be met
without the user seeing less or waiting longer, that choice goes to polarzero, and the target is
not raised quietly.

### For the first build

The answer already puts each target in the entry that adds the process, so the boundary and the
probe do not block anything. The first build needs targets for the engine, the helper and the
recorder, and the reference machine named. The Mac script below gives the first number for the
engine's flush.

---

## `measuring-the-store`: How is the store of the record designed?

- **Recommendation:** Take with an edit, narrowed. Choosing by measuring is right. For the first
  build it should ship one design and measure it, and compare others only when a target is
  missed. Two targets it gates on do not exist yet.
- **Confidence:** medium. This is a judgment about scope, plus one fact about APFS sizes.
- **What it decides, on one real case:** an agent runs `npm install` in main and adds 60,000 files
  to `node_modules`. The next checkpoint comparison produces about 60,000 file lines in one burst.
  The store writes them, compressed, and `silo changes main --since 41` must find them without
  reading the whole record.

### Points

1. **One design first.** The first build's record holds control lines and file lines, a few
   kinds, with bursts at each checkpoint. A comparison of candidates (cut sizes, compressors,
   indexes) on "the same recorded load, a large build among them" needs a load that does not
   exist until the first build runs. A design that follows `record-file-format` and is known to
   work: one append-only file per computer, cut at a fixed size, compressed in independent blocks
   (for instance zstd frames, which zstd's seekable format uses), and an index with each block's
   first line number, first time and the checkpoint marks it holds. The index sits in a small
   file that can be rebuilt from the blocks. journald's entry arrays and Prometheus's chunks
   follow the same idea. **From memory, not verified here.**
2. **Targets that do not exist cannot gate a merge.** "A change to the store that misses a target
   is not merged. The targets for size and for queries are polarzero's to set." Until polarzero
   sets them, which `record-disk-size` says needs days of real use first, the gate on size and
   queries is empty. Say that these two are measured and shown until they are set.
3. **Size means blocks, not length.** On APFS a file's length and the space it takes differ:
   clones share blocks, and a sparse or preallocated file takes space unlike its length. What the
   user loses on their disk is the space the files take (`st_blocks`). The running total should
   count that. Otherwise the limit `record-disk-size` informs is off by whatever preallocation
   adds.
4. **Forced flushes again.** The recorder flushes before it acknowledges
   (`events-until-the-recorder`). The store's design decides how many lines one flush covers. It
   is one of the costs the store is measured on.

### Options

| Option | Cost | Who does it |
|---|---|---|
| One design for the first build, measured on the targets, others tried when a target is missed (recommended) | A redesign later if the first misses | journald, Loki, Prometheus TSDB all started with one format and versioned it (`formats-carry-versions` covers a later change) |
| As written: compare candidates first | Work before there is a real load to compare on | |

### Proposed text

answer: The form of the store is settled by `record-file-format`: append-only files, compressed,
with an index. The first build ships one design, the simplest that follows it, and measures it on
the recorder's targets from `own-cost-targets`: processor, memory, disk writes and forced flushes,
on a recorded load with a large build among it. Another design is tried when a measurement
misses a target, on the same load. The store is also measured on what it serves: the bytes it
keeps per day, which `record-disk-size` measures, and the time of the queries the app and the
`silo` command make, such as opening an alert on what triggered it. The size on disk counts the
space the files take, not their length. It is kept as a running total: at start it is taken from
the files, then each write adds what it took, and it is never found by reading the lines. A change
to the store that misses a target is not merged. The targets for size and for queries are
polarzero's to set once `record-disk-size` has numbers. Until then those two are measured and
shown, and hold no change back.

### For the first build

It blocks `record-of-a-computer`. Narrowed, the builder needs the cut size, the block size and the
index fields, and agents can choose these on the first measurement. polarzero's part is only
the two targets, later.

---

## A benchmark for polarzero's Mac, about 5 minutes

It measures what one forced write costs: `fsync`, `F_BARRIERFSYNC` and `F_FULLFSYNC`, records of
128 bytes and 4 KB, appended and preallocated, 1, 8 and 64 records per flush, in three
interleaved rounds. Then it measures again while another process writes to the same disk, as
main's disk would. Last, it checks whether overwriting a file in place takes new space once a
clone shares its blocks (the clone stands in for a Time Machine local snapshot). The question of
`engine-saved-state` is "whether it does on a Mac". It needs the Command Line Tools
(`xcode-select --install`) for `cc`, and about 2 GB of free space. It writes only in
`~/Library/Caches` and its own temporary folder, and deletes both. Run it on mains power, with
other apps closed. It was run here on Linux, where `F_BARRIERFSYNC` and `F_FULLFSYNC` fall back to
`fdatasync`, to check that it runs. The Mac branches have not been run.

```bash
#!/bin/bash
# What a forced write costs on this Mac. About 5 minutes. Needs the Command Line Tools (cc).
# Writes nothing outside a temporary folder, which it deletes. Prints a summary and keeps the CSV.
set -euo pipefail
OUT="$HOME/silo-fsync-$(date +%Y%m%d-%H%M%S).csv"
W=$(mktemp -d "${TMPDIR:-/tmp}/silo-fsync.XXXXXX")
DIR="$HOME/Library/Caches/silo-fsync.$$"; [ "$(uname)" = Darwin ] || DIR="$W/data"
mkdir -p "$DIR"; trap 'rm -rf "$W" "$DIR"' EXIT
cat > "$W/b.c" <<'EOF'
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <time.h>
#ifndef F_FULLFSYNC
#define F_FULLFSYNC 51
#endif
#ifndef F_BARRIERFSYNC
#define F_BARRIERFSYNC 85
#endif
static double us(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec*1e6+t.tv_nsec/1e3;}
static int cmp(const void*a,const void*b){double x=*(const double*)a,y=*(const double*)b;return x<y?-1:x>y;}
static int flush(int fd,const char*m){
  if(!strcmp(m,"none"))return 0;
  if(!strcmp(m,"fsync"))return fsync(fd);
#ifdef __APPLE__
  if(!strcmp(m,"barrier"))return fcntl(fd,F_BARRIERFSYNC);
  if(!strcmp(m,"full"))return fcntl(fd,F_FULLFSYNC);
#else
  if(!strcmp(m,"barrier")||!strcmp(m,"full"))return fdatasync(fd);
#endif
  return -1;}
int main(int c,char**v){
  /* b <dir> <mode> <record bytes> <records per flush> <flushes> <preallocated 0|1> */
  if(c<7)return 2;
  const char*m=v[2];int rs=atoi(v[3]),bt=atoi(v[4]),n=atoi(v[5]),pre=atoi(v[6]);
  char p[4096];snprintf(p,sizeof p,"%s/log.%d",v[1],getpid());
  int fd=open(p,O_RDWR|O_CREAT|O_TRUNC,0600);if(fd<0){perror("open");return 1;}
  long tot=(long)rs*bt*n;size_t chunk=1<<20;
  if(pre){char*z=calloc(1,chunk);for(long o=0;o<tot;o+=chunk){size_t k=(size_t)((tot-o)<(long)chunk?(tot-o):(long)chunk);if(write(fd,z,k)!=(ssize_t)k){perror("prealloc");return 1;}}
    if(flush(fd,"full")){perror("flush");return 1;}lseek(fd,0,SEEK_SET);free(z);}
  size_t len=(size_t)rs*bt;char*b=malloc(len);memset(b,'x',len);double*l=malloc(sizeof(double)*n);
  double t0=us();
  for(int i=0;i<n;i++){double a=us();if(write(fd,b,len)!=(ssize_t)len){perror("write");return 1;}
    if(flush(fd,m)){perror(m);return 1;}l[i]=us()-a;}
  double el=us()-t0;qsort(l,n,sizeof(double),cmp);
  printf("%s,%d,%d,%d,%d,%.0f,%.0f,%.0f,%.0f\n",m,rs,bt,pre,n,l[n/2],l[(int)(n*0.99)],l[n-1],n*bt/(el/1e6));
  close(fd);unlink(p);return 0;}
EOF
cc -O2 -o "$W/b" "$W/b.c"
B="$W/b"
{
echo "# $(date -u +%FT%TZ)"
if [ "$(uname)" = Darwin ]; then
  echo "# model $(sysctl -n hw.model) chip $(sysctl -n machdep.cpu.brand_string) mem $(( $(sysctl -n hw.memsize) / 1073741824 ))GB"
  echo "# macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  echo "# disk $(diskutil info / | awk -F: '/Device \/ Media Name/{gsub(/^ +/,"",$2);print $2}') filevault $(fdesetup status | head -1)"
  echo "# power $(pmset -g batt | head -1 | sed "s/.*'\(.*\)'.*/\1/") lowpower $(pmset -g | awk '/lowpowermode/{print $2}')"
  echo "# local snapshots on /: $(tmutil listlocalsnapshots / 2>/dev/null | grep -c com.apple || true)"
fi
echo "mode,record_bytes,records_per_flush,preallocated,flushes,p50_us,p99_us,max_us,records_per_s,round,load"
} > "$OUT"
run(){ echo "$("$B" "$DIR" "$@"),$ROUND,$LOAD" >> "$OUT"; }
LOAD=quiet
for ROUND in 1 2 3; do
  echo "round $ROUND of 3, quiet disk" >&2
  for pre in 1 0; do for rs in 128 4096; do
    run none $rs 1 2000 $pre; run fsync $rs 1 1000 $pre; run barrier $rs 1 1000 $pre; run full $rs 1 300 $pre
  done; done
  run full 128 8 300 1; run full 128 64 300 1
done
echo "under load: another process keeps writing 1 GB files to the same disk, as a computer's disk would" >&2
LOAD=writer; ROUND=1
( while :; do dd if=/dev/zero of="$DIR/load.img" bs=1m count=1024 2>/dev/null; done ) & BG=$!
sleep 1; run full 128 1 300 1; run barrier 128 1 1000 1; run fsync 128 1 1000 1; run full 128 64 100 1
pkill -P $BG 2>/dev/null || true; kill $BG 2>/dev/null || true; wait $BG 2>/dev/null || true; rm -f "$DIR/load.img"
# Does overwriting a file in place take new space once it shares blocks with a clone?
# (A clone stands in for a snapshot of the volume.) Avail is read with df, in KB.
if [ "$(uname)" = Darwin ]; then
  echo "overwrite test: 512 MB file, with and without a clone" >&2
  f="$DIR/pre.img"; mib=512
  avail(){ sync; sleep 2; df -k "$DIR" | awk 'NR==2{print $4}'; }
  dd if=/dev/urandom of="$f" bs=1m count=$mib 2>/dev/null; a0=$(avail)
  dd if=/dev/urandom of="$f" bs=1m count=$mib conv=notrunc 2>/dev/null; a1=$(avail)
  cp -c "$f" "$f.clone"; a2=$(avail)
  dd if=/dev/urandom of="$f" bs=1m count=$mib conv=notrunc 2>/dev/null; a3=$(avail)
  echo "# overwrite_unshared_used_MB $(( (a0-a1)/1024 )) clone_used_MB $(( (a1-a2)/1024 )) overwrite_after_clone_used_MB $(( (a2-a3)/1024 )) (file is $mib MB)" >> "$OUT"
  rm -f "$f" "$f.clone"
fi
echo; echo "Saved: $OUT"; echo
grep '^#' "$OUT"
echo; echo "Median over rounds of p50 / p99 / max, in microseconds, per flush:"
grep -v '^#' "$OUT" | tail -n +2 | sort -t, -k11,11 -k1,1 -k2,2n -k3,3n -k4,4n | awk -F, '
 {k=$11" "$1" rec="$2" per_flush="$3" prealloc="$4; n[k]++; p50[k]=p50[k]" "$6; p99[k]=p99[k]" "$7; mx[k]=mx[k]" "$8; r[k]=r[k]" "$9; if(!(k in o)){o[k]=++c; ks[c]=k}}
 function med(s,  a,m,i,j,t){m=split(s,a," ");for(i=1;i<=m;i++)for(j=i+1;j<=m;j++)if(a[j]+0<a[i]+0){t=a[i];a[i]=a[j];a[j]=t};return a[int((m+1)/2)]}
 END{for(i=1;i<=c;i++){k=ks[i];printf "%-46s %8s %8s %9s  %9s records/s\n",k,med(p50[k]),med(p99[k]),med(mx[k]),med(r[k])}}'
echo; echo "Send the file above (it holds no personal data beyond the Mac model and macOS version)."
```

How to read it:

- **`full rec=128 per_flush=1 prealloc=1`, quiet, p99:** the latency of one `silo` call in the
  common case. Under about 20 ms, the design above needs nothing more.
- **`writer full ...` p99 and max:** the latency while main writes. That is the number the
  `own-cost-targets` target for the engine should be written against.
- **`full per_flush=64` against `per_flush=1`:** if the latency is about the same, grouping is
  free on this Mac as it was on Linux, and the engine never needs to wait to group.
- **`barrier` against `full`:** how much a recorder could save by flushing lines with the barrier
  and acknowledging with a full flush.
- **`overwrite_after_clone_used_MB` near 512, `overwrite_unshared_used_MB` near 0:** APFS
  overwrites in place while no clone or snapshot shares the blocks, and takes new space once one
  does. Then preallocating protects the engine's log from a full disk only when no local
  snapshot exists, and the reserve below is needed. APFS reports free space lazily, so a result
  off by a few MB means nothing.
- The header line `local snapshots on /` says whether Time Machine snapshots are present on this
  Mac today.

---

## Cross-cutting findings

1. **The plan never names the call that makes a write durable, and the language it chose gets it
   wrong by default.** `engine-language` is Zig. Zig's standard sync on macOS is `fsync(2)`.
   Every runtime that cares (Go, Rust, LMDB, TigerBeetle) calls `F_FULLFSYNC`, and TigerBeetle,
   written in Zig, wraps it itself. Put the rule in one place (proposed in `engine-saved-state`),
   and add a test that reads the engine's source and refuses a bare `sync`/`fsync` call outside
   the one function that flushes. That is the kind of source check `engine-states-and-failures`
   already uses.
2. **A full disk on the Mac is a first-build risk that no decision covers.** main's disk image
   grows on the same APFS volume as the engine's files. Checkpoints hold blocks. Time Machine local
   snapshots, if present, make every overwrite need space. When the volume fills, the engine's
   next write fails, the engine stops (which is right), and by `when-the-engine-stops` nothing can
   change: no restore, and no dropping a checkpoint to free space, which is exactly what the user
   needs to do. Proposed, for an entry of `going-back` or `engine-limits`: (a) the engine keeps a
   reserve file of fixed size that only it may delete, to make room for its own writes and for
   deleting checkpoints; (b) below a free-space threshold, the engine stops taking checkpoints and
   pauses main before the volume is full, and the record says so. Sometimes called a "ballast
   file" in operations practice (from memory, not verified). The Mac script's overwrite test says
   whether (a) is needed when no snapshot exists.
3. **Every forced flush on a Mac drains the whole drive, so Silo's processes and main slow each
   other down.** The engine, the recorder and main's disk, if it is in `.full` mode, all flush one
   queue. On Linux here, a second writer raised the p99 of one flush 10 to 30 times. This links
   four entries that are measured apart today: `engine-saved-state`, `events-until-the-recorder`,
   `record-speed-cost` and `own-cost-targets`. The cheapest way to keep it low is the same in
   all four: group, and do not flush while idle. That needs the clock rule (only due deadlines
   enter as inputs), which `01-engine-internals.md` also proposes. As a challenge to a decision in
   force: `deterministic-core` (no verdict) says "Time and randomness are inputs". That stays
   true, but add that the core says when it next needs the time, and that a tick enters only then.
4. **`engine-under-failure` contradicts the fix.** It says "a call the engine cannot save is
   refused with a reason instead of accepted." After a failed flush, "cannot save" is unknowable:
   the call may be on disk. Proposed wording there: "a call the engine knows it did not save is
   refused with a reason; a call whose save failed partway gets an error saying its outcome is
   unknown, and its id gives the real result once the engine is back." This reaches what the
   user sees, so it is polarzero's.
5. **Mac measurements cannot run in hosted CI.** `speed-budgets`, `own-cost-targets` and
   `record-overhead-limit` all say a miss blocks a merge. For anything that starts a macOS
   computer, the merge gate (`tools/ci/check.sh` on GitHub runners) cannot measure it. A named
   Mac has to run those measurements before a merge, and that is a decision about how Silo is built
   that nobody has made yet.
6. **A kernel panic may count as a power loss on Apple silicon.** Apple's documentation pairs
   "panic or loss of power" for `.full` mode and for `F_FULLFSYNC`. On Apple silicon the SSD
   controller is part of the chip. marcan reported it doing DRAM traffic on flushes, which
   suggests its cache may sit in memory the panic resets. **Not verified.** If so, a plain
   `fsync` loses data on a panic too, not only on power loss, and a laptop on battery is not
   protected by its battery. It is one more reason not to take `F_BARRIERFSYNC` or `fsync` for
   the engine.

## Sources not yet checked

- The `F_FULLFSYNC` and `F_BARRIERFSYNC` costs on polarzero's own Mac (the script above).
- Whether APFS overwrites unshared file data in place. Secondary sources disagree, and Apple's APFS
  reference was not read. The script's overwrite test answers it for this purpose.
- The `fcntl(2)` text for `F_BARRIERFSYNC` on a current macOS (read `man fcntl` on the Mac).
- The Linux numbers above came from a virtual disk with an unknown host cache. The scripts are in `fsb/`
  and `macbench/` of the project folder `research/2026-11-24-first-build-decisions-scripts/`, not in the repository.
