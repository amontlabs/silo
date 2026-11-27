# A Linux host for Silo

Research of 24 November 2026, by an agent. It asks what Silo looks like on a Linux machine that
holds computers: a server a team shares, or a Linux workstation. Two findings make the question
real. A shared team host has to be Linux, since macOS's licence lets one user use a Mac at a time
(#362, `shared-host-system`). And on Linux, a checkpoint's copy of a fragmented disk stops the
computer's writes for 11 to 23 s on XFS, against about 5 times less on Btrfs (#347).

Nothing here is decided. The choices it raises are written as agents' notes in the plan's entries,
listed at the end.

## What was measured, and where

A Linux cloud container: kernel 6.18, 4 processors, 15 GiB of memory, one virtual disk. XFS and
Btrfs were each made on a loop device over the container's ext4 disk. The numbers show how one
option compares with another on the same machine, not what a real server gives.

The container exposes no `/dev/kvm`, though its kernel holds KVM's modules (`kvm`, `kvm_pvm`). So
no virtual machine was booted. Where a measurement needed one, a network namespace stood in for the
computer. The container has no device-mapper and no ZFS module, so dm-thin, LVM thin volumes and
ZFS were not measured.

The scripts are in this folder. They are throwaway code, run as root.

| Script | What it measures |
|---|---|
| `xtenant.sh`, `bwriter.py`, `summ.py` | Whether a checkpoint of one computer's disk makes another computer's writes wait, on the same file system |
| `restore.sh` | How long a restore takes, and how fast the restored disk reads |
| `qcow2.sh` | qcow2 layers on ext4: checkpoint, restore, thinning and reads through 30 layers |
| `net.sh`, `fwd.go`, `rr.py` | Throughput, round trip and new connections per second through three network paths |

## 1. A Linux host needs KVM, and most cloud machines do not give it

Every virtual machine monitor in the running (libkrun, Firecracker, Cloud Hypervisor, QEMU with
acceleration) needs `/dev/kvm` on Linux. A Linux host for Silo is therefore one of:

- a machine Silo owns or rents whole: a workstation, a rented dedicated server, a cloud "metal"
  instance;
- a cloud virtual machine with nested virtualization switched on. AWS added it in February 2026,
  on Intel instances only (C8i, M8i, R8i, then the 7th generation and more in June 2026), with no
  Graviton; Google Cloud and Azure have offered it on Intel for years
  ([AWS user guide](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/amazon-ec2-nested-virtualization.html),
  [The Register, 17 February 2026](https://www.theregister.com/2026/02/17/nested_virtualization_aws_ec2/)).

The container these measurements ran in is the common case: a cloud sandbox with no `/dev/kvm`.
Silo cannot run computers there, and a first run on such a machine has to say so, with what to use
instead.

## 2. Checkpoints on a Linux host

### A checkpoint does not stall the other computers on the same file system

Two disk images on one file system: computer A's, 8 GiB in about half a million pieces (made the
way #347 made them, by random 4 KiB writes after a copy), and computer B's, 2 GiB, which shares no
blocks with A. B writes 4 KiB blocks at random places with a flush every 8 writes, as a busy
computer does. Meanwhile A's image is copied with `cp --reflink=always` three times, 6 s apart.
B's writes are timed one by one, and counted inside the copies and outside them.

| File system | A's copy | B's writes per second, during the copies / otherwise | B's slowest write, during / otherwise | B's 99th percentile, during / otherwise |
|---|---|---|---|---|
| XFS | 10.8 to 12.4 s | 6,700 / 8,200 | 141 ms / 352 ms | 1.0 ms / 0.8 ms |
| Btrfs | 3.9 to 5.4 s | 1,760 / 2,680 | 484 ms / 253 ms | 3.5 ms / 2.9 ms |
| Btrfs, B's image without copy-on-write (`chattr +C`) | 2.6 to 3.8 s | 4,190 / 4,270 | 417 ms / 830 ms | 1.7 ms / 1.7 ms |

- On XFS, the wait of #347 is A's alone. B lost a fifth of its writes per second while the copy
  ran, and its slowest write during a copy was faster than its slowest outside one.
- On Btrfs, B lost a third of its writes per second during the copies.
- Btrfs wrote a third as fast as XFS on this rig, copy or not: each overwrite of a Btrfs file goes
  to a new place (copy-on-write), and each flush writes more. That is a cost Btrfs puts on every
  computer, all the time, and not only at a checkpoint. With copy-on-write off for B's image, B
  wrote half as fast as on XFS and the copies no longer slowed it. Whether an image without
  copy-on-write keeps that speed once it has checkpoints of its own is not measured.
- Holding A's copy to 50 MB/s and 2,000 writes per second with the kernel's I/O throttle (cgroup
  v1 blkio) did not help B: on XFS B was not slowed in that run, and on Btrfs it was slowed more
  (1,190 writes per second). A reflink copy writes the file system's own records, which the
  throttle does not hold back.
- The engine thread found that on XFS, a copy of an old checkpoint that shares blocks with A also
  makes A's writes wait, up to 1.0 s. So the wait follows the shared blocks, not only the file. It
  did not reach a computer whose disk shares nothing with the one copied.

### A restore is a copy, and a fragmented disk reads slowly afterwards

8 GiB image fragmented as above. A checkpoint is taken, the computer goes on writing (20,000 more
random writes), then the disk is put back to the checkpoint. Then the restored disk is read with
the page cache dropped: 4 KiB random reads for 15 s, then 2 GiB in order, as a boot reads.

| | Restore | Random reads (4 KiB, per second) | Reading in order |
|---|---|---|---|
| XFS, reflink copy back | 10.6 to 11.7 s | 11,100 | 297 MB/s |
| XFS, an image never fragmented | | 45,900 | 2,482 MB/s |
| Btrfs, reflink copy back | 4.5 s | 9,400 | 173 MB/s |
| Btrfs, the image in its own subvolume, restored by a snapshot of the checkpoint's snapshot | 0.8 s | 16,900 | 257 MB/s |
| Btrfs, an image never fragmented, same run | | 17,300 | 1,360 MB/s |
| Btrfs, an image never fragmented | | 8,800 | 1,256 MB/s |

- Random reads moved between runs of the same case (8,800 and 17,300 for a fresh Btrfs image), so
  only reading in order is compared here.
- A Btrfs subvolume restore took 0.8 s: it swaps a subvolume and drops the old one, whatever the
  disk's pieces.
- A restore by reflink costs what a checkpoint costs: on XFS about 11 s for a disk in half a
  million pieces. The computer is stopped during a restore, so nothing waits on it; it is time the
  user waits.
- A fragmented disk read in order was 8 times slower on XFS than one written in one go. A computer
  restored to an old checkpoint, or a fork of one, boots from such a disk.

### qcow2 layers on ext4

`fork-shares-disk-blocks` gives qcow2 layers for a host on ext4, which has no reflink. An 8 GiB raw
base, then 30 layers, each made with `qemu-img create -b` and then given 64 MiB of 4 KiB writes at
random places with `qemu-io`, as 30 checkpoints of a running computer would leave them. QEMU's
tools, not libkrun's qcow2 code.

| Step | Time |
|---|---|
| A checkpoint: a new empty layer on top | under 1 s (0.86 s, the first; #347 measured 13 ms) |
| Thinning: merge one layer into the one below it (`qemu-img commit`), then point the next layer at it | 4.4 s |
| Restore to checkpoint 20: delete the 10 layers above it, make a new empty top | 5.2 s, not broken down |
| Rewrite the whole chain as one file | 37.7 s |
| 20,000 reads of 4 KiB spread over the disk, raw base / through 30 layers | 0.47 s / 0.53 s |

- A checkpoint and a restore cost nothing that grows with the disk: no copy, no fragmentation of
  the base. That is what reflink lacks on XFS.
- Thinning costs a copy of what the merged layer holds, here 64 MiB in 4.4 s, and the chain has to
  be rewritten from time to time, here 38 s for 8 GiB, or reads get slower: #347 measured random
  reads twice as slow through 50 layers. The spread reads above mostly fell on the base, which is
  why they barely slowed.
- The catch is the running computer. A new layer must become the one the computer writes to, which
  only the virtual machine monitor can do while it runs. QEMU does it (`blockdev-snapshot`).
  libkrun's interface on its main branch (`include/libkrun.h`, read on 24 November 2026) can pause
  and resume a computer and open a qcow2 disk, but has no call to switch a running disk to a new
  layer. So with libkrun a qcow2 checkpoint needs the computer stopped, or a patch.

### What this means for the choice of file system

- XFS's stall stays inside one computer, so on a shared host one member's checkpoint barely slows
  another member's computer. The stall itself, 10 s and more, is too long for checkpoints
  taken on their own while the computer runs.
- Btrfs's checkpoint is 3 to 5 times shorter, and a whole-subvolume snapshot was 0.4 to 0.6 s in
  the engine thread. It costs every write of every computer more, all the time.
- Neither a full reflink copy nor a Btrfs snapshot fixes fragmentation: it comes from the sharing
  itself. A host that keeps checkpoints for days needs a way to rewrite a computer's disk in one
  piece when it is stopped (defragment, or copy and drop the sharing), at the cost of the space
  it shared. On both file systems the disk read 7 to 8 times slower in order once fragmented,
  which is what a computer restored or forked from an old checkpoint boots from.
- qcow2 layers avoid the copy and the fragmentation of the base, but need the virtual machine
  monitor to switch layers while the computer runs, which libkrun cannot do yet.
- The engine thread's copy-before-write (a write log kept by Silo while the copy is made) kept the
  longest write at 0.16 to 0.31 s on XFS. With it, the file system matters less for the stall, and
  XFS's faster everyday writes count for more. That is the engine's choice, measured there.

## 3. Network: what Silo's own stack costs on Linux

`network-stack` says every packet of a computer goes through Silo's network stack, and that what
it costs in speed is to find out. Three paths were measured from a namespace "vm" (the computer) to
a namespace "srv" (the internet), with iperf3 and a 1-byte round trip. `fwd.go` plays the part of
the virtual machine monitor: it reads the computer's frames from a tap device and hands them to the
user-space stack over a unix socket, with the 4-byte length framing that libkrun's
`krun_add_net_unixstream` uses for passt and gvproxy. Three runs each; the median.

| Path | Computer to internet | Internet to computer | Round trip (median / 99th) | New connections per second | Memory |
|---|---|---|---|---|---|
| Kernel: veth, host forwarding, an nftables allow-list | 14.7 Gbit/s | 14.3 Gbit/s | 36 / 78 µs | about 5,000 | |
| passt (2024-02, Ubuntu 24.04's) | 1.3 to 3.0 Gbit/s | 13.3 Gbit/s | 102 / 260 µs | about 2,800 | 31 to 41 MB |
| gvproxy 0.8.7, MTU 1500 | 1.4 Gbit/s | 0.96 Gbit/s | 187 / 420 µs | about 1,580 | 21 MB |
| gvproxy 0.8.7, MTU 65520 | 4.1 Gbit/s | 3.0 Gbit/s | 193 / 380 µs | about 1,490 | 21 MB |

- The kernel path is the fastest by far, but it cannot read a request, hold a secret or keep a
  record of a call: nftables sees addresses and ports. It is what Firecracker's tap gives, and it
  is not Silo's boundary.
- A user-space stack costs 3 to 15 times the throughput and 3 to 5 times the round trip. A
  developer's computer rarely moves more than 1 Gbit/s, so a download of `npm install` or a
  container image is not the limit; a test suite that moves data between two computers through the
  boundary is.
- gvproxy at MTU 1500, the setting Podman machine and most libkrun tools use, is the slowest:
  under 1 Gbit/s into the computer. Raising the MTU tripled it.
- passt is fast into the computer and slow out of it in this rig.
- These numbers are a floor for Silo's own stack, not a forecast: the #347 boundary spike and the
  #363 strict reader parse HTTP, which neither passt nor gvproxy does.

## 4. Which virtual machine monitor on a Linux host

`linux-vm-engine` proposes libkrun on the Mac and on Linux. On a Linux host the alternatives are
mature, so the question is what each would change. From each project's documentation, read on
24 November 2026; nothing here was run.

| | libkrun | Firecracker | Cloud Hypervisor | QEMU |
|---|---|---|---|---|
| Runs on a Mac too | yes | no | no | yes, heavier |
| Network to a user-space stack | unix socket (passt, gvproxy); tap and vhost-user in the 2.0 interface | tap only | tap, or vhost-user (a stack can be the backend) | everything |
| Disk formats | raw, qcow2 | raw only | raw, qcow2, VHD | everything |
| Memory snapshots (fork of a running computer) | no; upstream pull request waits for 2.0 | yes, full and diff (diff is a preview) | yes, with on-demand restore | yes |
| Shared folders (virtio-fs) | yes | no | yes | yes |
| Built for many tenants on one host | one process per computer | the jailer: own uid, chroot, cgroup, seccomp | one process per computer, seccomp | sandboxing options |
| A screen | virtio-gpu | no | no | yes |

- Keeping libkrun on Linux keeps one engine for both systems (`macos-and-linux-consistent`), and
  the unix-socket network is the route Silo's boundary already takes on the Mac. libkrun's 2.0
  interface (its main branch, read on 24 November 2026) adds a call that checks for nested
  virtualization, a balloon device with free page reporting, and pause and resume of a running
  computer, which a checkpoint may need for a consistent copy.
- What a Linux host loses with libkrun is the fork of a running computer, the case of
  `linux-server-host` ("twenty forks of a Linux main"). Cloud Hypervisor has it today, with qcow2
  and vhost-user so that Silo's stack can still sit at the boundary. It costs a second engine to
  build on and keep alike.
- Firecracker fits least: tap only puts the boundary behind the kernel's network, raw disks only
  rules out the ext4 fallback, and it has no virtio-fs.

## 5. Many users on one host

A sketch for `shared-host-system` (#362), from the measurements above and how Firecracker's jailer
works. Not built.

- Each member's computers run as processes of a Unix user of their own, one process per computer,
  in a control group of their own with a memory limit and processor and I/O weights. A member's
  process can open only its own disk files and `/dev/kvm`.
- Each computer's network stack is its own process under that user, so nothing of one member's
  traffic passes through a process of another's. No tap device sits on a shared bridge.
- Each member's disks sit in their own directory, or their own Btrfs subvolume. Space per member is
  a quota of the file system (XFS project quotas, Btrfs quota groups), and the plan's sizes per
  computer (what it holds alone, what it shares) are counted inside it.
- A checkpoint of one member's computer does not stall another member's computer on XFS, and slows
  it by a third on Btrfs while it runs (section 2). The I/O throttle on the copy did not help.
- What the record learns about a computer from outside is wider on Linux: the control group of a
  computer's process gives its processor, memory and disk totals as the host sees them, a fact
  in the plan's sense, beside the report the probe sends from inside (`processes-from-inside-linux`).

## 6. The boundary and the record on a Linux host

Nothing about them changes with the host:

- The boundary is the user-space stack the computer's frames go to. libkrun hands them over the
  same unix socket on both systems, so the #347 boundary spike and the #363 strict reader run on a
  Linux host as they are, with gvproxy's `-sealed` patch of #363 or passt behind them.
- The record's sources inside a Linux computer (`processes-from-inside-linux`) do not depend on the
  host. On a Linux host Silo can add the control group's totals of each computer, read from
  outside, as above.

## Entries this informs

Agents' notes were added, with no answer, `sets` or verdict changed, to:

- `linux-server-host`: what a Linux host needs (KVM, a file system), and who it is for.
- `linux-vm-engine`: the alternatives on a Linux host, and the case for keeping libkrun.
- `network-stack`: the measured cost of a user-space stack.
- `checkpoint-per-system`: neighbours, restore time, reads after a restore, qcow2.
- `fork-shares-disk-blocks`: which file system to tell a Linux host's owner to use.
