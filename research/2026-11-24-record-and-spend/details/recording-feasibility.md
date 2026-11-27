# Recording a computer from outside: what can be done

Agent F, 24 November 2026. This is research for polarzero. It edits nothing under `plan/`. Every id
in backticks is an entry of the plan, and `plan show <id>` prints it. Where a statement comes from
memory rather than from a page read today, it says "(not checked today)". Where a number is my own
arithmetic and not a measurement, it says "(arithmetic)".

The question: how much of what the record should hold (processes, file changes, connections) can
Silo see from outside a computer, for a macOS computer under Apple's Virtualization framework and a
Linux computer under libkrun. Then what each way costs, what an agent with root inside can do
against it, and the smallest version that answers "what changed in main overnight".

## In short

- **File changes can be facts on both systems, at a finer grain than the plan assumes.** The plan
  compares checkpoints by walking file metadata (`file-changes-from-outside`). Two properties of
  the filesystems make this both exact and cheap. APFS never writes a changed object in place and
  stamps each one with a transaction number that is never reused, so the parts of the tree that
  changed since a checkpoint can be found without walking the rest. ext4 gives each inode a
  generation number when it is created. That tells a reused inode number from a rename, and it
  removes the 4,939 renames out of 10,000 deletes that the trial got wrong.
- **libkrun can track which disk blocks a Linux computer wrote, with a patch of about 30 lines.**
  Every guest write passes through one `match` arm in libkrun's block device
  (`src/devices/src/virtio/block/worker.rs`, `VIRTIO_BLK_T_OUT`). A bitmap set there is the
  changed-block tracking that QEMU, VMware and Hyper-V ship. With it, a file whose bytes were
  changed and whose times were put back is caught. So is a write that root makes straight to
  `/dev/vda` past the filesystem. The plan's metadata comparison misses both, and the second takes
  one `dd` command.
- **Apple's framework gives no view of the blocks, the memory or the processes of a macOS
  computer.** The only debug stub is behind the private entitlement `com.apple.private.virtualization` (AMFI part
  confirmed by third-party write-ups, SIP part not verified),
  which needs SIP and AMFI off on your own Mac. Silo cannot ship that. Two other ways are left.
  Silo can serve the disk itself over NBD (`VZNetworkBlockDeviceStorageDeviceAttachment`, macOS
  14), which costs speed nobody has measured. Or it can find what changed from the APFS structures
  on the disk image after each checkpoint. That needs no runtime hook.
- **Processes from outside are within reach for Linux computers, and the plan's reason to drop the
  feature is half wrong.** `processes-watched-from-outside` says it "means reading the computer's
  memory and following every version of its kernel". Silo ships the kernel (libkrunfw, pinned at
  6.12.115), so there is one version, with its symbols known at build time. libkrun already
  handles breakpoint exits from the guest under Apple's Hypervisor framework
  (`EC_AA64_BKPT` in `src/hvf/src/lib.rs`), and its bindings include
  `hv_vcpu_set_trap_debug_exceptions`. A hardware breakpoint on the kernel's exec and exit paths
  gives every process start, seen by the engine. Root inside cannot remove it without running code
  in the kernel. For macOS computers the conclusion stands: there is no way.
- **The stock libkrunfw kernel cannot run Tetragon, Falco or Tracee.** Its aarch64 configuration
  has `CONFIG_BPF_SYSCALL=y` but no tracepoints, no kprobes, no BTF, no `CONFIG_BPF_JIT` and no
  audit. It has the process connector and fanotify. So `processes-from-inside-linux` means Silo
  builds its own kernel configuration, and the eBPF overhead numbers published by vendors, all
  measured with the JIT on, do not carry over.
- **On macOS, Endpoint Security gives what the record needs and resists root better than any Linux
  probe, but only when Apple grants Silo the entitlement.** Apple's DTS confirms that an ES client
  can be a plain launchd daemon (forum thread 791996, July 2025), that the entitlement is tied to a
  team, and that "you'll [not] get a definitive answer" on DevForums about who gets it (forum thread
  820718, March 2026). With SIP on in the
  computer, root cannot unload a system process the way it can detach an eBPF program, and an ES
  client can refuse signals sent to itself (`ES_EVENT_TYPE_AUTH_SIGNAL`). The route without the
  entitlement, turning SIP off in the computer, broke once already: macOS 26.5 guests refused
  SIP changes in Recovery until the release candidate, which fixed it for Anka while one UTM user
  still saw it (FB22649912, April to May 2026).
- **A macOS computer already writes its own log of file changes to its disk.** `fseventsd` keeps
  paths and flags (created, removed, renamed, modified) in `/System/Volumes/Data/.fseventsd`.
  Forensic tools read it from a disk image: FSEventsParser, `fse_dump`. Silo can read it from the
  checkpoint with nothing running inside. It catches a file created and deleted between two
  checkpoints, which the comparison misses. It is a report, because root can switch it off with a
  `no_log` file, and the comparison shows when the report goes quiet while files change.
- **No product records a whole machine from outside it.** Each one that records processes, files
  and connections (Harden-Runner, Leash, Teleport, Sysdig, osquery, Santa) either sits in a kernel
  the watched program has no root on, or admits that root defeats it. Teleport says so in its
  documentation. Harden-Runner's answer was to take `sudo` away from the job, and the docker group
  then gave it back (CVE-2025-32955, fixed in 2.12.0). Silo cannot take root away
  (`full-access-inside`), so it has to rely on facts seen outside and on reports checked against
  them, as the plan already decided.
- **The cheapest version that answers "what changed overnight" needs nothing inside main.** Main
  is paused for about 2 seconds while its image is cloned. A reader in user space walks the APFS
  Data volume of two checkpoints and prints one JSON line per created, changed, renamed or deleted
  path. The boundary adds the hosts reached and the model calls. Everything in it is a fact.

## 1. What the plan already says

| Entry | Status | What it settles for this question |
|---|---|---|
| `what-the-record-holds` | taken | Facts and reports are written once and make up the record; opinions are computed apart |
| `the-chain` | taken | trace → process is a report, process → run an opinion |
| `report-matched` | taken | "matched" and "mismatch" |
| `what-runs-wrote` | taken | The books balance |
| `file-changes-from-outside` | open | Compare the disk at each checkpoint, from outside. Rename written as delete plus create. Five things to find out |
| `file-changes-in-the-record` | open, suggest take | File changes as facts, grain of checkpoints |
| `processes-from-inside-linux` | open | eBPF, process connector, systemd scopes, fanotify; all reports |
| `processes-from-inside-macos` | open | ES with permission in the image, then `eslogger`, then sampling |
| `processes-watched-from-outside` | open, suggest drop | "Nobody knows how to do it on a macOS computer" |
| `macos-ships-facts-first` | open | macOS ships with facts only if reports are not ready |
| `record-halves-per-system` | open | Outside is the same on both systems; inside differs |
| `matching-reports-to-facts` | open | Exact keys, marks counted by the probe, never nearness in time |
| `record-overhead-limit` | open | At most 5 percent on a build or a parallel test suite |
| `record-speed-cost` | open | Measure each source apart, on both systems |
| `losses-show`, `gaps-in-the-record` | open | A loss is a gap; a silence is a fact |
| `record-under-a-flood` | open | Merge new lines, never evict old ones |
| `checkpoint-per-system` | open | A checkpoint is an APFS clone of the disk image on a Mac, for both systems |
| `linux-vm-engine` | open | libkrun with Silo's patches |
| `macos-vm-driver` | taken | Apple's framework, driven from Swift |

Two trials of 21 November measured the routes for file changes on Linux computers. Their numbers
are in the notes of `file-changes-from-outside` and are not repeated here. The first-build proposal
of 24 November (`/mnt/project-files/research/2026-11-24-first-build.md`) lists reading a hostile
APFS disk as a risk not in the plan. This report builds on both and does not redo them.

## 2. File changes from outside

### 2.1 Four places a change can be seen

A file write in a computer leaves traces in four places that Silo can reach from outside.

| Place | What it shows | Who wrote what Silo reads | Kind |
|---|---|---|---|
| Disk blocks | Which 4 KiB blocks changed | The engine sees the write itself | Fact |
| Filesystem structures on the disk | Paths, sizes, times, inode numbers | The computer's kernel wrote them; root can write them too, directly | Fact about the disk; the names are the disk's own account |
| The computer's own logs on its disk | On macOS, `.fseventsd`: paths and flags of past changes | A program inside (`fseventsd`) | Report |
| A folder the engine serves | Every operation as it happens | The engine | Fact; the process number in it is the kernel's word |

The plan uses the second place only. The first and third make it better at little cost, and the
sections below say how.

### 2.2 Which blocks changed

**Prior art.** Every server hypervisor has changed-block tracking for incremental backups.

- QEMU keeps dirty bitmaps per disk. `block-dirty-bitmap-add` starts one, at a granularity such as
  32 KiB. A bitmap saved in a qcow2 file survives a restart; "only qcow2 supports persistent
  bitmaps". An incremental backup then copies only the marked clusters
  ([QEMU `docs/interop/bitmaps.rst`](https://qemu.googlesource.com/qemu/+/refs/tags/v8.0.5/docs/interop/bitmaps.rst)).
- VMware calls it Changed Block Tracking and Hyper-V calls it Resilient Change Tracking. Backup
  products such as Veeam read changed blocks through them (not checked today).

**libkrun.** Silo carries patches to libkrun (`libkrun-patches`). Every write from a Linux computer
to its disk arrives in one place. In `src/devices/src/virtio/block/worker.rs` (libkrun at
`715c1c9`, 23 November 2026), `process_request` handles `VIRTIO_BLK_T_OUT` with the sector and the
length. `VIRTIO_BLK_T_DISCARD` and `VIRTIO_BLK_T_WRITE_ZEROES` are in the same `match`. A bitmap of
4 KiB blocks set in those three arms, and swapped at each checkpoint, is the whole patch. A 100 GB
disk needs a bitmap of 3.2 MB (arithmetic: 100 GB / 4 KiB = 26 million bits). The cost per write is
setting bits, which no build will notice. libkrun reads qcow2 through the `imago` crate (version
0.2.6 in `src/devices/Cargo.toml`) as well as raw images. The bitmap sits above the format, so it
works for both.

**Apple's framework.** `VZDiskImageStorageDeviceAttachment` takes a file and hides the block layer.
There is no callback per write and no dirty bitmap (Apple's documentation lists none, read today).
Three ways remain.

1. **Serve the disk over NBD.** Since macOS 14, `VZNetworkBlockDeviceStorageDeviceAttachment`
   connects the computer's disk to an NBD server that "runs outside of and isn't controlled by"
   the framework ([Apple](https://developer.apple.com/documentation/virtualization/vznetworkblockdevicestoragedeviceattachment)).
   If Silo runs that server, it sees every block write, as libkrun would. The NBD URI format
   defines `nbd+unix:///export?socket=…` for a local socket
   ([NBD URI spec](https://repo.or.cz/nbd/ericb.git/blob/HEAD:/doc/uri.md)). Whether Apple's client
   accepts that scheme or only `nbd://host:port` is not documented; `validate(_:)` would tell. The
   cost is unknown and is the whole question. Every block of main would go through a socket and a
   server of Silo's. A bug in that server stops main's disk. I found no measurement of it.
2. **Compare where the two clones' blocks are.** A checkpoint is an APFS clone of the image file
   (`checkpoint-per-system`). After the clone, every block main writes is copied to a new place on
   your Mac's SSD, because the block is shared. Asking APFS where each range of the two clones
   lives (`fcntl(F_LOG2PHYS_EXT)`) would show which ranges diverged without reading any data. I
   found no source confirming that `F_LOG2PHYS_EXT` reports the physical address of a cloned range
   on APFS, so this is a test to run: `cp -c` an image, write to a few blocks of one copy, and
   compare the addresses. The trial of 21 November saw something close. With MicroSandbox on the
   Mac, "each checkpoint adds a layer that holds exactly the disk blocks written since the last one.
   The host reads them in about 0.1 s".
3. **Need no blocks at all.** For a macOS computer the filesystem itself says what changed since a
   transaction number. Section 2.3 explains it. This is the route I recommend first, because it
   needs nothing that Apple might take away.

### 2.3 From blocks to files

**APFS, inside a macOS computer.** Three facts from Apple's
[APFS reference](https://developer.apple.com/support/apple-file-system/Apple-File-System-Reference.pdf)
(2020-06-22 edition, read today):

- A physical object is "modified by writing the copy to a new location on disk". Nothing is
  changed in place.
- Every object header has `o_xid`, "the identifier of the most recent transaction that this object
  was modified in". Transaction numbers are "monotonically increasing" and "aren't allowed to restart
  from one or to be reused".
- The object map, which says where each node of a volume's file tree lives, is keyed by
  `(ok_oid, ok_xid)`. A node of the file tree that changed has a new entry in the object map with
  a newer transaction number.

So the walk that finds changes between two checkpoints is this. Read the transaction number of the
earlier checkpoint from its container superblock. Walk the later checkpoint's object map, and skip
every subtree whose `o_xid` is not newer. Because the object map is copied on write, a node with an
older number has nothing newer under it. The nodes left are exactly the file-tree nodes that
changed. Read them in both checkpoints and compare their records: inodes, directory entries,
extents, extended attributes. The work follows the number of changes, not the size of the disk.
This is how `zfs diff` works with block birth times, and how Btrfs answers `find-new` with
generation numbers (not checked today). I found no tool that does it for APFS. It has to be
written. A reader that does the full walk first, and this pruned walk later, is a fine order.

What the walk gives that the plan says it cannot:

- **Renames are exact.** An APFS inode number is the file's object identifier. The volume
  superblock holds `apfs_next_obj_id`, "the next identifier that will be assigned". The reference
  states that transaction and document identifiers are never reused, but does not say so of object
  identifiers. If they are never reused, a file whose identifier appears under a new name is a
  rename, with no guessing. A test settles it: create and delete 100,000 files in a macOS computer
  and check that no identifier comes back.
- **A file whose bytes changed but whose times were put back is caught.** Its extent records or
  the blocks they point to changed in a newer transaction. Root can set times; root cannot write
  through APFS without a new transaction number.

**ext4, inside a Linux computer.** ext4 writes in place and has no transaction number per object,
so it needs the block bitmap of 2.2.

- **Changed inodes.** Each block group's descriptor gives the place of its inode table. A changed
  block that falls in an inode table holds 16 inodes of 256 bytes. Silo reads those 16 in both
  checkpoints and compares them.
- **Renames and reused numbers.** Each ext4 inode carries `i_generation`, set when the inode is
  allocated, so that NFS can refuse a stale file handle after the number is reused
  ([linux-ext4, June 2017](https://lore.kernel.org/lkml/20170629045940.GB5865@birch.djwong.org/)).
  The same number with a new generation is a new file. The same number and generation under a new
  name is a rename. The trial took 4,939 of 10,000 deletes for renames because it compared numbers
  only. Whether the current kernel takes the generation from a counter or at random was not
  checked today; either way it changes on reuse.
- **Names.** An ext4 inode does not hold its name. Silo keeps a map from inode to path built at the
  first checkpoint and updates it from the directories whose blocks changed. Changed directory
  blocks also give the deletes and creates by name.
- **A write past the filesystem.** Silo also keeps a map from block to the inode that owns it, from
  the extents of the previous checkpoint. A changed data block whose owner inode did not change is
  a write that went around the filesystem. Root can do it with `dd of=/dev/vda`. It can also come
  from an application writing a file through `mmap` before the kernel updates the times (not
  checked today). Either way it is worth a line of its own: "block 81,233,410 of /usr/bin/git
  changed, the file's times did not".

### 2.4 Reading a disk that a hostile agent wrote

Root inside a computer can write any byte of its disk. A crafted filesystem is the input a reader
meets. Mounting a checkpoint in your Mac's kernel hands that input to the kernel. Four ways keep it
out.

| Reader | Filesystem | Where it runs | State, as read today |
|---|---|---|---|
| [`ext4-view`](https://docs.rs/crate/ext4-view/latest) | ext4 | Library in Silo's process or a helper | Read-only, `forbid(unsafe_code)` in the crate, stated goal "invalid data should never cause crashes, panics, or non-terminating loops"; version 0.9.3. No audit found |
| `debugfs -c` (e2fsprogs) | ext4 | Command | Mature, C; was used by the trial of 21 November |
| [libfsapfs](https://github.com/libyal/libfsapfs) | APFS | Library, C | Calls itself "experimental"; read-only; APFS version 2, compression, encryption with a password or recovery key. No statement on sealed volumes |
| apfs-fuse (sgan81) | APFS | Command, C++ | Read-only (not checked today) |
| linux-apfs-rw, in a throwaway Linux computer | APFS | Kernel of a Linux computer that Silo boots for the job | Out-of-tree module, packaged by Debian (0.3.18) |

The last row is worth a test. A small Linux computer under libkrun boots in under a second (not
checked today), mounts both checkpoints read-only, runs the comparison and prints JSON. If the
crafted disk takes over its kernel, it takes over a computer with no network and no secrets, which
is dropped after. Apple's limit of two macOS computers at once does not count it, because it runs
Linux. The trial of 21 November did the same thing with "a second computer". The plan should say
which kind, because on a Mac with main and one fork running, a third macOS computer is refused.

Whichever reader is used, three rules apply. The reader runs in its own process with no network.
It has a budget of time and memory per checkpoint, as `record-under-a-flood` asks. It is fuzzed,
as `fuzz-untrusted-parsers` asks. When it gives up, the record writes a gap for that checkpoint and
never "nothing changed".

**Encryption.** An agent with root and the user's password can turn on FileVault in a macOS
computer. Eclectic Light reports that FileVault works in macOS virtual machines since Sequoia, at a
cost: 4.66 and 3.11 GB/s read and write with it, 6.62 and 5.91 without, on an M4 Pro with macOS
26.4 ([Eclectic Light, 27 March 2026](https://eclecticlight.co/2026/03/27/does-disk-storage-speed-limit-macos-virtual-machines/)).
The reader then sees only encrypted blocks. That must be a gap ("main's Data volume is encrypted
since checkpoint 14:05; file changes are not seen"). The same holds for an encrypted disk image or
sparse bundle made inside: the image file changes, its contents are not seen.

### 2.5 How long a comparison takes on a 50 to 100 GB disk

No one has measured it on a macOS computer. These are the numbers that bound it.

| Way | What it reads | Bound |
|---|---|---|
| Read both images in full and compare | 2 × 100 GB | A macOS computer's own disk reads at 6.62 GB/s on an M4 Pro (above). On the host, about 30 s for 200 GB at that speed, and the SSD busy all that time (arithmetic) |
| Walk all file metadata of both checkpoints | Every inode and directory entry | The trial: 3.3 to 3.9 s on Linux, about 22 s on the Mac (15 s with the earlier list kept), for 200,000 files. The time follows the number of files, not the changes |
| Changed blocks, then their files | The changed blocks only | The trial: 4.4 s to compare two copies block by block on Linux, then 0.25 s for 10 changed files and 32.5 s for 50,000. With a bitmap from libkrun the 4.4 s goes away |
| APFS pruned walk (2.3) | The object-map path to each changed node | A night with 50,000 changed files touches at most a few thousand 4 KiB nodes, some tens of MB (arithmetic). Not built |

A macOS Data volume holds hundreds of thousands of files before any work is done in it (not checked
today; Xcode alone adds many). At the trial's Mac rate, a full walk of 1 million files would take
about 110 s (arithmetic, from 22 s per 200,000). That is too slow for checkpoints every few
minutes, and it is why the pruned walk matters on macOS more than on Linux.

### 2.6 Changes between two checkpoints, with no probe

The comparison misses a file created and deleted between two checkpoints. The plan accepts that
(`file-changes-in-the-record`). On a macOS computer, the system records those changes anyway.
`fseventsd` writes compressed pages of events to `/System/Volumes/Data/.fseventsd/`, each with a
path, flags such as `ItemCreated`, `ItemRemoved`, `ItemRenamed`, `ItemIsFile`, an event number and
the file's node identifier ([FSEventsParser](https://awesome.ecosyste.ms/projects/github.com%2Fdlcowen%2FFSEventsParser),
[`fse_dump`](https://github.com/kornelski/fse_dump)). CrowdStrike found events four months old on a
typical Mac and used them to find malware files that had been deleted
([CrowdStrike](https://www.crowdstrike.com/en-us/blog/using-os-x-fsevents-discover-deleted-malicious-artifact/)).

Silo can read these files from each checkpoint with the same reader, after the computer wrote them,
with nothing running inside for it. What they give:

- A file that existed between two checkpoints, by path, with its flags.
- Renames, as pairs of events.
- No process. FSEvents does not name one.

What they are: a report, told by a program inside. Root can stop it with a `no_log` file in the
folder, or delete the pages. `fseventsd` also keeps events in memory before it writes a page, so
the last minutes before a checkpoint may be missing (not checked today how long). The comparison
checks this report. A checkpoint whose file facts show changes while `.fseventsd` shows none for
the same paths gets a mismatch. A `no_log` file appearing is itself a file fact.

Linux has nothing like it on disk. The ext4 journal holds copies of recently changed metadata
blocks, which forensic tools use, but it is overwritten within seconds of busy work (not checked
today).

### 2.7 A folder the engine serves

The trials measured this and the plan rejected it for builds: 1.6 to 2.1 times slower, and 12.6 to
192 times slower on small files (`file-changes-from-outside`). Two facts add to it.

- With Apple's framework, Eclectic Light measured a shared folder at 5.0 GB/s read and 3.6 GB/s
  write on a Mac Studio with macOS 13, against 5.4 and 0.7 for the computer's own disk at the time
  ([Eclectic Light, December 2022](https://eclecticlight.co/2022/12/01/disk-performance-of-lightweight-macos-vms-on-apple-silicon/)).
  Large sequential transfers are not the problem. Builds and `npm install` are many small
  operations, which is where the trials measured the cost.
- With libkrun, each request already arrives with the process number the computer's kernel gives
  it (`Context` in `src/devices/src/virtio/fs/filesystem.rs`, noted in
  `processes-from-inside-linux`). For a folder that is not a build folder, such as a folder of
  secrets or `~/.ssh`, serving it gives a live fact per operation with the writer's process number.
  That is a narrow use worth keeping in mind. It is not a way to see the whole disk.

### 2.8 What the computer has not written to its disk

A checkpoint holds what reached the disk. On Linux, the trial found 1,000 files invisible after
5 s and visible after 40 s. That matches the kernel's default `vm.dirty_expire_centisecs` of 30 s
plus a writeback pass every 5 s (not checked today). Root can raise those values and keep data in
memory for longer. Nothing outside can catch it without reading the computer's memory.

Two answers, neither perfect:

- The probe runs `sync` just before each checkpoint. It is a program inside, so a hostile agent can
  stop it, and then the checkpoint is as before. The checkpoint line says whether the probe
  confirmed a sync. This costs a few lines and harms nothing.
- Data that stays in memory past one checkpoint reaches the disk by a later one, unless it was
  deleted first. The record then places the change one checkpoint late. That is a property of the
  grain, already stated.

## 3. Processes

### 3.1 From outside the computer

**Prior art.** Reading a guest's processes from its hypervisor is called virtual machine
introspection.

- [LibVMI](https://github.com/libvmi/libvmi) reads guest memory and registers on Xen and on KVM.
  Its KVM driver needs KVMi, patches that are not in mainline Linux
  ([KVM-VMI](https://github.com/KVM-VMI/kvm-vmi): "2 different/incompatible VMI patches for KVM").
- [DRAKVUF](https://tklengyel.github.io/drakvuf) traces guest system calls without an agent inside,
  by breakpoints the guest cannot see. It needs Xen and an Intel processor with EPT ("not going to
  work on any other CPUs").
- Volatility 3 lists processes from a memory image. For Linux it needs a symbol table that matches
  the exact kernel, which can now be made from the kernel's BTF with `btf2json`
  ([iblue.team](https://www.iblue.team/memory-forensics-1/volatility-plugins/generate-custom-profile-using-btf2json)).
  The hard part of all of these is matching every kernel version a guest might run.

**Apple's Virtualization framework.** It offers no memory access, no vCPU registers and no event
per process. A GDB stub exists, `VZGDBDebugStubConfiguration`, used by the Go wrapper `vz`. It
needs the private entitlement `com.apple.private.virtualization`, SIP turned off on the host, and
`amfi_get_out_of_my_way=1` in the host's boot arguments
([Code-Hex/vz wiki](https://github.com/Code-Hex/vz/wiki/How-to-debug-guest-with-virtualization-framework)).
That is your Mac with its protections off, which Silo must never ask for. `saveMachineStateTo`
writes a paused computer's memory to a file, but one developer reports the file is protected by the
keychain, and Apple documents no format
([forum thread 782007](https://developer.apple.com/forums/thread/782007)). A macOS computer cannot
run under anything but Apple's framework on Apple silicon (not checked today; no other monitor
boots macOS guests). **Conclusion for macOS: processes from outside are not possible, and the
suggestion to drop `processes-watched-from-outside` is right for macOS.**

**libkrun.** Here Silo owns the monitor. On a Mac, libkrun drives Apple's Hypervisor framework
directly, maps the computer's memory into its own process (`map_memory` with `hv_vm_map` in
`src/hvf/src/lib.rs`), and handles each exit. On a Linux host it uses KVM.

- **Breakpoints the computer cannot see.** The Hypervisor framework can send debug exceptions and
  accesses to debug registers to the monitor (`hv_vcpu_set_trap_debug_exceptions`,
  `hv_vcpu_set_trap_debug_reg_accesses`, both in libkrun's bindings). QEMU uses them for its GDB
  stub under HVF ([patches by Francesco Cagnin, Quarkslab, 2022 to 2023](https://lore.kernel.org/all/20221116174749.65175-1-fcagnin@quarkslab.com/)).
  KVM has the same with `KVM_SET_GUEST_DEBUG` (not checked today). A hardware breakpoint on the
  kernel function that completes an exec, and one on `do_exit`, stops the vCPU at each process
  start and end. The monitor reads the process number, the parent and the file name from the
  computer's memory, then resumes. With the debug registers trapped, the computer cannot read or
  clear them.
- **One kernel.** Silo ships the kernel through libkrunfw, pinned today at 6.12.115 in its
  Makefile. The offsets of `task_struct` and the addresses of the functions are known when Silo
  builds it. The usual problem of introspection, every kernel of every guest, does not arise.
  Address layout randomisation is undone by reading the exception vector base register, which the
  monitor can read (not checked today), or by booting with `nokaslr`, which the computer cannot
  change since the host gives the kernel command line.
- **What root inside can do.** Without running code in the kernel, nothing: no module can load
  (`CONFIG_MODULES` is off in libkrunfw), and `/dev/mem` is limited (`CONFIG_STRICT_DEVMEM=y`).
  With a kernel exploit, everything. That is a different bar from a probe, which root stops with
  `kill -9`.
- **Cost.** One exit per exec and per exit. A build that starts 3,000 processes in a minute causes
  6,000 exits. An exit and a few memory reads cost microseconds (not checked today for HVF), so a
  few tens of milliseconds a minute (arithmetic). Not measured.

A simpler variant with the same bar: Silo patches its own kernel to report each exec, fork and exit
through a hypervisor call (`HVC`), which libkrun already handles for PSCI (`EC_AA64_HVC` in
`src/hvf/src/lib.rs`). It needs no breakpoints, works the same on KVM, and gives up nothing: it is
code in a kernel that root cannot change. It is a patch to carry in libkrunfw, which
`libkrun-patches` covers.

**Is it a fact?** `words.json` defines a fact as "one thing Silo saw itself, from outside the
computer" and a report as "one thing a program inside the computer told Silo… An agent with full
access can interfere with it". The breakpoint route fits the first: the engine stops the vCPU and
reads the computer's memory itself. Its content still comes from the computer's kernel, as the
names in a file fact come from the filesystem the kernel wrote. The hypervisor-call variant is told
by the kernel, so it reads as a report under the words, although root cannot interfere with it
without a kernel exploit. The plan should decide which of the two it builds knowing this. Proposal:
the breakpoint route, written as facts with the source "engine: kernel events", and the record
states once that this source trusts the computer's kernel. A kernel exploit is outside what Silo
can see in any case.

### 3.2 From inside a Linux computer

**What the stock kernel has.** The configuration `config-libkrunfw_aarch64` (libkrunfw at
12 November 2026) has:

| Option | Value | What it allows |
|---|---|---|
| `CONFIG_PROC_EVENTS` | y | The process connector: fork, exec, exit by netlink |
| `CONFIG_FANOTIFY` | y | File events that name the process |
| `CONFIG_CGROUPS`, `CONFIG_MEMCG` | y | Groups per run, with totals |
| `CONFIG_BPF_SYSCALL`, `CONFIG_CGROUP_BPF` | y | eBPF on groups and sockets |
| `CONFIG_PERF_EVENTS` | y | |
| `CONFIG_TRACEPOINTS`, `CONFIG_BPF_EVENTS` | absent | No eBPF on tracepoints |
| `CONFIG_KPROBES`, `CONFIG_FTRACE` | not set | No eBPF on kernel functions |
| `CONFIG_DEBUG_INFO_BTF` | absent | No CO-RE: Tetragon, Falco's modern probe and Tracee will not load |
| `CONFIG_BPF_JIT` | not set | eBPF runs interpreted |
| `CONFIG_AUDIT` | not set | No audit |
| `CONFIG_MODULES` | not set | Nothing can be loaded |
| `CONFIG_SECURITY_LOCKDOWN_LSM` | not set | |

So with the stock kernel, the probe has the process connector, fanotify and groups. That is what
`processes-from-inside-linux` names as the fallback. The eBPF route it names first needs a kernel
Silo configures itself. That is allowed ("which of these means it is built with is Silo's choice"),
but it should be written down as work, with BTF, tracepoints and the JIT.

**The process connector's weak point.** It says "exec, pid 4412" and not what was run. The probe
reads `/proc/4412/cmdline` after the event, and a process that ends first is gone. A test suite
that starts 300 short processes will leave some exec lines with no command. The record must say
"command not read" on those lines, not drop them. A kernel event from 3.1 carries the file name in
the event and has no such race.

**Published costs of the eBPF tools.** All vendor numbers, all with the JIT on.

| Tool | Number | Source |
|---|---|---|
| Tetragon | Exec tracing adds 1.68% to a kernel build (1.5 million events at 2,600 a second); 2.46% when every event is also written as JSON | [Isovalent, 31 October 2023](https://isovalent.com/blog/post/tetragon-release-10) |
| Tetragon | Watching TCP connections cuts the connection rate of netperf `TCP_CRR` by 5.88% | same |
| Falco | A maintainer: up to 100,000 events a second "with acceptable drop rates", 60,000 to 70,000 "should likely be no problem" | [falcosecurity/libs issue 1557](https://www.github.com/falcosecurity/libs/issues/1557) |
| auditd | A script ran in 0.6 s without rules and 4.4 s with a set of `execve`, `exit`, `fork`, `clone` rules on an SSD | [Red Hat KB 3201282](https://access.redhat.com/node/3201282) |
| Leash | Third-party write-ups say 15 to 20% slower `npm install`, or 5 to 15%; StrongDM says under 1 ms per decision | [starlog](https://starlog.is/articles/ai-agents/strongdm-leash); no primary benchmark found |
| fanotify from the host, in Silo's trial | About 2% | `file-changes-from-outside` |

Tetragon's number fits `record-overhead-limit` (5 percent). Falco's guidance on drops matters for
`losses-show`: a build that starts 50,000 processes in a burst will make an eBPF probe drop lines,
and each drop must become a gap with its count.

**What root inside does.** Teleport's documentation says it plainly of its own eBPF recording:
"Users who have access to root can disable Enhanced Session Recording in a variety of manners"
([Teleport](https://goteleport.com/docs/enroll-resources/server-access/guides/bpf-session-recording/)).
For Silo's probe, root can kill it, detach its programs (`bpftool link detach`), raise its own
limits, or send false lines over its channel. The kernel's audit system has an immutable mode
(`auditctl -e 2`) that root cannot undo until reboot (not checked today), but libkrunfw has no audit
and the audit daemon is still a process root can kill. The plan already treats all of this as
reports; the checks of 3.4 are what make tampering show.

### 3.3 From inside a macOS computer

**Endpoint Security, as a daemon.** Apple's DTS, July 2025: "we absolutely support daemon-based ES
clients. And my experience is that the majority of ES products are based on a daemon rather than a
sysex" ([forum thread 791996](https://developer.apple.com/forums/thread/791996)). What it takes:

- The entitlement `com.apple.developer.endpoint-security.client`, which is restricted. The binary
  sits in an app-like bundle with an `embedded.provisionprofile` of type Developer ID.
- Full Disk Access. Without it `es_new_client` returns `ES_NEW_CLIENT_RESULT_ERR_NOT_PERMITTED`.
  It is granted by the user in System Settings or by an MDM profile with `SystemPolicyAllFiles`. A
  local profile is refused unless it comes from MDM
  ([Huntress](https://www.huntress.com/blog/ask-the-mac-guy-whats-the-deal-with-full-disk-access)).
- Apple's approval of the entitlement, which is discretionary. DTS, March 2026, to a solo developer
  of a tool that watches AI agents: "I don't think you'll get a definitive answer to this here on
  DevForums… 'Suck it and see.'" The capability is "assigned to a specific team", so a fork of
  Silo's code would need its own ([forum thread 820718](https://developer.apple.com/forums/thread/820718)).
  Silo should apply now. The answer decides most of `processes-from-inside-macos`.

**Granting Full Disk Access with nobody clicking.** The grant lives in the system TCC database on
the Data volume. SIP protects that file while macOS runs. Silo writes the computer's disk before
its first boot (first-build proposal, point 2, for Setup Assistant), when SIP is not running. So
Silo could write the TCC row for its daemon then, as it would write the user and the SSH service.
Whether macOS accepts a row written that way, with the right code requirement blob, is unknown. It
is the first test of the macOS trial. If it works, no click and no MDM.

**What root can do against it, with SIP on.** This is where macOS does better than Linux.

- Root cannot unload a system extension without SIP off (`systemextensionsctl uninstall` needs it;
  not checked today), and a daemon can be protected by the client itself:
  `ES_EVENT_TYPE_AUTH_SIGNAL` lets an ES client refuse a signal sent to it. DTS points to it for
  exactly this question ([forum thread 735413](https://developer.apple.com/forums/thread/735413)),
  and Apple's own `xprotectd` subscribes to it
  ([Objective-See](https://objective-see.org/blog/blog_0x87.html)).
- Root can still unload a launchd daemon with `launchctl bootout`, which goes through launchd
  rather than a signal from the agent (not checked today whether ES sees it). It can delete the
  TCC row, which SIP protects at runtime (not checked today on macOS 26). It can fill the client's
  queue so that events are dropped. Each of these is a test for the hostile trial.
- Root cannot boot the computer into Recovery to turn SIP off: Recovery is chosen by the host when
  it starts the computer (`VZMacOSVirtualMachineStartOptions.startUpFromMacOSRecovery`).

**The route without Apple's approval.** DTS: "for a daemon that calls EndpointSecurity directly,
disabling SIP is sufficient" for testing; a system extension may also need AMFI off
([forum thread 125048](https://developer.apple.com/forums/thread/125048)). Silo can turn SIP off in
a computer by starting it in Recovery and running `csrutil disable` (Lume does this,
[cua.ai](https://cua.ai/docs/how-to-guides/lume/change-sip)). Three costs:

- With SIP off, root inside can do anything to the client, and the reports are worth what a Linux
  probe's are.
- SIP off in the computer lets root change parts of macOS that are otherwise sealed, which is what
  makes an agent hard to clean up after.
- It breaks. On macOS 26.5 guests, `csrutil disable` in Recovery rejected the correct password in
  the Virtualization-based tools reported (Anka, UTM), until a later release candidate (FB22649912,
  [forum thread 824773](https://developer.apple.com/forums/thread/824773), April to May 2026).

**`eslogger`.** Apple ships it signed with the entitlement. It needs root and Full Disk Access for
the process that starts it ([Cybereason](https://cybereason.com/blog/blue-teaming-on-macos-with-eslogger)).
The same TCC row written before first boot would grant it. It is a command, so a probe reads its
JSON output. Root can kill it, unlike a client that refuses signals. It is the fallback if Apple
says no, and it keeps SIP on.

**With no permission.** Sampling the process table misses short processes. `kqueue` with
`EVFILT_PROC` gives `NOTE_FORK` and `NOTE_EXEC` on a watched process, but not the child's number, and
`NOTE_TRACK`, which followed children, has been unsupported since 10.5
([darwin-dev, October 2009](https://lists.apple.com/archives/darwin-dev/2009/Oct/msg00060.html)).
DTrace can see every exec but needs SIP's DTrace protection off (not checked today). None of these
names the writer of a file.

**Overhead.** No independent number for ES exists that I found. North Pole Security says Santa uses
"well under 0.5% CPU and under 150 MB of RAM, even on developer machines doing frequent builds and
executions" ([northpole.security](https://northpole.security/santa)). Santa only authorises execs.
A client that also takes every file write will cost more, and `record-speed-cost` has to measure it
in a macOS computer.

### 3.4 Making a report checkable, so that tampering shows

The plan matches reports to facts by exact keys (`matching-reports-to-facts`). Each check below
gives a mismatch or a gap when an agent with root interferes with the probe.

| What the agent does | What shows it | Kind of line |
|---|---|---|
| Kills the probe | The probe falls silent while the computer runs; the engine knows the computer is not paused | A silence, as `losses-show` says |
| Lets the probe run but hides its own writes from it | File facts with no report from any run; with a busy probe, that is a mismatch for the run that was active | Unattributed file facts |
| Reports writes it did not make | A file report with no file fact | Stays unverified, as the plan says |
| Hides its process | A connection fact at the boundary with no report of a socket | Unattributed connection, and a mismatch for the probe |
| Hides a process start (Linux) | The kernel event of 3.1 has it, the probe does not | Mismatch |
| Replays an old report | Reports carry the probe's own count, and the probe writes a mark at each checkpoint | A count that goes back is a mismatch |
| Fills the probe's queue | The probe counts its own losses | A gap with the count |
| Stops `fseventsd` on macOS (`no_log`) | The `no_log` file is a file fact; changes without events are a mismatch | Mismatch |

Two checks add a lot for little.

- **Byte counts.** The boundary counts the bytes of each connection. The probe on Linux can read
  them per socket too (`tcp_info`, not checked today on libkrunfw). A process that reports 2 KB
  sent while the boundary saw 40 MB on its connection is a mismatch, and the size of the
  difference says something.
- **Disk totals.** The group of a run counts the bytes it wrote (`io.stat` of cgroup v2). The
  changed blocks between two checkpoints are a fact. A run that reports 48 MB written while the
  disk changed by 2 GB with no other run active is the plan's own example of a mismatch
  (`report-matched`), and the block bitmap of 2.2 is what measures the 2 GB.

## 4. Connections

Every connection crosses the boundary. That part is settled and needs nothing new. Which process
opened a connection is a report joined to that fact. The join is the plan's: computer, local port,
remote host and port, between two marks (`matching-reports-to-facts`).

**How others do the join.**

- **Little Snitch and LuLu** run a Network Extension filter in the same macOS that runs the
  program. `NEFilterFlow` gives `sourceAppAuditToken` and, separately, the audit token of the
  process that created the flow
  ([Apple](https://developer.apple.com/documentation/networkextension/nefilterflow)). A filter needs
  its own entitlement and the user's approval ("System Settings → Network → VPN & Filters" in
  Leash's macOS guide). Inside a computer, that is one more permission to grant in the image.
- **Harden-Runner** uses eBPF on GitHub's runners and reports "Domain names resolved, IP addresses
  called, and processes that made these calls"
  ([StepSecurity](https://docs.stepsecurity.io/github-actions/harden-runner)). It reads HTTPS by
  watching the SSL write calls, not with a proxy.
- **Tetragon** hooks `tcp_connect` and reports the process with the socket, in the same kernel
  (above).
- **Leash** hooks `socket_connect` with eBPF LSM and checks a group number
  (`bpf_get_current_cgroup_id`), from a container next to the agent's
  (`docs/design/ARCHITECTURE.md` in [strongdm/leash](https://github.com/strongdm/leash), April
  2026). The agent has no root on that kernel. On macOS, Leash installs an ES extension and a
  Network Extension filter, each approved by hand (`docs/MACOS.md`).

**Where the join breaks.** These are real cases in a computer where agents work, and the plan
names none of them.

- **Docker inside the computer.** `containers-on-macos` and Linux computers run Docker. A container
  connects from `172.17.0.2:51000`, Docker's NAT rewrites it to the computer's address and another
  port, and the boundary sees only the second. The probe sees the first. Joining needs the
  computer's connection tracking table, which is one more report. Without it, every connection from
  a container is unattributed, and a mismatch would be wrong.
- **Name lookups.** On macOS every lookup goes through `mDNSResponder`. On Ubuntu most go through
  `systemd-resolved`. The boundary sees the lookup coming from that daemon. The program that asked
  is only known from inside. This matters for the DNS work of agent E.
- **Work done by a system daemon.** On macOS, `nsurlsessiond` downloads for apps, `trustd` checks
  certificates. Apple's own audit tokens name the daemon, not the app that asked, "with both
  `NEFilterFlow.sourceAppAuditToken` and `sourceProcessAuditToken`"
  ([forum thread 738848](https://developer.apple.com/forums/thread/738848)).
- **Connections shared by processes.** A local proxy or a language server keeps one connection open
  and carries the work of many processes. The boundary sees one connection; its owner is the proxy.
  For model calls this does not matter: the boundary reads the call itself, and the key, whatever
  process sent it. That is why the $453 runner of the competitor survey is caught at the boundary
  even with no process report at all.

## 5. Products that record a whole machine

| Product | What it records | Where it sits | Against root inside |
|---|---|---|---|
| [Harden-Runner](https://github.com/step-security/harden-runner) (StepSecurity) | Outbound calls with process, file writes ("files were overwritten" alert), process starts, HTTPS content from SSL writes | Agent inside the runner, eBPF | The job has `sudo`; the fix was `disable-sudo`, bypassed through the docker group ([CVE-2025-32955](https://nvd.nist.gov/vuln/detail/CVE-2025-32955)); now `disable-sudo-and-containers` |
| [Leash](https://github.com/strongdm/leash) (StrongDM) | Every `open`, `exec`, `connect` of the agent, with a Cedar decision | eBPF LSM in the host kernel, beside the agent's container; on macOS, ES and NE extensions | The agent has no root on the host kernel on Linux. Its own docs: a kernel exploit gets around it |
| [Teleport enhanced recording](https://goteleport.com/docs/enroll-resources/server-access/guides/bpf-session-recording/) | `session.command`, `session.disk` (files opened), `session.network`, per SSH session, by group number | eBPF on the server | "Users who have access to root can disable" it |
| Sysdig / Falco | Every system call, to a capture file (`.scap`) or to rules | Kernel module or eBPF on the node | The node is not the container's to change (not checked today for Sysdig's current product) |
| [osquery](https://osquery.readthedocs.io/en/stable/deployment/process-auditing/) | Tables of processes, sockets, files; on macOS `es_process_events` through ES since 5.0.1 ([Trail of Bits](https://blog.trailofbits.com/2021/11/10/announcing-osquery-5-now-with-endpointsecurity-on-macos/)) | Daemon inside the machine | Root stops it |
| Santa (North Pole Security) | Execs, with allow or deny | ES client inside macOS | ES protections with SIP on |
| Apple's Unified Log | What each system component chooses to log; not every exec by default (not checked today) | Inside macOS, on its disk | Root can change logging; files readable offline from a disk image with Mandiant's `macos-UnifiedLogs` (not checked today) |
| [testvm and VM Peek](https://flaviocopes.com/vm-monitor.md) (Flavio Copes, 1.5.0 of 20 November 2026) | Each command an agent runs through `testvm` in a Tart macOS virtual machine, with start and end, screenshots; "Each line says which agent ran it" | Script and log on the host; finds Claude Code by walking up the process tree | Records only what goes through `testvm`; a command run another way is not seen |

What this says for Silo. Every product that records processes either sits in a kernel the agent
does not control, or says that root defeats it. None reads file changes from the disk outside, and
none checks what it was told against what crossed a boundary. testvm shows the demand: a developer
built a host-side view of what agents do in a test macOS virtual machine four days ago, and it can
only see what agents chose to run through it.

## 6. Each thing the record holds, on each system

"Root" means what an agent with `sudo` inside the computer can do against the way named, without a
kernel exploit.

| Thing | macOS computer | Linux computer |
|---|---|---|
| **Files changed between checkpoints** | Fact. APFS walk of two clones; full walk first, pruned by transaction number later. Maturity: reader exists (libfsapfs, experimental) or a Linux helper computer; the pruned walk is not written. Cost: about 2 s pause per checkpoint (trial: 1.6 to 1.9 s), comparing time unmeasured. Root: can encrypt the volume (gap) or craft metadata (reader must hold) | Fact. Block bitmap in libkrun plus ext4 reader. Maturity: patch to write, `ext4-view` or `debugfs`. Cost: bits per write, then reading changed blocks. Root: crafted metadata; raw writes are caught by the bitmap |
| **Renames** | Fact, if object identifiers are never reused (to test) | Fact, by inode number and generation |
| **Bytes changed with times put back** | Fact, through the transaction number of the changed extents | Fact, through the block bitmap |
| **Files created and deleted between checkpoints** | Report, from `.fseventsd` read off the clone. Mature format, several parsers. Cost: reading a few compressed pages. Root: `no_log`, deleting pages | Report, from fanotify in the probe. Root: kills the probe (silence) |
| **Which process wrote a file** | Report. ES with the entitlement; else `eslogger`; else none. Root with SIP on: little; with SIP off: everything | Report. fanotify in the probe. Root: kills or blinds the probe |
| **Process starts and ends** | Report. ES or `eslogger`; else sampling, which misses short ones. Root: as above | Fact candidate. Kernel events through breakpoints or a hypervisor call from Silo's kernel. Not built. Cost: one exit per event. Root: nothing short of a kernel exploit. Report: process connector in the probe, today's kernel |
| **Processor and memory per run** | Report, sampled by the probe | Report, from the run's group (cgroup v2) |
| **Connections** | Fact at the boundary | Fact at the boundary |
| **Which process opened a connection** | Report. A Network Extension filter needs another entitlement and approval; without it, sampling sockets misses short connections | Report. eBPF on connect, with Silo's own kernel; or sampling `/proc/net` |
| **Model calls and their key** | Fact at the boundary | Fact at the boundary |
| **Data not yet written to disk** | Not seen; probe `sync` before a checkpoint helps | Same |

## 7. Risks the plan underrates

1. **Writes past the filesystem.** The plan's metadata comparison trusts the times the filesystem
   keeps. Root can change any block with `dd`, and the times stay put. Example: an agent replaces
   the bytes of `/usr/local/bin/node` in place. The comparison sees no change. On Linux, the block
   bitmap of 2.2 catches it; on macOS, the APFS transaction number catches writes through APFS but
   not raw writes to the disk device (not checked today whether macOS lets root write the raw device
   of its mounted boot disk with SIP on). `file-changes-from-outside` asks this only in its soft
   form ("a file written and put back with its change time reset").
2. **macOS writes a lot on its own.** Spotlight, the Unified Log, caches and `fseventsd` itself write
   all the time. A night's comparison may list thousands of paths under `/private/var/db` and
   `~/Library/Caches` that no agent touched (not measured). If those lines are not grouped, the
   overnight view is noise. Grouping them is an opinion and must stay apart from the facts. Turning
   Spotlight off in the image (`mdutil -a -i off`) removes one writer (not checked today).
3. **The stock kernel has no eBPF tracing.** `processes-from-inside-linux` reads as if eBPF were
   available. With libkrunfw as shipped, it is not (3.2). A Silo kernel configuration is work, and
   enabling eBPF also gives root inside a larger kernel interface to attack.
4. **Apple's permissions are discretionary and fragile.** The ES entitlement may be refused, has no
   timeline, and is tied to Silo's team. Full Disk Access for an ES client failed on macOS 26 even
   with the host app granted ([forum thread 844239](https://developer.apple.com/forums/thread/844239)).
   SIP could not be turned off in 26.5 guests for weeks. A macOS update can break any of these
   routes in main. `macos-ships-facts-first` is the right answer, and the record should show which
   inside sources work in main today.
5. **A third macOS computer is refused.** Apple allows two macOS computers at once, "enforced by
   macOS" ([Eclectic Light, April 2026](https://eclecticlight.co/2026/04/29/virtualisation-on-apple-silicon-macs-is-different/)).
   With main and one fork running, a macOS helper that mounts checkpoints cannot start. A Linux
   helper can.
6. **Encryption inside.** FileVault on in main, or an encrypted disk image made by the agent, makes
   file facts impossible from then on. It must show as a gap from that checkpoint.
7. **Containers inside break the connection join** (section 4). Docker's NAT rewrites the port the
   join depends on.
8. **The checkpoint pause.** Both trials paused the computer to clone the image (1.6 to 3.1 s).
   Whether an APFS clone of an image in use by Apple's framework is consistent without a pause, in
   each caching mode, is in `checkpoint-per-system` as "to find out". Every few minutes, a 2 s
   pause is felt in a terminal or a dev server (arithmetic: 2 s every 5 minutes is 0.7% of the
   time, but each one is a stall a person can see).

## 8. The cheapest first version that answers "what changed overnight"

The case: you start Claude Code in main at 23:00 and look at 08:00.

What it needs, all from outside main, nothing inside:

1. A checkpoint every few minutes: pause main, `clonefile` its image, resume. Measured at 1.6 to
   1.9 s on the Mac.
2. A reader that walks the APFS Data volume of two clones and prints, per path, created, changed,
   deleted, with sizes. The first version walks everything; it compares the 08:00 clone with the
   23:00 one, not each pair, so the night costs one walk of each (about 22 s per 200,000 files at
   the trial's Mac rate). The reader runs in its own process with no network, or in a Linux helper
   computer.
3. The boundary's list of hosts reached and model calls with their keys and cost, which the plan
   already builds.

What it prints, as `silo changes main --since 23:00` might:

```text
main, 23:00 to 08:00, 108 checkpoints, all facts seen from outside

  created  12,431 files  1.9 GB   /Users/agent/work/site/node_modules/...   (grouped)
  changed       48 files  2.1 MB   /Users/agent/work/site/src/...
  deleted        3 files           /Users/agent/work/site/tests/fixtures/
  renamed        1 file            /Users/agent/work/site/README.md → docs/README.md
  changed    3,210 files  610 MB   written by macOS itself, by path (grouped)

  hosts       registry.npmjs.org 1,204 connections, api.anthropic.com 317, github.com 12
  model calls 317, $41.20, key: the one Silo holds for main

  not seen    files created and deleted between two checkpoints; which program wrote each file
```

The last line is what `macos-ships-facts-first` asks: the absence of reports must not read as
nothing happening. The "renamed" line needs the identifier test of 2.3, and the "written by macOS
itself" group is an opinion shown as one.

What it leaves for later, in order of value: the pruned walk, so that each pair of checkpoints can
be compared and the grain is minutes, not the night; `.fseventsd`, for files that came and went;
the probe with ES, for which process wrote what.

## Recommendations for the plan

1. **`file-changes-from-outside`** (open, proposal). Replace "A rename is written as a delete and a
   create" with: a rename is written as a rename when the file's identity is the same, by object
   identifier on APFS and by inode number with generation on ext4. Add to its "to find out": whether
   APFS ever reuses an object identifier. The trial's 4,939 false renames came from comparing inode
   numbers alone.
2. **`file-changes-from-outside`** (open, proposal). Answer two of its questions. "Whether a file
   written and put back with its change time reset can be caught through the blocks written": yes,
   on Linux with a block bitmap in libkrun and a map from blocks to inodes, and on macOS through
   APFS transaction numbers. "Reading only the blocks written since the last checkpoint": on Linux,
   libkrun's block device; on macOS, the APFS object map pruned by transaction number, which needs
   no block tracking. Add raw writes to the disk device as a hostile case.
3. **New feature, `blocks-written-per-checkpoint`** (new, proposal). The engine keeps, for each
   Linux computer, the set of disk blocks written since the last checkpoint, as a fact. About 30
   lines in libkrun's `virtio/block/worker.rs`, carried as `libkrun-patches` says, with an upstream
   pull request. It makes comparing cheaper and catches writes past the filesystem.
4. **`processes-watched-from-outside`** (open, challenge to its `suggest`). Keep the drop for macOS
   computers. For Linux computers, its reason does not hold: Silo ships one kernel through
   libkrunfw and owns the monitor. Proposal: split it, and put forward "Process starts and ends of a
   Linux computer, seen by the engine" through hardware breakpoints or a hypervisor call from
   Silo's kernel. Measure the cost per exec on a Mac before taking it.
5. **New decision, `kernel-events-are-facts`** (new, proposal). Ask whether a process event that the
   engine reads itself from the computer's memory at a breakpoint is a fact. Recommended: yes, from
   the source "engine: kernel events", with the trust in the computer's kernel stated once; the
   hypervisor-call variant, told by Silo's kernel, stays a report as the words define it.
   polarzero's direction of 20 November ("as many facts as possible") points to the breakpoint
   route. The taken `words-for-what-silo-knows` already fits it ("seen from outside").
6. **`processes-from-inside-linux`** (open, proposal). State that libkrunfw as shipped has no
   tracepoints, kprobes, BTF, eBPF JIT or audit, so the eBPF route needs a Silo kernel
   configuration, and its cost is measured with that configuration, not taken from Tetragon's
   numbers. Add the race of the process connector: an exec whose process ended before its command
   was read is written as "command not read", not dropped.
7. **`processes-from-inside-macos`** (open, proposal). Put the ES client as a launchd daemon, not a
   system extension, first, with Full Disk Access written into the TCC database before first boot.
   Apply for the entitlement now, since the answer has no timeline. Put the SIP-off route last, after
   `eslogger`, and say why: it gives root everything, and it broke in macOS 26.5. Add the hostile
   tests: `kill -9` against `ES_EVENT_TYPE_AUTH_SIGNAL`, `launchctl bootout`, deleting the TCC row,
   flooding the queue.
8. **New feature, `fsevents-from-the-disk`** (new, proposal). For a macOS computer, the record reads
   `.fseventsd` from each checkpoint as a report of changes between checkpoints, with no probe. It
   gives back the files created and deleted between two checkpoints, which `file-changes-in-the-record`
   gives up, and it ships with the facts-only first version of `macos-ships-facts-first`.
9. **`matching-reports-to-facts`** (open, proposal). Add two cases. A connection from a container
   inside the computer is joined through the computer's connection-tracking table; without it, the
   report stays unverified, never a mismatch. A name lookup made by `mDNSResponder` or
   `systemd-resolved` is the daemon's connection, and the program that asked is a report from
   inside.
10. **`file-changes-from-outside`** (open, proposal), under `fuzz-untrusted-parsers` (taken, which
    applies as it is). Name the
    reader of a hostile disk: a user-space reader in its own process with no network and a budget,
    or a Linux helper computer that mounts both checkpoints read-only. Say that the helper is Linux,
    because a third macOS computer is refused. A reader that gives up writes a gap, never "nothing
    changed". FileVault turned on inside is a gap from that checkpoint on.
11. **`losses-show`** (open, proposal). Add the case of the encrypted volume and of `no_log` as
    named gaps, so that the overnight view says "file changes not seen since 02:14: main's Data
    volume is encrypted".
12. **`checkpoint-per-system`** (open, proposal). Add a "to find out": how much the probe's `sync`
    before a checkpoint changes what the checkpoint holds, and whether the checkpoint line records
    that the probe confirmed it.
13. **New decision, `macos-churn-grouped`** (new, proposal). Ask how the record shows the paths
    macOS itself writes in main. Recommended: as facts like any other, grouped in the view by an
    opinion that names the folders, never left out. Measure the churn of an idle macOS computer over
    a night first.
14. **Linear, project "Insights"** (not the plan). Apply to Apple for the Endpoint Security
    entitlement under Silo's team now. It is business material, so it belongs there.

## Sources read today

- Silo: `plan/record/*`, `plan/computers/decisions/*`, `plan/engine/decisions/linux-vm-engine*`,
  `plan/going-back/decisions/checkpoint-per-system*`,
  `/mnt/project-files/research/2026-11-24-first-build.md`.
- libkrun at `715c1c9` (23 November 2026): `src/devices/src/virtio/block/worker.rs`,
  `src/hvf/src/lib.rs`, `src/hvf/src/bindings.rs`, `src/devices/Cargo.toml`.
- libkrunfw (12 November 2026): `config-libkrunfw_aarch64`, `Makefile`.
- strongdm/leash (6 April 2026): `docs/MACOS.md`, `docs/design/ARCHITECTURE.md`.
- [Apple File System Reference](https://developer.apple.com/support/apple-file-system/Apple-File-System-Reference.pdf).
- Apple: [`VZNetworkBlockDeviceStorageDeviceAttachment`](https://developer.apple.com/documentation/virtualization/vznetworkblockdevicestoragedeviceattachment),
  [`NEFilterFlow`](https://developer.apple.com/documentation/networkextension/nefilterflow),
  [`ES_EVENT_TYPE_AUTH_SIGNAL`](https://developer.apple.com/documentation/endpointsecurity/es_event_type_auth_signal).
- Apple developer forums: threads [125048](https://developer.apple.com/forums/thread/125048),
  [735413](https://developer.apple.com/forums/thread/735413),
  [738848](https://developer.apple.com/forums/thread/738848),
  [782007](https://developer.apple.com/forums/thread/782007),
  [791996](https://developer.apple.com/forums/thread/791996),
  [820718](https://developer.apple.com/forums/thread/820718),
  [824773](https://developer.apple.com/forums/thread/824773),
  [844239](https://developer.apple.com/forums/thread/844239).
- Eclectic Light: [December 2022](https://eclecticlight.co/2022/12/01/disk-performance-of-lightweight-macos-vms-on-apple-silicon/),
  [27 March 2026](https://eclecticlight.co/2026/03/27/does-disk-storage-speed-limit-macos-virtual-machines/),
  [29 April 2026](https://eclecticlight.co/2026/04/29/virtualisation-on-apple-silicon-macs-is-different/),
  [Time Machine to APFS, 2021](https://eclecticlight.co/2021/03/18/time-machine-to-apfs-how-processes-have-changed/).
- [QEMU dirty bitmaps](https://qemu.googlesource.com/qemu/+/refs/tags/v8.0.5/docs/interop/bitmaps.rst),
  [NBD URI spec](https://repo.or.cz/nbd/ericb.git/blob/HEAD:/doc/uri.md),
  [ext4-view](https://docs.rs/crate/ext4-view/latest), [libfsapfs](https://github.com/libyal/libfsapfs),
  [linux-ext4 thread on `i_generation`](https://lore.kernel.org/lkml/20170629045940.GB5865@birch.djwong.org/).
- Introspection: [LibVMI](https://github.com/libvmi/libvmi), [KVM-VMI](https://github.com/KVM-VMI/kvm-vmi),
  [DRAKVUF](https://tklengyel.github.io/drakvuf),
  [QEMU HVF gdbstub patches](https://lore.kernel.org/all/20221116174749.65175-1-fcagnin@quarkslab.com/),
  [Code-Hex/vz wiki](https://github.com/Code-Hex/vz/wiki/How-to-debug-guest-with-virtualization-framework),
  [btf2json guide](https://www.iblue.team/memory-forensics-1/volatility-plugins/generate-custom-profile-using-btf2json).
- Tools and products: [Isovalent, Tetragon 1.0](https://isovalent.com/blog/post/tetragon-release-10),
  [falcosecurity/libs #1557](https://www.github.com/falcosecurity/libs/issues/1557),
  [Red Hat KB on audit cost](https://access.redhat.com/node/3201282),
  [Teleport](https://goteleport.com/docs/enroll-resources/server-access/guides/bpf-session-recording/),
  [StepSecurity docs](https://docs.stepsecurity.io/github-actions/harden-runner),
  [CVE-2025-32955](https://nvd.nist.gov/vuln/detail/CVE-2025-32955),
  [osquery process auditing](https://osquery.readthedocs.io/en/stable/deployment/process-auditing/),
  [Trail of Bits, osquery 5](https://blog.trailofbits.com/2021/11/10/announcing-osquery-5-now-with-endpointsecurity-on-macos/),
  [Santa](https://northpole.security/santa), [VM Peek and testvm](https://flaviocopes.com/vm-monitor.md),
  [CrowdStrike on FSEvents](https://www.crowdstrike.com/en-us/blog/using-os-x-fsevents-discover-deleted-malicious-artifact/),
  [fse_dump](https://github.com/kornelski/fse_dump),
  [darwin-dev on `NOTE_TRACK`](https://lists.apple.com/archives/darwin-dev/2009/Oct/msg00060.html),
  [Lume, changing SIP](https://cua.ai/docs/how-to-guides/lume/change-sip),
  [Huntress on Full Disk Access](https://www.huntress.com/blog/ask-the-mac-guy-whats-the-deal-with-full-disk-access),
  [Cybereason on eslogger](https://cybereason.com/blog/blue-teaming-on-macos-with-eslogger).
