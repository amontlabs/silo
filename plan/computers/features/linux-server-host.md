# A Linux server as host

## The choice

An agent's part, 22 November 2026. You run Silo on a Linux server with 128 GB of memory. Twenty
forks of a Linux main try twenty fixes at once, on Intel processors, and you follow them from your
Mac.

The options:

- Build it (recommended). Silo runs computers on a Linux server as on a Mac. It costs a second host
  system to build and test, and a way into Silo from your Mac to the server that has to be guarded.
- Leave it out. Silo runs only on your Mac. It costs the many-forks case and Intel Linux computers,
  and it goes back on what polarzero decided: that Silo also runs on Linux, and that computers run on a
  server after your own machine.

Recommended: build it, in its later stage. A Mac holds a few computers, and a server is where the
rest go.

## What a Linux host needs (24 November 2026)

An agent's part, 24 November 2026. Measured in a Linux cloud container and read from each
project's documentation; the scripts and every number are in `research/2026-11-24-linux-host/`.
Nothing here changes the recommendation above.

- **KVM.** Every virtual machine monitor Silo could use needs `/dev/kvm` on Linux. That rules out
  most cloud sandboxes: the container these measurements ran in has KVM's modules in its kernel
  and no `/dev/kvm`. A host is a workstation, a rented dedicated server, a cloud "metal" instance,
  or a cloud virtual machine with nested virtualization on: AWS added it in February 2026 on Intel
  instances only (C8i, M8i, R8i, then more), and Google Cloud and Azure have it on Intel. A first
  run on a machine without `/dev/kvm` has to say so and name these.
- **The file system of its disks.** Checkpoints and forks share blocks with a reflink, so the host
  needs Btrfs or XFS for them (`fork-shares-disk-blocks`). Measured: on XFS a checkpoint of a disk
  in half a million pieces makes that computer's writes wait about 11 s, and a restore takes as
  long; on Btrfs it is 3 to 5 times shorter, but every write of every computer is slower, all the
  time. The engine's copy-before-write keeps the wait under 0.3 s on XFS, which makes XFS the
  better host if it is built. See `checkpoint-per-system`.
- **Many people on one host** (`shared-host-system`, proposed in #362). One member's checkpoint
  did not make another member's computer wait on XFS (it lost a fifth of its writes per second,
  and no write waited longer than outside the copy), and cost it a third of its writes per second
  on Btrfs. The kernel's I/O throttle on the copy did not help. The sketch: each member's computers run as a Unix user of
  their own, one process per computer in a control group of its own, and each computer's network
  stack is its own process, so no member's traffic passes through a process of another's.
- **The case above, twenty forks of a running Linux main**, needs memory snapshots, which libkrun
  lacks (`forks-of-running-linux`). Until they land, the twenty forks boot cold from a checkpoint.
  Cloud Hypervisor has them today on Linux; what that would cost is in `linux-vm-engine`.

## A host five people share (25 November 2026)

An agent's part, from measurements in a Linux container
([`research/2026-11-25-shared-linux-host`](../../../research/2026-11-25-shared-linux-host/README.md)).
It keeps the recommendation and splits what a host several people share needs into entries that
can be decided one by one:

- Keeping people apart: `members-on-a-host` (a Unix user and a memory limit per person),
  `memory-merging-across-people` (KSM off), `disk-share-on-a-host`.
- The network: `computer-known-by-its-link` (on a shared bridge, a computer that took a stopped
  computer's address got its owner's cloud token), `host-out-of-reach`.
- Secrets and records: `secrets-on-a-shared-host`, `records-on-a-shared-host`. The host's
  administrator can read both, and Silo says so.
- Spend: `pooled-budget-ledger`, exact on one host.
- Size: `team-host-size`, 128 GB, 16 cores and 4 TB for five.
