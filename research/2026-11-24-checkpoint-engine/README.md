# The checkpoint engine, prototyped on Linux

An agent's research, 24 November 2026, for step 4 of the first build (#348): checkpoints taken on
their own, thinning, restore, and putting back one path. It is a trial in Zig, in
[`proto/`](proto/), and is not meant to be merged as code: nothing here is a feature the owner
took. It answers what can be answered without a Mac, and says which test on a Mac settles the rest.

Every number below was measured in this Linux container (kernel 6.18, 4 cores, no KVM), with XFS
and Btrfs on loop devices over the container's disk, so the numbers show how costs grow and how
the ways compare, not what a Mac gives. Each figure names its run in [`proto/results/`](proto/results/).

## What it found

1. **A checkpoint does not have to make the computer wait, if Silo sees the computer's writes.**
   With a log of the blocks written, kept where the computer's writes pass (Silo's own virtio-blk
   in libkrun, or a disk Silo serves over NBD to Apple's framework), the moment of a checkpoint is
   a swap of two bitmaps between two writes. On an 8 GiB disk image in 485,000 pieces, the computer
   stopped for less than a microsecond, and its longest write during a checkpoint was 0.16 to
   0.31 s. A clone of the same image made every write wait 9.2 to 11.8 s. Every checkpoint of every
   way was checked block by block to hold exactly the writes up to one moment.
2. **On a Mac, cloning anything that shares blocks with the live image may still make it wait.**
   Cloning an older checkpoint that nobody writes made the live image's writes wait up to 1.0 s on
   XFS. A variant that clones nothing while the computer runs (each checkpoint a file of the blocks
   it copied, `delta`) removes that cause: its checkpoints took 0.45 to 1.06 s, and most kept the
   longest write under 160 ms, near the disk's own noise of 16 to 85 ms. Whether APFS behaves like
   XFS here is the first Mac test to run.
3. **A full volume is a write error inside the computer, not only a checkpoint that fails.** Once
   the disk image shares blocks with a checkpoint, overwriting its own blocks needs free space: on
   a full XFS volume the computer's overwrites failed with "No space left on device". Removing a
   reserve file let them go on. Silo has to act before the volume is full, not after.
4. **The 30 days of daily checkpoints hold most of the disk.** Under a model of an agent rewriting
   2 GiB an hour, 10 hours a day, into a working set of 4 to 16 GiB, the kept checkpoints of the
   proposed thinning hold 247 to 413 GiB beyond the live disk; with 7 days of dailies, 72 to
   105 GiB. The disk limit, not the 30 days, decides how far back a Mac reaches.
5. **With a checkpoint at each turn, thinning removes up to 233 checkpoints an hour** (three
   turns a minute), and keeps up to 504. Each removal made the computer's writes wait 0.08 to
   0.19 s on XFS and 3 to 5 ms on Btrfs.
6. **The catalog needs no database.** A checkpoint is a folder that appears by one rename and
   disappears by one rename; a restore swaps the computer's folder in one call (`RENAME_EXCHANGE`
   on Linux, `RENAME_SWAP` on macOS), so a disk never comes back with the auxiliary storage of
   another moment. Killed at each of its first 300 steps, the catalog recovered every time, and the
   same test caught both bugs planted in it on purpose.

## The ways to take a checkpoint, measured

`ckpt bench`: an 8 GiB image is written in full, cloned once, then written with 300,000 random
4 KiB writes, which leaves it in 485,000 pieces, about what #352 found after weeks of checkpoints.
A thread then stands for the computer: one 4 KiB uncached write at a random place, 1,000 a second.
Eight checkpoints are taken 5 s apart, three are dropped, and every one kept is read back and
checked against the log of writes. "Longest write" is the longest single write that overlapped a
checkpoint; the computer's usual write took 0.2 ms.

| Way | Silo needs | Longest write during a checkpoint | Computer paused | Checkpoint takes | Write wait while one is dropped | Mac |
|---|---|---|---|---|---|---|
| None (the disk's own noise) | | 16 to 85 ms | | | | |
| `full`: clone the live image | a file system that clones | 9.2 to 11.8 s | no | 9.2 to 11.8 s | 82 to 101 ms | clonefile; #359 test 01 |
| `range`: clone the changed ranges into a clone of the last checkpoint | a range clone | 69 to 281 ms | 1.0 to 1.4 s | 13.5 to 18.1 s | 137 to 178 ms | no range clone on APFS |
| `cbw`: copy-before-write from a write log into a clone of the last checkpoint | to see the writes | 158 to 313 ms | under 1 µs | 12.1 to 15.6 s | 169 to 187 ms | clonefile, pread, pwrite |
| `nbd`: `cbw` behind Silo's NBD server | to serve the disk | 128 to 326 ms | no; the moment waits for the request in flight, 0.2 to 8.4 ms | 13.4 to 16.6 s | 101 to 134 ms | NBD attachment, macOS 14 |
| `delta`: copy-before-write into a new file of the copied blocks only | to see the writes | 65 to 395 ms, six of eight under 160 ms | 40 to 50 µs | 0.45 to 1.06 s | 80 to 123 ms (a merge of 0.8 to 1.5 s) | pread, pwrite |
| `btrfs`: read-only snapshot of the image's subvolume | Btrfs | 217 to 374 ms | no | 0.42 to 0.56 s | 3 to 5 ms | APFS snapshots need an entitlement Apple grants to backup apps only |

Runs: `results/<way>-8g.jsonl`. Each line of a run is one phase; `scripts/show.py` prints them.

### Why a clone makes the computer wait

A clone of the live image locks it for as long as the file system takes to share every piece, and
that grows with the pieces: 9.2 s at 485,000 and 11.8 s at 548,000 here, as #347 and #352 found.
`full` is the plan's answer today (`checkpoint-per-system`). Its wait also delays a turn when a
checkpoint is taken at each turn (`checkpoint-at-each-turn`): the reply is held until the clone
ends.

### Seeing the writes: `cbw`, `nbd`, `delta`

Silo keeps a bitmap of the blocks written since the last checkpoint, set before each write.

1. Silo clones the last checkpoint, which nobody writes, as the start of the new one.
2. Between two writes, Silo swaps the bitmap for an empty one. That is the checkpoint's moment.
3. Silo copies each block the old bitmap marks from the live image into the new checkpoint. A write
   to a marked block not yet copied copies that block first, then goes on.
4. Silo flushes the checkpoint and renames it into place.

The computer stops only for step 2. In the 8 GiB run, step 2 took under 1 µs and step 3 copied
47 to 53 MiB, the blocks written since the last checkpoint. The one write that found its block not
yet copied waited for one 4 KiB copy.

The checkpoint's moment is fixed at step 2, so a reply held for a turn's checkpoint
(`checkpoint-at-each-turn`) can go on after step 2, without waiting for the copy. A new checkpoint
cannot start its step 2 before the last one finished step 3; at 485,000 pieces that was 12 to 16 s
here, which bounds how often checkpoints come (about one every 15 s on this disk).

The remaining waits of 0.16 to 0.31 s came from step 1 and step 3, both of which create or break
shared blocks: cloning a file that shares blocks with the live image made its writes wait up to
1.0 s in the `full` run (`unrelated-clone`), and 0.18 s in the `cbw` run. `delta` removes both:
step 1 creates an empty file, and step 3 writes into it, so nothing is shared while the computer
runs. In the 8 GiB run, a `delta` checkpoint took 0.45 to 1.06 s instead of 12 to 16 s, the
computer stopped for 40 to 50 µs (the copy of the bitmap), and six of its eight checkpoints kept
the longest write under 160 ms, near the disk's own noise. Reading a `delta` checkpoint as a disk means applying every delta since the first
checkpoint to a clone of it, which a restore does anyway with the computer stopped
(about 15 to 25 s for the 8 GiB run, with six deltas). Dropping a delta moves its blocks into the next one.

The bitmap is also the list of blocks a checkpoint changed, which `file-changes-in-the-record`
needs. Comparing the block maps of two clones (FIEMAP on Linux) gave exactly the blocks written,
none missed and none extra, in every run, but cost 26 to 32 s on XFS at 600,000 pieces (0.1 s at
50,000, so it grows faster than the pieces) and 2.4 s on Btrfs.

Who can keep the bitmap:

- **Linux computers on libkrun.** Silo already patches libkrun (`libkrun-patches`); its virtio-blk
  is where each write passes. This is the prototype's `cbw`, with the writer thread standing for
  the virtio-blk worker.
- **macOS computers.** Apple's framework writes the image itself, so Silo sees nothing, unless the
  disk is attached with `VZNetworkBlockDeviceStorageDeviceAttachment` (macOS 14) and Silo serves it
  over NBD. [`proto/src/nbd.zig`](proto/src/nbd.zig) is such a server, about 300 lines: `nbdinfo`
  reads it, `nbdcopy` wrote 200 MB into it and read the same bytes back, and the bench's computer
  wrote through it while checkpoints were taken: a 4 KiB write took 0.35 ms through it against
0.2 ms to the file, over a socket in one process. Nobody has published whether a macOS guest boots
  from an NBD disk, or what it costs per write on a Mac. That is a Mac test.

### Range clones

`range` clones only the ranges of the live image that differ from the last checkpoint, found by
comparing block maps, then compares again with the computer paused to catch what changed
meanwhile. Writes waited less than with `full`, but the paused comparison of two maps of 600,000
pieces took 1.0 to 1.4 s, and APFS has no public call that clones a range of a file. It is not
worth pursuing.

### Btrfs snapshots

A read-only snapshot of a subvolume holding the image took 0.42 to 0.56 s, made writes wait up to
0.37 s, and a dropped snapshot cost writes 3 to 5 ms. It is the best way here for a Linux host on
Btrfs (the thread on Linux hosts measures that case). The APFS equivalent, a volume snapshot,
needs the entitlement `com.apple.developer.vfs.snapshot`, which Apple grants only to backup apps
after review, and going back to a snapshot needs a second entitlement that only Apple holds
([Apple's man page](https://keith.github.io/xcode-man-pages/fs_snapshot_create.2.html),
[a DTS answer](https://developer.apple.com/forums/thread/89635)).

## Thinning

[`proto/src/thin.zig`](proto/src/thin.zig) is the rule of `thinning-checkpoints`: every automatic
checkpoint of the last 2 hours, one per hour for 2 days, one per day for 30 days. The one kept for
an hour or a day is the oldest still there. With that choice, thinning often, rarely or after a
crash gives the same set: a test checks it on 300 random histories and random rules. Keeping the
newest of each hour instead would remove checkpoints the rule needs later.

`ckpt thin` runs 40 days of work, 10 hours a day, with a checkpoint every 5 minutes:

| Checkpoints at each turn | Taken | Kept at most | Removed in one hour at most |
|---|---|---|---|
| none | 4,800 | 71 | 12 |
| 1 a minute | 28,993 | 225 | 96 |
| 3 a minute | 78,854 | 504 | 233 |

The disk the kept checkpoints hold beyond the live disk, under a model of writes (1 MiB pieces of a
64 GiB disk, 80% of the writes in a working set): the size of the working set and how much is
written are guesses until a Mac measures a real day of agents.

| Written an hour | Working set | Dailies for 30 days | Dailies for 7 days |
|---|---|---|---|
| 0.5 GiB | 4 GiB | 107 GiB | 27 GiB |
| 0.5 GiB | 16 GiB | 135 GiB | 32 GiB |
| 2 GiB | 4 GiB | 247 GiB | 72 GiB |
| 2 GiB | 16 GiB | 413 GiB | 105 GiB |
| 5 GiB | 4 GiB | 424 GiB | 133 GiB |
| 5 GiB | 16 GiB | 700 GiB | 199 GiB |

Each daily checkpoint keeps its own copy of whatever the agents rewrote that day, so the 30 dailies
cost about 30 days of rewrites. A macOS computer makes this worse: APFS inside it writes new data
to free places, so the image's blocks keep changing even when the files do not, unless the
guest's TRIM reaches the host image, which is a Mac test.

## The catalog and crashes

[`proto/src/catalog.zig`](proto/src/catalog.zig):

```
<computer>/live/           disk.img, aux.bin         the computer
<computer>/ckpt/<id>/      disk.img, aux.bin, meta   one whole checkpoint
<computer>/ckpt/.tmp-<id>/                           being made: removed at start
<computer>/ckpt/.trash-<id>/                         being removed: removed at start
<computer>/live.new/                                 a restore or a start being prepared
<computer>/next-id                                   written before an id is used
```

- A checkpoint is built in `.tmp-<id>`, flushed (`F_FULLFSYNC` on a Mac), and renamed to `<id>`:
  it is whole or absent. It goes by a rename to `.trash-<id>`, then its files are deleted.
- A restore saves first (`restore-saves-first`), builds `live.new` from clones of the checkpoint's
  files, and swaps it with `live` in one call. The disk and the auxiliary storage of a macOS
  computer always come from the same moment, which #352 (file 07) found Silo must ensure.
- An id is written to `next-id` before it is used, so an id is never given twice, even when the
  newest checkpoint was removed before a crash.
- Starting the engine removes what starts with a dot and `live.new`. Nothing else needs repair.

`ckpt crash systematic=1` runs takes, removals, restores and restarts in a child process that kills
itself at its k-th step, for k from 1 to 300, then recovers and checks: the computer's disk and
auxiliary storage are of one boot; every checkpoint is whole and of one moment; every checkpoint
reported done is still there; nothing half-made is left; no id came twice. All 300 passed. Planting
a bug catches it: swapping the folders with two renames instead of one call failed at a crash
between them, and building a checkpoint in its final place failed at the first crash inside it.
Killed at random times instead, the two-renames bug was caught in one run of 400 rounds and passed
another: random kills miss short windows, which is why the test kills at every step.

This tests a process crash, not a power cut. What a power cut keeps depends on the flushes, which
is a Mac test with `F_FULLFSYNC` (#352 file 02).

## A full volume

[`proto/scripts/full-volume.sh`](proto/scripts/full-volume.sh): on a 2 GiB XFS volume, an 800 MB
image with one checkpoint, a 200 MB reserve file, and a file filling the rest. Overwriting the
image failed with "No space left on device" after 174 blocks: each overwrite of a shared block
needs a new one. After the reserve file was deleted, 1,000 overwrites went through. Apple's
`clonefile(2)` says the same can happen on APFS ("a subsequent overwrite of an existing data
block" can return `ENOSPC`).

So `checkpoint-storage` and `thinning-checkpoints` need a floor of free space below which Silo
stops taking checkpoints, thins harder and alerts, well before the volume is full; the reserve file
is what the computer keeps writing into while that happens. What Apple's framework does when the
host refuses a write (an I/O error inside the computer, or the computer stopped) is a Mac test.

## Putting back one run

`ckpt putback before=<dir> after=<dir> live=<dir>` is `putting-back-a-runs-paths`: the run's paths
are those that differ between the checkpoints around it; each goes back when the live computer
still has it as the run left it, and is shown otherwise. Written to a temporary name and renamed,
so a program never reads half a file.

[`proto/scripts/putback.sh`](proto/scripts/putback.sh): an ext4 image stands for a Linux
computer's disk. A run deletes a folder of 20,000 files, edits 10 files and adds 5, then another
agent edits 2 of those 10. Put-back found 20,219 paths changed by the run in 1.2 s, put back 20,209
and removed 8 in 0.7 s, and showed the 2 edited later. The other agent's new file stayed.

Three things it showed:

- **A checkpoint is read through a clone of it.** A checkpoint of a running ext4 disk has a journal
  to replay, and Linux refused to mount it read-only. Mounting a clone replays the journal there
  and leaves the checkpoint as it was (`checkpoint-never-changes`). The two mounts took 0.1 s.
- **Put-back writes through the computer.** The computer's own kernel has its disk mounted, so
  nothing may write the image from outside while it runs. Silo writes the files inside it, over
  SSH or a shared folder.
- **"Nobody touched it since" is checked, then written.** An agent writing the same path between
  the two loses its write. Pausing the computer for the put-back (0.7 s for 20,000 files here)
  closes that window.

## What transfers to a Mac

| Part | On a Mac |
|---|---|
| `full` | As is (`clonefile`). Its wait on APFS is #359's test 01 |
| `cbw` and `delta` | As code: they use only `clonefile`, `pread` and `pwrite`. For a macOS computer they need the NBD attachment; for a Linux computer, libkrun's virtio-blk |
| `nbd` | The server compiles for macOS. Whether a macOS guest boots from it, and its cost per write, are unknown |
| `range`, `btrfs` | No: no range clone on APFS, and volume snapshots are closed to Silo |
| Thinning | As is: it reads only times |
| Catalog | As is, with `F_FULLFSYNC` and `renamex_np(RENAME_SWAP)` |
| Put-back | The rule as is. Reading a macOS computer's checkpoint is the APFS reader of #352 (file 11) or a mounted clone |
| Full volume | The same `ENOSPC`, per `clonefile(2)` |

The prototype builds for a Mac: `zig build --release -Dtarget=aarch64-macos` succeeds from Linux.

## Tests for the Mac

[`proto/scripts/mac.sh`](proto/scripts/mac.sh) runs on a Mac with Zig, in about 40 minutes, with
no VM and no sudo: `full`, `cbw` and `nbd` on APFS with the stand-in computer, and the catalog's
crash test with `F_FULLFSYNC` and `RENAME_SWAP`. It answers:

1. Does cloning a file that shares blocks with the live image (the last checkpoint) make the live
   image's writes wait on APFS? If yes, `delta` rather than `cbw`.
2. How long does a write wait while a checkpoint is dropped on APFS, which happens up to 233 times
   an hour with checkpoints at each turn?
3. Does the catalog hold on APFS when killed at every step?

Three need a computer, and belong in #359's kit:

4. Does a macOS computer, and a Linux one, boot and run from a disk Silo serves over NBD
   (`VZNetworkBlockDeviceStorageDeviceAttachment`)? What does a write cost through it, against the
   image attached directly?
5. When the host refuses a write with `ENOSPC`, does the computer see an I/O error, or does
   Apple's framework stop it?
6. Does TRIM inside the computer free blocks of the image on the host? Without it, every daily
   checkpoint keeps what the guest's file system moved, deleted files included.

## Running it

```
cd proto
zig build --release                  # Zig 0.15.2; zig build test runs the thinning and catalog tests
scripts/big.sh full cbw delta        # 8 GiB runs on /mnt/x (XFS) and /mnt/b (Btrfs); see scripts/fresh.sh
./zig-out/bin/ckpt bench strategy=cbw dir=/mnt/x/run size_gib=1 frag=20000 count=4 interval_ms=1000
./zig-out/bin/ckpt thin turns_per_min=3
./zig-out/bin/ckpt crash root=/mnt/c/crash systematic=1 rounds=300
scripts/putback.sh                   # needs root, for loop mounts
```

`bench` takes `strategy` (`none`, `full`, `range`, `cbw`, `nbd`, `delta`, `btrfs`), `size_gib`,
`frag` (writes after the first clone, to make pieces), `rate` (writes a second), `interval_ms`,
`count`, `drop`, `verify` and `unrelated`. Each line it prints is one phase, as JSON.

## Related

#366 (Linux hosts) measured what this page does not: a second computer on the same file system
kept 4/5 of its writes a second during a checkpoint on XFS and lost a third on Btrfs; a restore by
reflink took 10.6 to 11.7 s on XFS and 4.5 s on Btrfs; and a disk restored from a fragmented
checkpoint read 7 to 8 times slower in order. That last one applies to `cbw` and `delta` too,
whose checkpoints fragment as they chain. It also found that libkrun 2.0 exposes
`krun_vmm_handle_pause` and `krun_vmm_handle_resume`, which a Linux computer's swap of bitmaps
could use if Silo's write log lives outside virtio-blk.
