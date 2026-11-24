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
  XFS and 4.5 s on Btrfs. The computer is stopped then, so the time is the user's wait, not a
  stall.
- **A restored disk reads slowly.** Read in order, as a boot reads, the restored disk gave 297 MB/s
  on XFS against 2,482 MB/s for an image written in one go, and 173 against 1,256 MB/s on Btrfs.
  Every checkpoint adds pieces, and nothing joins them again. A host that keeps checkpoints for
  days needs to rewrite a stopped computer's disk in one piece from time to time, at the cost of
  the space it shared.
- **Pausing for a consistent copy.** This entry leaves open whether a copy needs the computer
  paused. libkrun's 2.0 interface (its main branch, read on 24 November 2026) has
  `krun_vmm_handle_pause` and `krun_vmm_handle_resume`, so a pause needs no patch on either host.
