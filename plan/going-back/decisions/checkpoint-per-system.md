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
