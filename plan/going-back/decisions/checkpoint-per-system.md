# What a checkpoint is made of on each system

## The choice

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
