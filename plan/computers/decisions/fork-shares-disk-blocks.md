# What does a fork store of its parent's disk

## The choice

An agent's part, 22 November 2026. Main's disk holds 60 GB, of which 25 GB is build output. You fork
main to try a dependency upgrade.

The options:

- Share the disk (recommended). The fork exists in seconds and takes almost no space. It grows only
  by what it changes, say the 3 GB it rebuilds. It costs simple sizes: a computer's size is two
  numbers, what it holds alone and what it shares, and dropping a computer does not free what its
  forks still share. On a Linux server with the most common Linux file system, the fork reads its
  disk through one more layer, and how much slower that is, is to find out.
- A full copy. The fork is a separate 60 GB on disk and owes nothing to main. It costs 60 GB and
  minutes of copying for every fork, the way VMware and VirtualBox make a full clone.

Recommended: share the disk. Forks are made to try something and then dropped, and 60 GB each would
rule them out on most Macs.

## Which file system a Linux host should use (24 November 2026)

An agent's part, 24 November 2026. From measurements in a Linux cloud container, with QEMU's
tools for qcow2; the scripts and tables are in `research/2026-11-24-linux-host/`. It does not
change the answer, which already names Btrfs, XFS and qcow2 on ext4.

| On the host | A checkpoint of a disk in half a million pieces | Every write, all the time | Note |
|---|---|---|---|
| XFS, reflink | that computer's writes wait about 11 s | fastest (about 8,200 small writes per second) | the wait stays inside the computer copied |
| Btrfs, reflink | 3 to 5 s | a third of XFS's | other computers lose a third of their writes during the copy |
| ext4, qcow2 layer | under 1 s, no copy | through the layers | the computer must be stopped: libkrun cannot switch a running disk to a new layer |

- With the engine's copy-before-write (the engine thread, 24 November 2026), the wait on XFS falls
  under 0.3 s, and XFS's everyday speed decides. Without it, Btrfs's shorter wait does.
- qcow2 has no fragmentation of the base and no copy, but every few checkpoints a layer has to be
  merged into the one below it (4.4 s for 64 MiB of changes) and the chain rewritten from time to
  time (38 s for 8 GiB), or reads slow down (#347: twice as slow through 50 layers).
- Not measured, because the container had no device-mapper and no ZFS: dm-thin and LVM thin
  volumes, and ZFS volumes. All three snapshot a block device rather than a file, at a fixed cost
  that does not grow with fragmentation, and would be the next to try on a real server.
