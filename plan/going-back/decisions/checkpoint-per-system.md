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

## Is a copy of a running disk consistent? (24 November 2026)

An agent's part, 24 November 2026. Measured on Linux, for XFS and Btrfs reflink copies; the details
are in the notes of `fork-shares-disk-blocks`.

- Yes, with no pause asked of the computer: a reflink copy taken while the disk was being written
  held every write up to one point and none after it, on both file systems.
- The file system makes the disk's writes wait while it copies. That wait grows with how
  fragmented the disk is, and the copies themselves fragment it: 11 to 23 s on XFS and 2.6 to
  5.8 s on Btrfs for a disk of about half a million pieces.
- APFS on the Mac is not measured yet.
