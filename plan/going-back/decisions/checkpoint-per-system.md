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
