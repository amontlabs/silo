# What a checkpoint is made of on each system

## The choice

An agent's part, 25 November 2026, from the challenge of #365, with #352, #366 and #359, as #364 (section 10)
reconciles them.

The case: main's 8 GiB disk image is in 485,000 pieces after weeks of checkpoints. With a checkpoint
at each turn, every write in main waits for the whole clone of the image: 9.2 to 11.8 seconds on XFS
(#365), and 11 seconds for one 4 KiB write at 500,000 pieces (#352). How long on APFS is the Mac kit's
test 01.

The options:

- From Silo's own list of the blocks written where Silo carries the writes, and a clone where it does
  not (recommended). #365's prototype: the checkpoint's moment under 1 µs, the longest write during
  a checkpoint 0.16 to 0.31 seconds, and every checkpoint held exactly the writes up to its moment.
  #366 agrees on a Linux host. It costs a block layer of Silo's own in the path of every write, and
  for a macOS computer a disk served over NBD, which nobody has booted macOS from yet: Mac kit tests
  31 and 32 check it.
- A clone of the image, as the earlier answer says. It costs main's writes stalling longer as main
  ages.
- A volume snapshot. The fastest on Btrfs, but an APFS snapshot needs an entitlement Apple grants
  only to backup apps.

Recommended: the list of blocks where Silo carries the writes. Until tests 31 and 32 pass, the clone
stays the way on a Mac, and if it stalls main's writes on APFS as on XFS, a checkpoint at each turn
needs #352's minimum gap. DiskImageKit layers per checkpoint are not an option: #352 read UTM's source,
and the layers of a running disk cannot change.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. At 14:00 Silo takes a checkpoint of main while a dev server runs
on port 3000 and Claude Code is in the middle of a reply. At 15:00 you restore main to 14:00.

The options:

- The disk only (recommended). Main comes back with its disk as it was at 14:00, the agents'
  conversations included, and starts from a cold boot. The dev server is not running until something
  starts it, and what a program had not yet saved is lost, as after a power cut. Each checkpoint
  takes only the disk space of what changes after it. It costs the running programs. Whether a
  checkpoint of a running computer needs it paused for a moment is still to find out.
- The disk and the memory, for macOS computers now. A macOS computer comes back with its programs
  and its screen as they were. It costs disk up to the size of the computer's memory for each
  checkpoint, a pause while it is written, and checkpoints that a macOS update can make unusable.
  Linux computers cannot do it yet, so the two systems would differ.

Recommended: the disk only. Checkpoints are taken often and on their own, so each must stay light
and work the same on both systems; keeping the memory is the later feature
`checkpoints-with-memory`.

## Notes

The answer is worded from the table of what can be seen per system, which comes from the plan's
notes:

| | Linux computer | macOS computer |
|---|---|---|
| Checkpoint | A snapshot of the disk. Memory later. | A clone of the disk. A memory state can be saved, and is refused after some updates of the host. |

A checkpoint that keeps memory is the later feature `checkpoints-with-memory`.

## A checkpoint that does not stop the computer's writes (24 November 2026)

An agent's part, 24 November 2026, from the Zig prototype in
`research/2026-11-24-checkpoint-engine/` (Linux, XFS and Btrfs, no Mac). It adds an option the
answer does not have, for polarzero to weigh when the Mac's numbers come in.

The case: main's 8 GiB disk image is in 485,000 pieces after weeks of checkpoints, as #352 found.
A stand-in for the computer writes 4 KiB blocks, 1,000 a second; its usual write takes 0.2 ms.

- **A clone of the image (the answer).** Every write waited 9.2 to 11.8 s per checkpoint, the whole
  clone, and with a checkpoint at each turn the turn waits as long. How long on APFS is #359's
  test 01.
- **Silo sees the writes.** Silo marks each block written in a bitmap, where the writes pass: in
  libkrun's virtio-blk for a Linux computer (Silo patches libkrun already, `libkrun-patches`), or
  in a disk Silo serves over NBD for a macOS computer, which Apple's framework allows
  (`VZNetworkBlockDeviceStorageDeviceAttachment`, macOS 14). The checkpoint's moment is a swap of
  two bitmaps between two writes, under 1 µs; Silo then copies the marked blocks into a clone of
  the last checkpoint, and a write to a block not yet copied copies it first. The longest write
  during a checkpoint was 0.16 to 0.31 s, and every checkpoint held exactly the writes up to its
  moment. The bitmap is also the list of blocks each checkpoint changed. It costs a block layer of
  Silo's own in the path of every write, and for a macOS computer the NBD route, which nobody has
  shown to boot macOS or measured on a Mac.
- **A volume snapshot.** The best on a Linux host with Btrfs (0.42 to 0.56 s, writes waiting up to
  0.37 s), but on a Mac an APFS snapshot needs an entitlement Apple grants only to backup apps.

What is left of the 0.16 to 0.31 s comes from cloning the last checkpoint, which still shares
blocks with the live image: on XFS that alone made the live image's writes wait up to 1.0 s. A
variant that clones nothing while the computer runs, each checkpoint a file of only the blocks it
copied, is in the prototype too (`delta`): its checkpoints took 0.45 to 1.06 s instead of 12 to
16 s, the computer stopped for 40 to 50 µs, and six of eight kept the longest write under 160 ms,
near the disk's own noise of 16 to 85 ms. Reading it as a whole disk means applying the deltas to
the first checkpoint, about 15 to 25 s for the 8 GiB disk, which a restore does with the computer
stopped anyway.

A challenge to the answer, on that case: "taken from outside the computer by the same means as a
fork's disk" ties a checkpoint to a clone of the image, and the clone is what makes main wait,
longer as main ages. The proposal: a checkpoint is taken from Silo's own record of the blocks
written where Silo sees them, and by a clone where it does not; the "to find out" becomes the three
Mac tests in the research's README (a clone of a file sharing blocks with the live image, an NBD
disk under a macOS computer, and what a write refused for lack of space does to the computer).

## On a Linux host: neighbours, restore and reads afterwards (24 November 2026)

An agent's part, 24 November 2026. Measured in a Linux cloud container, XFS and Btrfs on loop
devices, an 8 GiB disk image in about half a million pieces as #347 made it. The scripts and the
full tables are in `research/2026-11-24-linux-host/`. The engine thread measures the wait inside
the computer being copied; this is what happens around it.

- **Other computers do not wait.** While one computer's disk was copied, another computer writing
  on the same file system kept four fifths of its writes per second on XFS and two thirds on
  Btrfs, and its slowest write was no slower than outside the copy. Holding the copy back with the
  kernel's I/O throttle did not help.
- **A restore costs what a checkpoint costs.** Copying the checkpoint back took 10.6 to 11.7 s on
  XFS and 4.5 s on Btrfs. With the disk in a Btrfs subvolume of its own, restored from a snapshot
  of the checkpoint's snapshot, it took 0.8 s. The computer is stopped then, so the time is the user's wait, not a
  stall.
- **A restored disk reads slowly.** Read in order, as a boot reads, the restored disk gave 297 MB/s
  on XFS against 2,482 MB/s for an image written in one go, and 173 to 257 against 1,256 to
  1,360 MB/s on Btrfs.
  Every checkpoint adds pieces, and nothing joins them again. A host that keeps checkpoints for
  days needs to rewrite a stopped computer's disk in one piece from time to time, at the cost of
  the space it shared.
- **Pausing for a consistent copy.** This entry leaves open whether a copy needs the computer
  paused. libkrun's 2.0 interface (its main branch, read on 24 November 2026) has
  `krun_vmm_handle_pause` and `krun_vmm_handle_resume`, so a pause needs no patch on either host.

## Is a copy of a running disk consistent? (24 November 2026)

An agent's part, 24 November 2026. Measured on Linux, for XFS and Btrfs reflink copies; the details
are in the notes of `fork-shares-disk-blocks`.

- Yes, with no pause asked of the computer: a reflink copy taken while the disk was being written
  held every write up to one point and none after it, on both file systems.
- The file system makes the disk's writes wait while it copies. That wait grows with how
  fragmented the disk is, and the copies themselves fragment it: 11 to 23 s on XFS and 2.6 to
  5.8 s on Btrfs for a disk of about half a million pieces.
- APFS on the Mac is not measured yet.

## Layers of a disk image on macOS 27 (24 November 2026)

An agent's part, 24 November 2026. Read, not tried. A note above found that on XFS a reflink copy
of a disk in many pieces stopped the computer's writes for seconds. macOS 27 gives a second way to
take a checkpoint of a macOS computer's disk, besides an APFS clone: DiskImageKit, "a new framework
in macOS 27" ([session 224](https://developer.apple.com/videos/play/wwdc2026/224/)). A disk is a
stack: a base of ASIF or raw, then ASIF layers on top, and "overlay layers can be used to implement
copy-on-write semantics for snapshots". Writes go to the top layer, read-only layers "can be shared
by multiple concurrent stacks", and the stack is attached with
`VZDiskImageStorageDeviceAttachment(diskImage:)`.

For Silo: a checkpoint is a new empty top layer, and the layer below stops changing, so nothing is
copied and no clone of a large file is needed while the computer runs. A fork is a new top layer
over the same layers (`fork-shares-disk-blocks`). The cost is the depth: Apple says "shallow stacks
perform better", so checkpoints every few minutes need layers merged in the background, and a
merge rewrites blocks. A fork must also copy the auxiliary storage of a virtual Mac, which is not
in the stack. What to measure on a Mac: reads at a depth of 10, 100 and 1,000 layers, and the time
and stall of a merge, next to the APFS clone of the first-build test.

## A checkpoint that does not share blocks

An agent's part, 25 November 2026, from [option B's specs](../../../research/2026-11-25-linux-main/README.md). #365 measured its `delta` way best: the blocks the
computer overwrites after the checkpoint's moment, copied just before each write, so nothing is
cloned while the computer runs and its writes do not wait. That is not "a copy of the computer's
disk that shares blocks with it", so the answer as written excludes it. Proposed first sentence:
"On every system, a checkpoint is a copy of the computer's disk at one moment, taken from outside
the computer: either a copy that shares blocks with the disk, by the same means as a fork's disk
(`fork-shares-disk-blocks`), or the blocks the computer overwrites after that moment, copied just
before each write, where Silo sees the computer's writes." Silo sees them for a Linux computer
through a hook in libkrun's block device, and for a macOS computer only if its disk is served over
NBD.
