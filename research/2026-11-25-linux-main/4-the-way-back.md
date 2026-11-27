# Step 4 under B: the way back for a Linux main

Part of option B's specs ([README](README.md)). It takes the place of #370's
`4-the-way-back.md` for a Linux main and builds the same features: `checkpoints-taken-on-their-own`,
`thinning-old-checkpoints` and `restore-to-a-checkpoint`. It needs step 2 under B.

What stays exactly as #370 wrote it: the screens of `silo checkpoints` and `silo restore`, when
checkpoints are taken, thinning, space and the reserve, restore as an operation of seven steps,
the core's rows, the record of step 4 and its crashes. This page says only what differs, and the
one new piece: the write log.

## What a checkpoint is under B

Main's disk is one raw file, `live/disk.raw`, on the Mac's APFS volume. libkrun writes it with
`pwrite`, in the helper's own process. Two ways to checkpoint it follow, and the spec builds both,
as #370 builds both answers of #359's test 04. The catalog's `meta` names the strategy, so main can
carry checkpoints of both kinds.

| Strategy | How | Silo needs | What the Mac must show |
|---|---|---|---|
| `clone` | `clonefile` of `disk.raw` while main runs, as #370 | nothing new | test L5: a clone taken between two writes of libkrun is a moment in time; #359 tests 03 and 31: how long writes wait |
| `delta` | #365's copy-before-write into a file of the copied blocks only, fed by a log of the blocks main writes | the write log, a patch to libkrun | test L6: what the log costs per write on a Mac |

`clone` is what step 4 ships first. It is the same catalog driver as A's, on a file that has no
auxiliary storage. `delta` replaces it once the patch has passed review (`engine-patch-review`) and
tests K07, K08 and L6, with no change outside the catalog driver and the helper.

Why B goes to `delta` and A does not: #365 measured `clone` making every write of the computer
wait 9.2 to 11.8 s once the image is in half a million pieces, and `delta` keeping six writes of
eight under 160 ms. Under A, `delta` needs main's disk served over NBD, and nobody has published
whether a macOS computer boots from NBD (#359 test 32). Under B the writes already pass through
code Silo builds: libkrun's virtio-blk worker, in the helper.

### Is a clone of `disk.raw` a moment in time

ext4 in main relies only on what a disk promises: a write that main was told is done is on the
disk after a flush, and nothing about the order of writes not yet flushed. A copy is a moment in
time for ext4 when it holds every write libkrun completed before some instant and none after it.

- libkrun completes a write to main only after its `pwrite` returns, so a clone taken at one
  instant holds a prefix of completed writes, if `clonefile` is atomic against a `pwrite` in
  another thread of the same process.
- On Linux, `FICLONE` takes the file's lock against writes, so it is. The kit's test K07 checks it
  with a real computer: a writer in main numbers each 4 KiB block it writes with `O_DIRECT` and a
  flush; the checkpoint must hold a prefix of the numbers.
- On APFS it is not documented. Test L5 runs the same writer on the Mac. If it fails, `clone`
  pauses main around the clone (`krun_vmm_handle_pause`) and waits for the block worker's queue to
  empty before it clones, and the pause's length is in the record, as #370 does for test 04.

ext4 replays its journal at the next boot of a restored disk, as after a power cut. Step 5's reader
replays it in memory before it reads, which turns out to matter (`5-the-record.md`).

### The write log

A patch to libkrun's block device, written to be sent upstream (`libkrun-patches`): a hook called
on the block worker's thread before each request that changes the disk.

In `src/devices/src/virtio/block/worker.rs` (libkrun `main`, read on 25 November 2026), every
change to the disk passes through `process_request`, in three request types: `VIRTIO_BLK_T_OUT`
(a write), `VIRTIO_BLK_T_WRITE_ZEROES` and `VIRTIO_BLK_T_DISCARD`. The patch adds, before each:

1. Compute the byte range with checked arithmetic, and refuse a range past the end of the disk with
   `VIRTIO_BLK_S_IOERR`. The read path already uses `checked_mul` for its offset; the write, discard and write-zeroes paths multiply
   `sector * 512` unchecked, and the patch makes all four the same. Whether the disk layer below
   already refuses a wrapped offset is not checked here: the fuzz target below answers it.
2. Call the hook with the range. The hook returns 0 to go on, or an error that fails the request
   with `VIRTIO_BLK_S_IOERR`.

The proposed C interface:

```c
/* Called on the block worker's thread before each write, write-zeroes and discard,
 * with a range already checked against the disk's size. Return 0 to go on. */
typedef int32_t (*krun_block_write_hook)(void *user, uint64_t offset, uint64_t len);
void krun_block_device_set_write_hook(KrunBlockDevice handle, krun_block_write_hook hook, void *user);
```

Silo's hook, in the helper, is #365's `cbw` and `delta` with the block worker in place of the
prototype's writer thread:

- it sets the bits of the range in the bitmap of blocks written since the last checkpoint (4 KiB a
  bit: 1 MiB of bitmap for a 32 GiB disk);
- while a checkpoint is being copied, for each block of the range that the old bitmap marks and
  that is not copied yet, it copies the block's current bytes into the checkpoint first.

A checkpoint's moment is the swap of the bitmap for an empty one, under the hook's lock, between
two requests. #365 measured that swap under 1 µs, and 40 to 50 µs for `delta`'s copy of the bitmap.

Every value the hook receives comes from main, so `engine-patch-review` applies: the patch brings
a fuzz target that drives `process_request` with random headers and lengths against a small disk
and checks that the hook never sees a range past the end, and that a refused range changes
nothing. The hook's own code is Silo's, and the same target runs it.

If upstream refuses the hook, the patch is carried with the reason, as `libkrun-patches` says. It
touches one function and adds one setter.

### A checkpoint's files under B

```
computers/c1/ckpt/<id>/
  disk.raw     clone strategy: the whole disk, cloned
  delta        delta strategy: the blocks copied, as #365's format
  writes       the bitmap of blocks written between the parent and this checkpoint
  meta
```

`writes` is new. It is the bitmap the checkpoint's moment swapped out, kept for step 5: it tells
the comparer exactly which blocks changed between this checkpoint and its parent. Under `clone`,
no bitmap exists and the comparer finds the changed blocks another way (step 5). `meta` gains
`"strategy":"delta"` and `"writes":true`.

### Restore

#370's seven steps, with two changes:

- Step 4 prepares `live.new/` from the checkpoint's `disk.raw` (under `clone`) or from the disk
  rebuilt by applying the deltas (under `delta`, as #365's restore does, with main stopped).
  `config.json` and `machine-id` come from `live/`. There is no auxiliary storage to keep in step
  with the disk.
- The first boot after a restore replays ext4's journal. Nothing of Silo's does that: main's own
  kernel does it, as after a power cut.

The login files of Claude Code and Codex come back from the checkpoint as under A, and #359's test
02 in its Linux form says whether each program asks you to sign in again.

### Discards

When main deletes files, ext4 sends `DISCARD` for their blocks if it is mounted with `discard` or
when `fstrim.timer` runs, which Ubuntu enables weekly. libkrun passes a discard to its disk layer
(imago's `discard_to_any`), which should free the blocks of `disk.raw`, so the Mac gets the space
back unless a checkpoint shares them. Whether it does on APFS is not read in the source here. The write
log counts a discard as a write, since the blocks' contents change. The kit's test K10 checks that a
deleted 1 GiB file comes back to the host after `fstrim`, and test L8 does the same on APFS, as
#359's test 33 does for a macOS computer.

## Cost targets

#370's, unchanged: the longest write in main during a checkpoint under 100 ms at the 99th
percentile after a week of checkpoints, a checkpoint under 5 s, a restore under 2 minutes. Under
`clone` on a fragmented image the first target fails, as it fails for A (#365); it is what moves B
to `delta`. One more:

| Measure | Target |
|---|---|
| The write log's cost per 4 KiB write in main | under 2 µs at the median, on a Mac; measured by test L6 against the same build with no hook |

## Tests

| Test | Checks | Where |
|---|---|---|
| #370's catalog, thinning and simulator tests | Unchanged | CI |
| #365's `delta` and `cbw` benchmark with the writer thread replaced by a fake block worker that calls the hook | The same results as #365; every checkpoint read back holds the writes up to its moment | CI on Linux |
| The hook's fuzz target | No range past the end; a refused range changes nothing | CI, nightly longer |
| Kit K07: the numbered writer in main, checkpoints by `clone` (FICLONE on XFS) and by `delta` | Each checkpoint holds a prefix of the completed writes | a Linux host with KVM |
| Kit K08: #365's longest-write measure with a real computer, `clone` against `delta`, on a disk fragmented by #365's script | The cost target holds for `delta` | a Linux host with KVM |
| Kit K09: restore to a checkpoint taken during `npm install` | Main boots, ext4 replays its journal with no error, `claude --resume` shows the conversation | a Linux host with KVM, then the Mac |
| Kit K10 and L8: a deleted file's space after `fstrim` | It comes back to the host, unless a checkpoint holds it | a Linux host with KVM, then the Mac |
| L5, L6 | A clone on APFS is a moment in time; the write log's cost on a Mac | polarzero's Mac |
| #359 tests 03, 06, 31, 33 on `disk.raw` | Clone waits on APFS, the flush cost, the prototype on APFS, TRIM | polarzero's Mac, before step 4 |

## Done when

#370's four conditions, and:

- with the write log, the longest write in main during a checkpoint stays under 100 ms after a week
  of checkpoints, on a disk image in half a million pieces;
- every checkpoint main has, whatever its strategy, restores to a disk that boots and whose journal
  replays without error.
