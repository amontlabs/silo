# Going back: a second opinion on seven decisions

An agent's part, 24 November 2026, for polarzero. It edits nothing in `plan/`. Every fact below is
marked **verified** (I read the source or the document, or ran the command and give the number) or
**not verified**. The measurements were run in this Linux container (kernel 6.18, no KVM, XFS and
Btrfs on loop devices over the container's disk). They show how costs grow, not what a Mac gives.
No Mac was available, so nothing here is measured on APFS. The test that settles the APFS questions
is at the end, as a script to run.

| id | Recommendation | Confidence | One-line reason |
|---|---|---|---|
| `checkpoint-per-system` | Take with an edit | medium | The copy is a moment in time without a pause on XFS and Btrfs, but the computer's writes wait for the whole copy and the wait grows for good; and a macOS computer needs its auxiliary storage copied with its disk |
| `fork-shares-disk-blocks` | Take as written | high | Right for the first build, which makes no fork; a fork is cloned from a checkpoint, which nothing writes, so it makes main wait for nothing |
| `checkpoint-storage` | Take with an edit | high | A clone can live only on the same APFS volume, not just the same drive; and "removed whole" needs a defined order once a checkpoint is more than one file |
| `restore-saves-first` | Take with an edit | high | Thinning by age can remove the saved checkpoint, which is the only copy of the work the restore left; and saving after the stop gives a cleaner checkpoint at no cost |
| `what-a-restore-leaves` | Take with an edit | medium | Grants that live as files on the disk, such as the SSH key Silo uses to reach main, go back with the disk unless Silo writes them again |
| `why-destructive` | Take as written | high | It files the three by what they really lose today |
| `meanings-for-computers` | Take with an edit | medium | "A saved moment" promises running programs, which a checkpoint does not keep; "nothing is lost" in a pause is untrue for connections to the internet |

The three findings that matter most:

1. **The cost of a checkpoint is a wait of every write in main, and it grows with main's age.**
   Reproduced here: on XFS, an 8 GiB disk image in 500,313 pieces took 11.0 s to clone, and a
   program writing to it waited 10.7 s for one 4 KiB write. It is the clone time itself, almost
   to the hundredth. An agent-like workload in a file system on the image reached 98,000 pieces
   after 30 checkpoints (1.7 s per clone) and stays there, because each checkpoint shares the
   blocks again. With a single checkpoint the same workload on ext4 healed to 21,000 pieces. The cost
   moves with the number of pieces, not with the size of the disk or the number of checkpoints
   kept. APFS keeps one record per piece too, so it is expected to grow the same way, at a speed
   nobody has published.
2. **Overlays do not replace clones for checkpoints of a running computer, even on macOS 27.**
   DiskImageKit (macOS 27) stacks copy-on-write ASIF layers, and UTM and Tart now use it. But the
   layers of an attached disk cannot change while the computer runs (UTM's source says so and
   refuses a snapshot of a running Apple VM), and Apple has no API to merge a layer back (Apple's
   DTS, June 2026). A layer per boot is possible; a layer per checkpoint is not.
3. **Restore has three holes in its current wording**: thinning can remove the only copy of the
   work a restore left; the SSH key Silo uses to reach main lives on main's disk and goes back with
   it; and a macOS computer's auxiliary storage has to go back with its disk, as UTM and Tart copy
   it with every snapshot and clone.

## The measurements behind this page

All scripts are in `exp/` of the project folder `research/2026-11-24-first-build-decisions-scripts/`
(outside the repository). XFS was made with `mkfs.xfs -m reflink=1` on a 40 GB loop device.

### PR #347's stall, reproduced

`run1.sh`: an 8 GiB image written in full, cloned once with `cp --reflink=always`, then written to
with 4 KiB writes at random places (`fio`, direct I/O). After each round the image is cloned twice:
once alone, once while a second program writes 4 KiB blocks to it one after another and records
the longest one (`stall.py`).

| 4 KiB writes since the first clone | Pieces (`filefrag`) | Clone | Clone while written | Longest single write meanwhile | Writes that got through during the clone |
|---|---|---|---|---|---|
| 0 | 1 | 0.003 s | 0.17 s | 0.17 s | 7 |
| 20,000 | 56,463 | 1.04 s | 1.12 s | 1.11 s | 23 |
| 100,000 | 202,053 | 4.23 s | 3.99 s | 3.98 s | 6 |
| 300,000 | 500,313 | 11.0 s | 10.7 s | 10.7 s | 15 |
| 600,000 | 815,653 | 17.4 s | 20.9 s | 20.9 s | 34 |

Verified, matches PR #347: about 21 µs per piece on XFS, and the writer is blocked for the whole
clone. Its usual write took 0.1 to 0.3 ms.

### What a file system on the image does, with a checkpoint after each step

`run3.sh`: a 24 GiB image on XFS, attached as a loop device with direct I/O, as a virtual machine's
disk is. A guest file system (ext4, then Btrfs) is made on it and mounted. One step of work
(`work.py`) unpacks 20,000 files (362 MB), deletes the tree from two steps before, rewrites 10% of
2,000 source files, rewrites 50 build outputs of 4 MB, updates 20,000 random rows of a 400 MB
SQLite table and appends to a log. About 0.75 GiB reaches the disk per step. After each step the
guest syncs and the host clones the image; the last 10 clones are kept.

| Step | ext4: pieces | ext4: clone | Btrfs: pieces | Btrfs: clone |
|---|---|---|---|---|
| 1 | 689 | 0.01 s | 696 | 0.02 s |
| 2 | 31,749 | 0.53 s | 6,063 | 0.09 s |
| 5 | 69,601 | 1.61 s | 29,986 | 0.46 s |
| 10 | 88,338 | 2.30 s | 65,889 | 5.46 s |
| 20 | 96,421 | 2.15 s | 112,861 | 2.02 s |
| 30 | 98,237 | 1.71 s | 127,414 | 2.27 s |

(Clone times vary by a factor of two between steps because other experiments shared the disk;
the pieces are exact.)

- Verified: the pieces level off once the file system has written over the part of the disk it
  uses. The level is set by how much of the disk the guest keeps rewriting, not by time.
- Verified: a copy-on-write guest file system (Btrfs here, APFS in a macOS computer) is not better
  in the long run. It starts with fewer pieces, because it writes new data in long runs, but it
  moves across more of the disk and ended higher.

`run4.sh`: the same work with one clone at step 1 and none after.

| Step | ext4, one clone only: pieces | Btrfs, one clone only: pieces |
|---|---|---|
| 2 | 31,532 | 10,075 |
| 5 | 57,959 | 19,293 |
| 10 | 39,521 | 25,462 |
| 15 | 21,185 | 25,522 |

Verified: without new clones, the ext4 image heals as XFS writes over the shared parts, and the
Btrfs one stops at 25,500, a fifth of the 127,414 it reached with a clone per step. Frequent
checkpoints keep the image at its worst. The number of checkpoints kept does not matter: deleting
older clones does not join the pieces again (PR #347 found this; not re-measured here).

### Mitigations tried

`run2.sh`, on the 8 GiB image after 300,000 random 4 KiB writes (485,898 pieces, 10.9 s per clone):

| Mitigation | Result | Cost |
|---|---|---|
| The first write into a block of G bytes after a checkpoint copies the whole block (what a disk layer of Silo's own could do; Apple's attachment cannot) | G = 64 KiB: 76,705 pieces, 1.14 s. 256 KiB: 32,767, 0.49 s. 1 MiB: 8,192, 0.13 s. 4 MiB: 2,048, 0.03 s | Writes and checkpoint space multiplied: 1.2 GB written became 8.5 GB at 64 KiB and 9.8 GB (the whole image) at 1 MiB, for scattered writes. The count can never pass the disk size divided by G |
| A full copy of the image, without sharing, made while nothing writes ("flattening") | 32.5 s for 8 GiB here; the copy is in 4 pieces and clones in 0.002 s | A whole disk of space until the old checkpoints are thinned, and the computer stopped for the copy |
| `xfs_fsr` (XFS's own defragmenter) with a clone still sharing the image | 115 s, then 1 piece | It unshares what it moves. Without a clone sharing it: 0.3 s. APFS has no such tool for a single file |
| Cloning in ranges (`FICLONERANGE`, 64 MiB at a time) | Longest single write 1.27 s instead of 14.7 s; total 36.8 s | The copy is no longer a moment in time: a later write in a range not yet copied lands in the copy while an earlier one in a copied range does not |
| Deleting a clone in 485,569 pieces while the live image (549,084 pieces) is written | `rm` returns at once; a write waits 2.1 s while XFS frees the pieces in the background. Deleting the first clone, which holds the blocks from before the writes: 2.0 s | Thinning makes main wait too, less than a clone |

### A chain of qcow2 layers (the Linux alternative to clones)

`run5b.sh`, `qemu-img` and `qemu-io` (QEMU's code, not libkrun's) on the container's ext4 disk: a
2 GiB base, then layers each holding 500 random 4 KiB writes, and 4,000 random 4 KiB reads through
the top.

| Layers | 4,000 random reads | Opening the chain |
|---|---|---|
| 1 | 0.17 s | 7 ms |
| 10 | 0.18 s | 10 ms |
| 50 | 0.44 s | 47 ms |
| 100 | 0.97 s | 96 ms |
| 150 | 1.52 s | 168 ms |
| 150, collapsed into one file (`qemu-img convert`, 9.8 s) | 0.20 s | |

Verified: a chain costs nothing up to about ten layers, then reads slow down in proportion to its
length (9 times at 150 layers). A layer costs constant time to make at any age, unlike a clone,
but a checkpoint per layer needs collapsing, which QEMU can do on a live disk (`block-stream`,
`block-commit`; QEMU's documentation, not run here) and Apple's framework cannot. With a checkpoint every few minutes, 150 layers is
one working day.

### Is a clone taken during writes a moment in time?

PR #347 found yes on XFS and Btrfs, direct and buffered. Re-run here with the helper of the Mac
script on XFS (`h_lin.py pit`): 54,073 writes acknowledged before the clone, 55,109 after, and the
clone held every write up to 55,055 and none after it. Verified on XFS only. One more fact seen
here: with buffered writes, a 1 GiB clone in few pieces took 4.35 s, probably because Linux writes
the dirty page cache of the file out before it clones (the cause is not verified; the disk was
shared with another experiment). If APFS does the same, a computer whose disk goes through the
Mac's cache (`cached` mode) makes each clone wait for that too.

## `checkpoint-per-system`: What is a checkpoint made of on each system?

- **Recommendation:** Take with an edit. The disk-only answer is right, but the answer leaves out
  the auxiliary storage of a macOS computer, and the "to find out" asks the wrong question: the
  open question is how long the computer's writes wait, not whether the copy is consistent.
- **Confidence:** medium. The consistency rests on two Linux file systems and one reasoning about
  APFS (below); the cost on APFS is not measured by anyone I could find.
- **What it decides, on one real case:** main has run for three weeks. Claude Code runs
  `npm install` while Silo takes the 14:05 checkpoint. For as long as the clone lasts, every write
  of main's disk waits: `npm` stops, and so does Claude Code writing its conversation file. On XFS
  at 500,000 pieces that is 11 seconds; on APFS it is unknown. At 15:00 you restore 14:05: main
  boots cold with the disk and its firmware settings as they were at 14:05.
- **Options:**
  - Disk only, clone while running (the answer). Costs the write wait above, at every checkpoint,
    growing with fragmentation. Nobody else does this for macOS guests: Tart clones the files with
    `FileManager.copyItem` and does not check that the source is stopped (verified, `Clone.swift`
    and `VMDirectory.clone` at commit `a8d03d4`), but documents nothing about running VMs;
    UTM refuses to snapshot a running Apple VM (verified, `UTMAppleSnapshotBackend.swift`,
    "the current state can only be saved or replaced while the VM is not running"); Anka clones
    stopped or suspended templates (not verified, Packer plugin docs in a search summary).
  - Disk only, paused for the copy. Costs the same wait, plus the programs' clocks; buys nothing
    if the copy is already a moment in time.
  - Disk and memory. Costs disk the size of memory per checkpoint and refuses after some macOS
    updates. Parallels Desktop 20 snapshots macOS guests this way (not verified how; its
    knowledge base says snapshots exist since version 20, and its 20.3.1 notes fix "could not be
    resumed after creating a snapshot in a suspended state").
  - An overlay layer per checkpoint (DiskImageKit, macOS 27). Not possible while the computer
    runs: "Virtualization cannot change the layers of a disk that is attached" (UTM's source,
    verified). Possible once per boot.
  - A disk served by Silo itself over NBD (`VZNetworkBlockDeviceStorageDeviceAttachment`, macOS
    14). A checkpoint becomes a mark in Silo's own block store and costs the same at any age;
    it also gives the blocks written between two checkpoints, which `file-changes-from-outside`
    names as its third route. It costs a storage engine to write, every disk I/O through a
    socket, and whether a macOS guest boots from it is not known (not verified).
- **Evidence:**
  - Apple's framework takes two formats, RAW and ASIF, with three caching modes (automatic,
    cached, uncached) and three synchronization modes (full, fsync, none) (verified, Apple's pages
    for `VZDiskImageStorageDeviceAttachment`, `VZDiskImageCachingMode`,
    `VZDiskImageSynchronizationMode`). Nothing in Apple's documentation says what a clone of an
    attached image holds (verified, absent).
  - `clonefile(2)` says the call is atomic and that "a subsequent overwrite of an existing data
    block" can return ENOSPC; it says nothing about a file open for writing or data not yet
    flushed (verified, man page).
  - APFS keeps a file's data as one extent record per piece, and the shared count on the physical
    extents (not verified: the layout as the Sleuth Kit and the Linux APFS patches give it, seen
    in search summaries; that a clone walks every record is inferred, not measured). So the clone
    time should grow with pieces as on XFS. Eclectic Light notes that edits make clones diverge and fragment ("COW and clones",
    2023; it says nothing on where the cost of a clone lies and gives no timing; corrected by the
    fact-check, 13). I found
    no published measurement of clone time against fragmentation on APFS.
  - A macOS computer is three files: the disk, the auxiliary storage (`VZMacAuxiliaryStorage`,
    "information the boot loader needs") and the hardware model and machine identifier. UTM
    copies the auxiliary storage and the EFI variable store with every snapshot and restores them
    with the disk (verified, `snapshotAuxiliaryURLs`). Tart copies `nvram.bin` with every clone
    (verified, `VMDirectory.clone`). WWDC 2026 session 224: "a virtual Mac has an auxiliary
    storage file … remember that you must duplicate those other files" (verified, transcript).
    What happens when a disk boots with another moment's auxiliary storage is not verified.
  - Consistency: on XFS and Btrfs, a clone during writes holds a prefix of the writes (verified,
    PR #347 and the re-run above). On APFS: not measured. The reasoning that it holds: the guest
    only relies on writes it was told are done and on its flushes, and an atomic clone holds a
    prefix of what the host received. The case where it could fail: `synchronizationMode` none,
    if a clone holds only what the host cache wrote out, in any order. That is test B of the
    script.
- **Risks and what would change this:**
  - If test A on the Mac shows APFS clones of a 500,000-piece image under one second, the wait
    is a non-issue and the answer stands as written. If it shows XFS-like numbers, the answer
    still stands, but the checkpoint's speed target has to name the write wait, and the build
    needs one of the mitigations in "Cross-cutting findings".
  - If test B shows a clone that misses writes the guest was told were done, in some mode, Silo
    attaches main's disk only in the modes that pass, or pauses for the copy.
  - If test C shows a macOS guest panicking or reporting I/O errors during a long wait, the wait
    becomes a correctness problem, not a speed one.
- **Proposed text:**

  > answer: On every system, a checkpoint is a copy of the computer's disk that shares blocks with
  > it, taken from outside the computer by the same means as a fork's disk
  > (`fork-shares-disk-blocks`): on a Mac an APFS clone of the disk image, for macOS and Linux
  > computers alike; on a Linux host a reflink copy, or, on a filesystem with none such as ext4,
  > what `fork-shares-disk-blocks` settles. With the disk it copies, at the same moment, the small
  > files the computer's firmware writes: a macOS computer's auxiliary storage, a Linux
  > computer's EFI variables. It takes only the space of what changes after it. Nothing inside the
  > computer takes part, so nothing inside can stop or change it. It holds the disk and not the
  > running programs: what a program had not yet written to the disk is not in it, as after a
  > power cut, and a computer restored to it starts cold. Keeping memory as well is the later
  > feature `checkpoints-with-memory`. The copy is taken while the computer runs, without a pause,
  > when the copy is a moment in time in the way Silo attaches the disk; the computer's writes
  > wait while it is made. How long that wait gets as the disk image fragments, and what keeps it
  > under the checkpoint's speed target, is to find out. If the copy fails, as on a full disk, no
  > checkpoint is made, and the record says so.

- **For the first build:** it blocks `checkpoints-taken-on-their-own`. Take it now; the find-out
  does not stop the build, since the first version clones and measures. What the builder needs:
  attach main's disk with `synchronizationMode` full (Tart's default, verified) until test B says
  otherwise; copy `disk.img` and the auxiliary storage together, with the machine identifier and
  hardware model kept in the checkpoint's own small file; measure the longest write wait at every
  checkpoint and put it in the record, so the growth is seen before it hurts.

## `fork-shares-disk-blocks`: What does a fork store of its parent's disk?

- **Recommendation:** Take as written.
- **Confidence:** high for the Mac half. The ext4 half (qcow2 over the parent) is not needed by
  the first build and keeps its own "to find out".
- **What it decides, on one real case:** in the second build you fork main from the 14:05
  checkpoint to try a dependency upgrade. Silo clones the checkpoint's files, which nothing writes,
  so main does not wait at all; the fork waits for its own clone, as long as a checkpoint of main
  takes at that age (9.8 s against 9.6 s on XFS, under Evidence). The fork gets main's 500,000
  pieces with it.
- **Options:** as in the entry's notes (share, or full copy). Add one: a DiskImageKit stack on
  macOS 27, the checkpoint frozen as a read-only base and an ASIF overlay on top for the fork.
  Tart (`tart clone --stacked`, macOS 27) and UTM use such stacks (verified in their source), but
  neither forks a local VM this way: Tart's `--stacked` takes only a pulled remote image as the
  base, macOS images only, and refuses an image that is already stacked; UTM's layers are its
  snapshots (corrected by the fact-check, 13). It makes a fork in constant time at any age, at the
  cost of a stack Apple cannot merge, and of macOS 27.
- **Evidence:**
  - `fork-per-system` already makes every fork start from a checkpoint, so a fork never clones
    main's live disk (verified, plan).
  - A clone keeps the source's pieces, and cloning a checkpoint makes main wait for nothing
    (verified on XFS: a live image in 486,188 pieces cloned in 9.6 s; cloning that clone took
    9.8 s, and a program writing to the live image meanwhile never waited more than 0.17 s).
  - "What it holds alone" has an API on APFS: `getattrlist` with `ATTR_CMNEXT_PRIVATESIZE` gives
    the bytes of a file held in no clone or snapshot (verified, xnu's `getattrlist.2`). It counts
    per file: the bytes shared only between a computer's own checkpoints are not "alone" by it.
    Eclectic Light: which extents are shared "isn't simple or quick" to find (not verified, seen in
    a search summary of its 5 September 2026 article).
- **Risks and what would change this:** none for the first build. For Linux hosts, the qcow2
  numbers above stand.
- **For the first build:** not needed except through `checkpoint-per-system`, which points to it
  for "the same means". It can be taken as written.

## `checkpoint-storage`: Where is a checkpoint stored?

- **Recommendation:** Take with an edit: "the same APFS volume" in place of "the same drive", and
  one sentence on the order of removal.
- **Confidence:** high.
- **What it decides, on one real case:** your Mac has its internal SSD and an external SSD
  formatted APFS. Silo's folder is on the internal one. A clone to the external drive is a full
  copy, and so is a clone to another APFS volume of the same internal drive: `clonefile` returns
  `EXDEV` across volumes, and Tart users hit it when moving `~/.tart/tmp` to another drive (not
  verified: a pgsql-hackers post, and Eclectic Light saying that clones stay clones only within
  one volume, both seen in search summaries). A checkpoint of main is three or four files (disk,
  auxiliary storage, its description); thinning removes them.
- **Options:**
  - Outside the computer, on its volume, never changed (the answer).
  - Inside the guest, as APFS snapshots (`tmutil localsnapshot`). Costs the way back: an agent
    with `sudo` deletes them with `tmutil deletelocalsnapshots`, Time Machine removes its local
    snapshots after 24 hours, and `deleted` may purge them under space pressure (not verified:
    Apple support article 102154 and Eclectic Light, August 2026, seen in search summaries). A
    complement at most; it adds nothing the outside copy lacks.
  - A snapshot of the Mac's own volume. Constant time at any fragmentation, but
    `fs_snapshot_create` needs `com.apple.developer.vfs.snapshot`, which Apple grants only to
    backup apps after review, and rolling back needs a private entitlement (not verified: Apple
    DTS on the forums, threads 89635 and 768708, seen in search summaries).
- **Evidence:** above; `clonefile(2)`: an overwrite after a clone can return ENOSPC (verified).
  So on a full Mac, main's own disk writes fail, not only the next checkpoint.
- **Risks and what would change this:** a process on your Mac, outside every computer, running as
  you can delete the files. `mac-agent-not-the-main-path` (taken) accepts that; `chflags uchg` on
  each file makes an accidental `rm -rf` fail, and only that.
- **Proposed text:**

  > answer: Silo stores a checkpoint outside the computer, on the machine that runs it, on the
  > same volume as the computer's own disk, since a copy that shares blocks can live only there.
  > Nothing inside any computer can reach it, and it never changes once saved. A checkpoint is
  > removed whole or not at all: by thinning, when its computer leaves the bin for good, or by
  > you. Its description is written last when it is saved and removed first when it is removed, so
  > a checkpoint whose description is missing is not one, and its files are cleared away.

  The property `checkpoint-never-changes` stays as it is.
- **For the first build:** blocks `checkpoints-taken-on-their-own`. Take now.

## `restore-saves-first`: What does a restore do before it goes back?

- **Recommendation:** Take with an edit: say that the save comes after the stop, and keep the
  saved checkpoint out of the hourly thinning.
- **Confidence:** high for the thinning hole (it follows from `thinning-checkpoints` as written);
  medium for the order.
- **What it decides, on one real case:** at 16:00 you restore main to 15:38. A second agent had
  finished a fix at 15:55, uncommitted. The next morning you remember it. `thinning-checkpoints`
  keeps every checkpoint for 2 hours, then one per hour. After 18:00 the hour from 16:00 to 17:00
  keeps one checkpoint. If it keeps 16:55, taken at an interval, the save of 16:00 goes, and it
  was the only copy of the fix: 16:55 and every later checkpoint descend from 15:38, not from
  16:00.
  Thinning by age assumes that a later checkpoint holds what an earlier one led to. After a
  restore that is false for the line of work that was left.
- **Options:**
  - Save first, thinned like the others (the answer). Costs the case above.
  - Save first, kept as long as the daily ones (30 days), or until you remove it. Costs the disk
    that the left line of work holds, for a month.
  - Restore at once. Costs the work since the checkpoint.
- **Order:** a restore is destructive for the running programs anyway (`why-destructive`). If
  Silo stops main first (`what-stop-ends`: the system is asked to shut down, 30 seconds at most)
  and saves after, the saved checkpoint holds what the databases and editors wrote as they shut
  down, the clone makes nothing wait because nothing writes, and the clone costs the same. If it
  saves first, the checkpoint is as after a power cut. The cost of the order is that the save
  waits up to 30 seconds for the stop.
- **Evidence:** UTM restores only a stopped Apple VM and removes the saved memory state of a
  suspended one when its disks go back (verified, `UTMAppleSnapshotBackend.restore`). On XFS a
  clone with no writer takes the same time as with one (verified above, 11.0 s alone, 10.7 s while
  written).
- **Proposed text:**

  > answer: Before a restore, Silo stops the computer, then takes a checkpoint of where you were,
  > so the restore can be undone. That checkpoint is the last moment of the work the restore
  > leaves, so thinning keeps it as long as it keeps the checkpoints of each day. If it cannot be
  > taken, for example because the disk is full, the restore does not happen and Silo says why.
  > You can then restore without saving, as a choice of your own.

- **For the first build:** blocks `restore-to-a-checkpoint`. The thinning part matters only once
  `thinning-old-checkpoints` is built, which is not in the first build, but the sentence is
  cheapest now.

## `what-a-restore-leaves`: When you restore a computer, what stays as it is now?

- **Recommendation:** Take with an edit: one sentence for grants that Silo keeps as files inside
  the computer.
- **Confidence:** medium. The SSH case is certain; how many other grants live on the disk depends
  on later features.
- **What it decides, on one real case:** in the first build Silo reaches main over SSH with a key
  whose public half is in `~/.ssh/authorized_keys` on main's disk. On Tuesday Silo (or you)
  replaces that key. On Wednesday you restore main to Monday. The disk goes back, the old public
  key with it, and Silo's new key is refused: `silo` and `ssh main` both fail until someone gets
  in another way. The same holds for any secret or setting Silo writes into the computer as a
  file. The property `restore-changes-only-the-inside` promises that what was granted or taken
  back stays as it is; for these grants it does not hold unless Silo writes them again.
- **Options:**
  - Only the inside goes back (the answer), with Silo writing again, after each restore and before
    the computer starts, the grants it keeps as files inside. Costs a step in every restore, and
    the way to write into a stopped disk from outside, which the unattended first start already
    needs (`10-macos-unattended-setup.md`).
  - Grants never live as files inside. Costs the ordinary SSH way in, a public key in
    `authorized_keys` on the computer's disk, which is how Lima reaches its machines (not verified
    here).
- **Evidence:** the record goes on across a restore, so the comparison that gives file changes
  (`file-changes-from-outside`) has to compare the first checkpoint after the restore with the one
  restored to, not with the one taken before in time (the saved one). Otherwise every change the
  restore undid shows as a change made by the computer. Not in the plan; a need for the builder of
  `file-changes-in-the-record`.
- **Proposed text:**

  > answer: Only what is inside the computer goes back: its disk and the conversations. The
  > record, what was spent, the rules and precautions, and what you granted or took back stay as
  > they are now. A grant that Silo keeps as a file inside the computer, such as the key that lets
  > Silo in over SSH, is written again after the restore and before the computer starts, so it too
  > stays as it is now.

  The property it sets stays as it is.
- **For the first build:** blocks `restore-to-a-checkpoint`. Take now; the builder of
  `agent-programs-reach-every-computer` needs to know that the key is rewritten on restore.

## `why-destructive`: Why are stop, restore and drop destructive?

- **Recommendation:** Take as written.
- **Confidence:** high.
- **What it decides, on one real case:** you stop main at night. The dev server and the Claude Code
  process end; the disk stays. A rule could do the same (`kinds-of-action`: destructive, so you or
  a rule), a precaution could not.
- **Options:** as in its notes.
- **Evidence:** no new facts. One check of consistency: a checkpoint is protective ("a wrong one
  costs only time"), and the measurements above show that the time is the computer's: a
  precaution that takes a checkpoint makes main's writes wait. That is still only time, so the
  filing holds. It argues for a limit on how often precautions take checkpoints (below).
- **Risks and what would change this:** `checkpoints-with-memory`, as the answer already says.
- **For the first build:** blocks `restore-to-a-checkpoint`. Take now.

## `meanings-for-computers`: What do these words for a computer mean?

- **Recommendation:** Take with an edit of two meanings: checkpoint and pause.
- **Confidence:** medium; these are wording choices on one real case each.
- **What it decides, on one real case:**
  - Checkpoint. A user who knows Parallels or VMware reads "a saved moment of a computer" as the
    running programs too, as those products' snapshots keep them. Silo's restore boots cold.
  - Pause. You pause main at 12:30 while Claude Code waits on a long reply from the API and go to
    lunch. At 13:30 you resume. Inside main nothing was lost, but the API's server has closed the
    connection long ago, so Claude Code sees a network error and retries or stops. "Nothing is
    lost" reads as a promise about the agent's work.
- **Options:** the current meanings, or the two below. The other eight hold on the cases I tried:
  stop for one run or a computer matches `what-stop-ends`; fork matches `fork-per-system`; drop
  and bin match `drop-and-bin`.
- **Proposed text:** in `sets`:

  > {"in":"words","word":"checkpoint","means":"A saved moment of a computer's disk, the agents'
  > conversations included. It never changes once saved."}
  >
  > {"in":"words","word":"pause","means":"Freeze a whole computer: nothing in it runs, nothing
  > crosses its boundary, and its memory is kept. Undone by resume."}

  When `checkpoints-with-memory` is built, "checkpoint" changes with it.
- **For the first build:** blocks four first-build features. The words fork, fresh computer, drop,
  bin and only here are not used by the first build; they can be taken now with the others, since
  they hold.

## Cross-cutting findings

### The checkpoint's speed target has to name the wait inside the computer

`speed-budgets` (taken) gives a checkpoint a target as a 99th percentile and a worst case. If it is
written as "the time of the clone", it misses who pays: every program in main waits for that time,
the agents included, and with `checkpoint-at-each-turn` the model's reply waits too. Proposal for
whoever writes the target: "the longest a write in the computer waits because of a checkpoint",
measured inside the computer, at a named fragmentation (pieces of the disk image), with the clone
time beside it.

### What to do if APFS behaves like XFS, from cheapest to dearest

1. **Measure and show.** Count the pieces of `disk.img` at each checkpoint (`F_LOG2PHYS_EXT`,
   as the script does) and record the longest write wait. Nothing else until the numbers say so.
2. **Make the disk whole when main stops.** When main is stopped anyway (the Mac restarts, you
   stop it, a restore), copy `disk.img` in full to a new file and put it in place: one piece, and
   clones at 0.002 s again. It costs a disk's worth of space until the checkpoints that share the
   old blocks are thinned, and the copy time (32 s for 8 GiB here; on a Mac's SSD at several GB/s,
   seconds to a minute for 60 GB, not measured). It cannot be done while main runs.
3. **A layer per boot (macOS 27).** At each start, the disk as it is becomes a read-only base and
   writes go to a new ASIF overlay; checkpoints clone the overlay, which holds only what was
   written since the start, and the base files, which nothing writes. It caps the pieces at what
   one run of main writes. It costs a stack that grows by one layer per start, which Apple cannot
   merge ("No, but that would make a fine enhancement request", Apple DTS, forum thread 830940,
   verified), so step 2 is still needed now and then; "shallow stacks perform better" (WWDC 2026
   session 224, verified). polarzero's Mac ran macOS 26.5 on 21 November (verified, notes of
   `file-changes-from-outside`), so this needs an upgrade.
4. **A disk of Silo's own over NBD.** Constant-time checkpoints and the list of blocks written
   between two checkpoints, at the price of writing and owning a block store. Only if 1 to 3 fail.

### A precaution that takes checkpoints needs a limit on how often

`what-a-precaution-may-do` (taken) lets a precaution take a checkpoint, as a protective action. An
opinion that fires on every tool call would take a checkpoint on every tool call, and each one is
a wait for every program in main and more pieces. Not in the first build (no opinions), but the
checkpoint feature should refuse checkpoints closer than a set gap, and say so in the record.

### Main's disk on a full Mac

After a clone, overwriting a shared block needs new space (`clonefile(2)`, verified). With
checkpoints holding old blocks, a full Mac makes main's own writes fail, and the guest sees disk
errors, not a refused checkpoint. `low-disk` (open) should act before the Mac is full, with
headroom for main's writes, not only for the next checkpoint.

### A challenge to `whole-computer-grain` (taken)

It says "a whole computer at one moment is consistent". It is consistent as a disk after a power
cut is. A checkpoint taken while Claude Code appends to its conversation file can hold half of the
last line. Whether `claude --resume` takes up a conversation whose last line is cut is what
`conversation-resumes-in-a-copy` has to check, with a checkpoint taken during a write, not after a
clean stop. I would not change the decision, but the check should include that case.

### The Mac test, ready to run

Copy it to a file, `chmod +x`, and run the parts in order. Part A needs no virtual machine. Part B
needs Tart and runs a small Linux computer to test each caching and synchronization mode. Part C
runs a real macOS computer for two to three hours. `results.txt` in `~/silo-disk-trial` is what to
bring back.

```bash
#!/bin/bash
# silo-mac-disk-trial.sh: what an APFS clone of a VM's disk costs and whether it is a moment in time.
# Apple silicon Mac, macOS 26 or later, on the internal SSD. Needs python3 (Xcode command line
# tools) and, for parts B and C, Tart (brew install cirruslabs/cli/tart). No sudo except where
# diskutil asks for it. Free space: 40 GB for A, 60 GB for B, 120 GB for C.
#
#   ./silo-mac-disk-trial.sh A      # clone time against fragmentation, raw and ASIF, no VM (~20 min)
#   ./silo-mac-disk-trial.sh B      # a clone of a disk a running VM writes: moment in time? per mode (~30 min)
#   ./silo-mac-disk-trial.sh C      # a real macOS computer under agent-like work, checkpoint each step (~2-3 h)
#   ./silo-mac-disk-trial.sh clean  # removes the folder and the trial VMs
#
# Everything lands in $W (default ~/silo-disk-trial); results.txt is what to paste back.
set -euo pipefail
W=${W:-$HOME/silo-disk-trial}; mkdir -p "$W"; cd "$W"
GIB=${GIB:-8}
R="$W/results.txt"
log(){ echo "$*" | tee -a "$R"; }
now(){ python3 -c 'import time;print(f"{time.time():.3f}")'; }
since(){ python3 -c "import time;print(f'{time.time()-$1:.3f}')"; }

cat > "$W/h.py" <<'PY'
import fcntl, mmap, os, random, struct, subprocess, sys, threading, time
F_NOCACHE, F_LOG2PHYS_EXT, BS = 48, 65, 4096
def nocache(fd): fcntl.fcntl(fd, F_NOCACHE, 1)
def cmd_fill(img, gib):                       # a disk image written in full, as after an install
    fd = os.open(img, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644); nocache(fd)
    chunk = os.urandom(16 << 20)
    for i in range(int(gib) * 64): os.write(fd, chunk)
    os.fsync(fd); os.close(fd)
def cmd_frag(img, n, seed, gib=None):         # n writes of 4 KiB at random places
    size = int(gib) << 30 if gib else os.path.getsize(img)
    fd = os.open(img, os.O_WRONLY); nocache(fd)
    r = random.Random(int(seed)); buf = mmap.mmap(-1, BS); buf.write(os.urandom(BS))
    for _ in range(int(n)): os.pwrite(fd, buf, r.randrange(size // BS) * BS)
    os.fsync(fd); os.close(fd)
def cmd_extents(img):                         # pieces of the file on the SSD
    fd = os.open(img, os.O_RDONLY); size = os.fstat(fd).st_size; off = n = 0; last_end = None
    while off < size:
        a = struct.pack("=Iqq", 0, size - off, off)
        try: _, contig, dev = struct.unpack("=Iqq", fcntl.fcntl(fd, F_LOG2PHYS_EXT, a))
        except OSError: off += BS; continue      # a hole
        if contig <= 0: off += BS; continue
        if dev != last_end: n += 1
        last_end = dev + contig; off += contig
    os.close(fd); print(n)
def cmd_stall(img, shell):                    # longest 4 KiB write while `shell` runs
    size = os.path.getsize(img); fd = os.open(img, os.O_WRONLY); nocache(fd)
    buf = mmap.mmap(-1, BS); buf.write(os.urandom(BS)); lat = []; stop = [False]
    def w():
        r = random.Random(7)
        while not stop[0]:
            t = time.perf_counter(); os.pwrite(fd, buf, r.randrange(size // BS) * BS); lat.append((t, time.perf_counter() - t))
    th = threading.Thread(target=w); th.start(); time.sleep(1)
    a = time.perf_counter(); subprocess.run(shell, shell=True, check=True); b = time.perf_counter()
    time.sleep(1); stop[0] = True; th.join(); os.close(fd)
    print(f"clone_s={b-a:.2f} longest_write_s={max(l for _, l in lat):.2f} writes_during={sum(1 for t,_ in lat if a<=t<=b)}")
def pos(i, nblk, magic): return (i * 2654435761 + magic) % nblk   # nblk a power of two: no repeats
def cmd_pit(img, magic):                      # host writer: is a clone taken mid-write a prefix?
    magic = int(magic, 16); size = os.path.getsize(img); nblk = size // BS
    fd = os.open(img, os.O_WRONLY); nocache(fd); buf = mmap.mmap(-1, BS); done = [0]; N = 200000
    def w():
        for i in range(N):
            buf.seek(0); buf.write(struct.pack("<QQ", magic, i)); os.pwrite(fd, buf, pos(i, nblk, magic) * BS); done[0] = i + 1
    th = threading.Thread(target=w); th.start()
    while done[0] < N // 4: time.sleep(0.01)
    a = done[0]; subprocess.run(["cp", "-c", img, img + ".pit"], check=True); b = done[0]; th.join(); os.close(fd)
    p, after = verify(img + ".pit", magic, N); os.unlink(img + ".pit")
    print(f"acked_before={a} acked_after={b} prefix={p} found_after_prefix={after} moment_in_time={after==0 and a<=p<=b}")
def verify(img, magic, n):
    fd = os.open(img, os.O_RDONLY); nblk = os.fstat(fd).st_size // BS; first = None; after = 0
    for i in range(n):
        m, k = struct.unpack("<QQ", os.pread(fd, 16, pos(i, nblk, magic) * BS))
        ok = m == magic and k == i
        if not ok and first is None: first = i
        elif ok and first is not None: after += 1
    os.close(fd); return (n if first is None else first), after
def cmd_verify(img, magic, n):
    p, after = verify(img, int(magic, 16), int(n)); print(f"prefix={p} found_after_prefix={after}")
def cmd_flatten(src, dst):                    # full copy that keeps holes: one piece again
    i = os.open(src, os.O_RDONLY); o = os.open(dst, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644); nocache(o)
    size = os.fstat(i).st_size; os.ftruncate(o, size); z = bytes(1 << 20)
    for off in range(0, size, 1 << 20):
        b = os.pread(i, 1 << 20, off)
        if b != z[:len(b)]: os.pwrite(o, b, off)
    os.fsync(o); os.close(i); os.close(o)
globals()["cmd_" + sys.argv[1]](*sys.argv[2:])
PY

# Runs inside a Linux guest: numbered 4 KiB blocks to the second disk, one after another,
# with the count of acknowledged writes and every write slower than 0.2 s kept in /dev/shm.
cat > "$W/gw.py" <<'PY'
import mmap, os, struct, sys, time
dev, magic, sync = sys.argv[1], int(sys.argv[2], 16), sys.argv[3] == "1"
BS = 4096; fd = os.open(dev, os.O_WRONLY | os.O_DIRECT); nblk = os.lseek(fd, 0, 2) // BS
buf = mmap.mmap(-1, BS); c = os.open("/dev/shm/count", os.O_WRONLY | os.O_CREAT); slow = open("/dev/shm/slow", "w")
for i in range(1 << 22):
    buf.seek(0); buf.write(struct.pack("<QQ", magic, i)); t = time.time()
    os.pwrite(fd, buf, ((i * 2654435761 + magic) % nblk) * BS)
    if sync: os.fdatasync(fd)
    d = time.time() - t
    if d > 0.2: slow.write(f"{t:.3f} {d:.3f}\n"); slow.flush()
    os.pwrite(c, b"%12d" % (i + 1), 0)
PY

partA() {
  log "== A $(sw_vers -productVersion) $(sysctl -n machdep.cpu.brand_string) $(date)"
  rm -f a-*.img a-*.asif
  python3 h.py fill a-raw.img $GIB
  total=0
  for n in 0 20000 100000 300000 600000; do
    [ $n -gt $total ] && { python3 h.py frag a-raw.img $((n - total)) $n; total=$n; }
    ext=$(python3 h.py extents a-raw.img); t=$(now); cp -c a-raw.img a-ck$n.img; ct=$(since $t)
    st=$(python3 h.py stall a-raw.img "cp -c a-raw.img a-probe.img"); rm -f a-probe.img
    log "A raw writes=$n extents=$ext clone_s=$ct | under a writer: $st"
  done
  log "A raw moment in time: $(python3 h.py pit a-raw.img 5110a001)"
  t=$(now); python3 h.py flatten a-raw.img a-flat.img; log "A flatten ${GIB}GiB: $(since $t) s, extents=$(python3 h.py extents a-flat.img)"
  t=$(now); cp -c a-flat.img a-flatck.img; log "A clone after flatten: $(since $t) s"
  log "A delete the 5 clones while the live image is written: $(python3 h.py stall a-raw.img 'rm -f a-ck*.img') ; live extents now $(python3 h.py extents a-raw.img)"
  rm -f a-raw.img a-flat*.img
  # ASIF: the image is attached to the host and written through its device, as a VM would
  diskutil image create blank --fs none --format ASIF --size ${GIB}GiB a.asif >/dev/null
  dev=$( (diskutil image attach --noMount a.asif 2>/dev/null || hdiutil attach -nomount a.asif) | awk '/\/dev\/disk/{print $1; exit}'); rdev=${dev/disk/rdisk}
  log "A asif attached at $dev"
  [ -w "$rdev" ] || sudo chown "$USER" "$rdev"
  python3 - "$rdev" $GIB <<'PY'
import os, sys, fcntl
fd = os.open(sys.argv[1], os.O_WRONLY); fcntl.fcntl(fd, 48, 1); c = os.urandom(16 << 20)
for i in range(int(sys.argv[2]) * 64): os.pwrite(fd, c, i * (16 << 20))
os.fsync(fd)
PY
  total=0
  for n in 0 100000 300000; do
    [ $n -gt $total ] && { python3 h.py frag "$rdev" $((n - total)) $n $GIB; total=$n; }
    ext=$(python3 h.py extents a.asif); t=$(now); cp -c a.asif a-asifck$n.asif; ct=$(since $t)
    log "A asif writes=$n file_size=$(stat -f %z a.asif) extents=$ext clone_s=$ct (attached, host writes through diskimagesiod)"
  done
  diskutil image detach "$dev" >/dev/null || hdiutil detach "$dev"
  rm -f a*.asif
}

tart_vm_linux() { tart list | grep -q silo-trial-linux || tart clone ghcr.io/cirruslabs/ubuntu:latest silo-trial-linux; }
gexec() { tart exec "$1" "${@:2}"; }
wait_exec() { for _ in $(seq 1 60); do tart exec "$1" true 2>/dev/null && return 0; sleep 3; done
  echo "tart exec does not answer in $1: the image has no Tart Guest Agent. Use ssh admin@\$(tart ip $1) (password admin) instead." >&2; exit 1; }

partB() {
  log "== B $(sw_vers -productVersion) $(tart --version) $(date)"
  command -v tart >/dev/null || { echo "brew install cirruslabs/cli/tart"; exit 1; }
  tart_vm_linux
  rm -f b-data.img; python3 h.py fill b-data.img $GIB; cp -c b-data.img b-base.img
  python3 h.py frag b-data.img 300000 1                  # so that a clone takes seconds
  log "B data disk extents=$(python3 h.py extents b-data.img)"
  i=0
  for caching in automatic cached uncached; do for sync in full fsync none; do for gsync in 1 0; do
    i=$((i + 1)); magic=$(printf '5110b%03x' $i)
    tart run silo-trial-linux --no-graphics --disk="$W/b-data.img:sync=$sync,caching=$caching" >/dev/null 2>&1 &
    vmpid=$!
    wait_exec silo-trial-linux
    gexec -i silo-trial-linux sh -c 'cat > /tmp/gw.py' < gw.py
    gexec silo-trial-linux sh -c "sudo rm -f /dev/shm/count /dev/shm/slow; sudo nohup python3 /tmp/gw.py /dev/vdb $magic $gsync >/dev/null 2>&1 &"
    sleep 5
    a=$(gexec silo-trial-linux cat /dev/shm/count | tr -d ' '); t=$(now)
    cp -c b-data.img b-ck.img; ct=$(since $t)
    b=$(gexec silo-trial-linux cat /dev/shm/count | tr -d ' ')
    sleep 2; slow=$(gexec silo-trial-linux sh -c 'sort -k2 -n -r /dev/shm/slow | head -1' || true)
    gexec silo-trial-linux sudo pkill -f gw.py || true
    tart stop silo-trial-linux >/dev/null 2>&1 || true; wait $vmpid 2>/dev/null || true
    v=$(python3 h.py verify b-ck.img $magic $((b + 50000)))
    log "B caching=$caching sync=$sync guest_fdatasync=$gsync clone_s=$ct acked_before=$a acked_after=$b $v longest_guest_write=[$slow]"
    rm -f b-ck.img
  done; done; done
  log "B reading: moment in time when found_after_prefix=0 and prefix>=acked_before. If prefix<acked_before, the clone lost writes the guest was told were done."
}

partC() {
  log "== C $(sw_vers -productVersion) $(date)"
  command -v tart >/dev/null || { echo "brew install cirruslabs/cli/tart"; exit 1; }
  V=silo-trial-mac; STEPS=${STEPS:-24}; KEEP=${KEEP:-12}
  tart list | grep -q $V || tart clone ghcr.io/cirruslabs/macos-tahoe-base:latest $V
  D=$HOME/.tart/vms/$V; mkdir -p "$W/c"
  tart run $V --no-graphics >/dev/null 2>&1 & vmpid=$!
  wait_exec $V
  # a probe inside: 4 KiB written and flushed every 0.1 s; writes slower than 0.2 s are kept
  tart exec -i $V sh -c 'cat > /tmp/probe.py' <<'PY'
import os, time, fcntl
fd = os.open("/Users/admin/probe.bin", os.O_WRONLY | os.O_CREAT); b = os.urandom(4096); log = open("/Users/admin/slow.txt", "a")
while True:
    t = time.time(); os.pwrite(fd, b, 0); fcntl.fcntl(fd, 51)  # F_FULLFSYNC
    d = time.time() - t
    if d > 0.2: log.write(f"{t:.3f} {d:.3f}\n"); log.flush()
    time.sleep(0.1)
PY
  tart exec $V sh -c 'nohup python3 /tmp/probe.py >/dev/null 2>&1 &'
  tart exec -i $V sh -c 'cat > /tmp/work.sh' <<'SH'
set -e; k=$1; cd ~
rm -rf "w$((k - 2))"; mkdir -p "w$k"; cd "w$k"
git clone -q --depth 1 https://github.com/expressjs/express e1                 # small files
git clone -q --depth 1 https://github.com/vercel/next.js e2                    # ~100k files
python3 - <<'P'
import os, random, sqlite3
db = sqlite3.connect(os.path.expanduser("~/app.db")); c = db.cursor()
c.execute("create table if not exists t(id integer primary key, v blob)")
if c.execute("select count(*) from t").fetchone()[0] == 0:
    c.executemany("insert into t values(?,?)", ((i, os.urandom(400)) for i in range(1000000)))
for _ in range(20000): c.execute("update t set v=? where id=?", (os.urandom(400), random.randrange(1000000)))
db.commit()
for i in range(50): open(os.path.expanduser(f"~/obj{i}.o"), "wb").write(os.urandom(4 << 20))
P
SH
  for k in $(seq 1 $STEPS); do
    t=$(now); tart exec $V sh /tmp/work.sh $k; wt=$(since $t)
    ext=$(python3 h.py extents "$D/disk.img"); t0=$(python3 -c 'import time;print(time.time())')
    cp -c "$D/disk.img" "$W/c/ck$k.img"; cp "$D/nvram.bin" "$W/c/ck$k.nvram"; ct=$(since $t0)
    [ $k -gt $KEEP ] && rm -f "$W/c/ck$((k - KEEP)).img" "$W/c/ck$((k - KEEP)).nvram"
    slow=$(tart exec $V sh -c "awk -v t=$t0 '\$1>=t-1' ~/slow.txt | sort -k2 -n -r | head -1" || true)
    log "C step=$k work_s=$wt disk_extents=$ext clone_s=$ct longest_flushed_write_in_guest=[$slow] mac_free=$(df -g "$W" | awk 'NR==2{print $4}')G"
  done
  # a checkpoint taken mid-run boots, and the guest's file system checks clean
  mid=$((STEPS - KEEP / 2)); tart stop $V >/dev/null 2>&1 || true; wait $vmpid 2>/dev/null || true
  tart list | grep -q silo-trial-restore && tart delete silo-trial-restore
  tart clone $V silo-trial-restore; RD=$HOME/.tart/vms/silo-trial-restore
  cp -c "$W/c/ck$mid.img" "$RD/disk.img"; cp "$W/c/ck$mid.nvram" "$RD/nvram.bin"
  t=$(now); tart run silo-trial-restore --no-graphics >/dev/null 2>&1 & rpid=$!
  ok=no; for _ in $(seq 1 100); do tart exec silo-trial-restore true 2>/dev/null && { ok=yes; break; }; sleep 3; done
  fsck=$(tart exec silo-trial-restore sh -c '(diskutil verifyVolume / 2>&1 || sudo -n diskutil verifyVolume / 2>&1) | tail -2' || true)
  log "C restore of ck$mid: booted=$ok in $(since $t) s; verifyVolume: $fsck"
  tart stop silo-trial-restore >/dev/null 2>&1 || true; wait $rpid 2>/dev/null || true
  t=$(now); python3 h.py flatten "$D/disk.img" "$W/c/flat.img"
  log "C flatten of the stopped disk: $(since $t) s, extents $(python3 h.py extents "$D/disk.img") -> $(python3 h.py extents "$W/c/flat.img")"
  rm -f "$W/c/flat.img"
}

case "${1:-}" in
  A) partA;; B) partB;; C) partC;;
  clean) for v in silo-trial-linux silo-trial-mac silo-trial-restore; do tart delete $v 2>/dev/null || true; done; rm -rf "$W";;
  *) sed -n 2,12p "$0";;
esac
```

What each line answers:

- A, `raw writes=… clone_s=…`: APFS's cost per piece, next to XFS's 21 µs and Btrfs's 3.6 µs.
- A, `moment in time`: the same check as PR #347, on APFS, for a host writer.
- A, `delete the 5 clones`: whether removing old checkpoints (thinning) also makes main wait.
- A, `asif`: whether an ASIF image fragments and clones like a raw one.
- B: for each of the 18 combinations of caching mode, synchronization mode and guest flush,
  whether a clone of a disk that a running VM writes holds every write the guest was told was done
  (`prefix>=acked_before`) and nothing after a missing one (`found_after_prefix=0`), and the
  longest write the guest saw.
- C: how fast a real macOS guest fragments its disk under agent-like work with a checkpoint per
  step, the longest flushed write inside it during each clone, and whether a checkpoint taken
  mid-work boots and checks clean.

## Sources

- PR #347 of `amontlabs/silo`, its notes on `fork-shares-disk-blocks` and `checkpoint-per-system`
  (read through the GitHub API).
- Apple: `VZDiskImageStorageDeviceAttachment`, `VZDiskImageCachingMode`,
  `VZDiskImageSynchronizationMode`, `VZNetworkBlockDeviceStorageDeviceAttachment`, DiskImageKit
  (https://developer.apple.com/documentation/diskimagekit), WWDC 2026 session 224
  (https://developer.apple.com/videos/play/wwdc2026/224/), `clonefile(2)`, `getattrlist(2)`.
- Apple Developer Forums: thread 830940 (no merge of a stacked image), 89635 and 768708 (snapshot
  entitlements).
- Tart, `github.com/cirruslabs/tart` at `a8d03d4` (17 November 2026): `Clone.swift`,
  `VMDirectory.swift`, `DiskImageStack.swift`, `VM.swift`, `Run.swift`.
- UTM, `github.com/utmapp/UTM` (18 November 2026): `UTMAppleSnapshotBackend.swift`,
  `UTMAppleDiskImage.swift`.
- huven/asif-format (https://github.com/huven/asif-format): ASIF's layout, 1 MiB chunks in the
  images it read (community work, not verified against Apple).
- Eclectic Light: "COW and clones" (2023-05-04), "Should you use Tahoe's new ASIF disk images?"
  (2025-09-17), "Aren't snapshots purgeable?" (2026-08-24), "When the Finder's size 'on disk' is
  misleading" (2026-09-05).
- Parallels KB 128867 and release notes 20.3.1, 20.4.2 (snapshots of macOS guests; not verified how
  they are made). Broadcom KB 315602: Fusion does not run macOS guests on Apple silicon (search summary).
- Bitrise, "WWDC26: the Virtualization framework updates that matter for large Mac fleets".
