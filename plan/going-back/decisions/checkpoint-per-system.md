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

## Compression needs checkpoints as Silo's own files

An agent's part, 25 November 2026, from `research/2026-11-25-checkpoint-disk-use/`. A 512 GB Mac with an agent rewriting 2 GiB an
hour keeps 4.3 days of checkpoints under #389's limit. Compressed with zstd, the same blocks take
24% of their size, and the same Mac keeps 26.3 days. An APFS clone cannot be compressed or
deduplicated: APFS has no block dedup, and its file compression is undone when the file is written.
So that gain exists only when checkpoints are files Silo writes, as in #365's `delta`.

In #365's `delta`, the oldest checkpoint is still a clone. It held 25% of the checkpoints' space at
2 GiB an hour and 68% on a macOS computer without TRIM, which brings the gain down to 13.3 days, and
to 3.3 instead of 11.3 without TRIM. Copying each block's old content into the open checkpoint's
file before the first write to it leaves no clone: every byte held is compressible, and removing the
oldest checkpoint deletes one file. Its cost is that a damaged live image damages every checkpoint,
which one compressed full copy renewed each week bounds.

A challenge to the answer: "a copy of the computer's disk that shares blocks with it" would rule
out compression. Proposed instead: a checkpoint is a file of the blocks it alone holds, written and
compressed by Silo outside the computer, with one full compressed copy as an anchor.
