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

## Measured on a Linux host (24 November 2026)

An agent's part, 24 November 2026. Measured in a cloud container: Ubuntu 24.04, kernel 6.18, XFS
and Btrfs each on a 30 GB loop device over the container's ext4 disk, qcow2 files on that ext4
disk. The numbers show how things grow, not what a real server gives. Not measured: APFS, and
libkrun's own qcow2 code (QEMU's tools were used).

### A reflink copy takes longer as the disk gets fragmented

An 8 GiB disk image was written in full, copied with `cp --reflink=always`, then written to with
4 KiB writes at random places, as a computer's file system does, and copied again after each
round.

| 4 KiB writes since the first copy | Pieces of the image (extents) | Copy on XFS | Copy on Btrfs |
|---|---|---|---|
| 0 | 2 to 14 | 0.004 s | 0.007 s |
| 20 000 | about 40 000 | 0.8 s | 0.16 s |
| 100 000 | about 190 000 | 4.3 s | 0.7 s |
| 300 000 | about 500 000 | 11.6 s | 1.9 s |

- The time grows in step with the number of pieces: about 24 µs a piece on XFS, 3.6 µs on Btrfs.
- The copies themselves cause the pieces. Before the first copy, a write lands in place. After it,
  each 4 KiB write to a shared part goes to a new place, and the image gains a piece. Deleting the
  older copies does not join the pieces again.
- XFS's hint for larger copy-on-write allocations (`cowextsize` of 1 MiB) did not help: 186 279
  pieces after 100 000 writes, against 186 261 without it.
- 300 000 writes of 4 KiB is 1.2 GB written at scattered places. How many a real computer makes in
  a day of agent work is not measured.

### Writes wait while the copy is made, and the copy is a moment in time

A program wrote 4 KiB blocks to the image one after another, each holding its number, at random
places, and the image was copied while it wrote. The copy was then read back.

| | XFS, direct writes | XFS, through the page cache | Btrfs, direct writes | Btrfs, through the page cache |
|---|---|---|---|---|
| Copy time | 13.8 s, and 22.6 s on a later run | 11.5 s | 2.6 s | 5.8 s |
| Writes finished during the copy | 31, and 19 | 566 | 32 | 34 |
| The copy holds every block up to some number, and none after it | yes | yes | yes | yes |

- On both file systems the copy is a moment in time, with nothing more to do: it holds what the
  host had received up to one point, like a disk after a power cut. What the computer had not yet
  sent to the host is not in it, as `checkpoint-per-system` already says.
- The cost is that writes to the disk wait for the whole copy. The writer above would have made
  thousands of writes in that time, and made a few dozen.
- The images held about half a million pieces each. The later XFS run, on an image the earlier
  runs had fragmented further, took longer.

### A qcow2 layer over its parent, on ext4

- Making a layer took 13 ms.
- A chain of 50 layers was made, each with 2 000 writes of 4 KiB at random places. 4 000 reads of
  4 KiB at random places took about 0.35 s through the base alone and about 0.75 s through all 50
  layers, about twice as long. Opening the chain took 87 ms against 9 ms.
- Each scattered 4 KiB write into a layer took 64 KiB of disk, qcow2's default cluster: 2 000
  writes made a layer of about 128 MB, not 8 MB.

### What this means for the answer

- A fork or a checkpoint on a reflink copy is not "in seconds" for good. It slows down as the
  computer gets older, and for that whole time the computer's disk writes wait. On XFS, a disk with
  half a million pieces stopped its writes for 11 to 23 seconds per checkpoint, which an agent would
  see as a frozen computer. Btrfs did the same work five to six times faster.
- A qcow2 layer costs the same at any age, but each checkpoint adds a layer to read through, and
  64 KiB of disk per scattered write.
- APFS clones on the Mac, Silo's first host, may behave like XFS here. That is the measurement to
  make next, before the checkpoint's speed target (`speed-budgets`) is written: the same script on
  a Mac, an 8 GiB image, `cp -c` in place of `cp --reflink=always`.

Two ways to keep the pieces down, neither tried: copy the live disk in full now and then while
the computer sleeps, which gives one piece again at the price of the space it shared; or make the
computer's own file system write in larger blocks.
