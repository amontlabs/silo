# Reading a macOS computer's disk from outside

An agent's research, 24 November 2026, for polarzero. It changes nothing in `plan/`. It covers a
risk the plan does not list yet. To give `silo changes main --since <checkpoint>` its lines, the
engine must find which files changed between two checkpoints of main's disk. Main is a macOS
computer and its disk is APFS. An agent with `sudo` wrote every byte of that disk, so the program
that reads it is reading hostile input.

## What to decide, in short

| Question | Recommendation | Confidence |
|---|---|---|
| Mount a checkpoint on your Mac with `mount_apfs -o rdonly,nobrowse`? | No. The Mac's kernel would parse a disk the agent wrote. Apple fixed APFS kernel bugs reached by "mounting a maliciously crafted disk image" again this year. | high |
| Mount it in a second macOS computer, as the 21 November trial did? | No. A Mac runs at most two macOS virtual machines at once, so a macOS reader takes the slot a fork of main needs. | high |
| Use libfsapfs, apfs-fuse, a Rust crate or The Sleuth Kit as the reader? | No, not as they are. In my tests here, libfsapfs could not open a volume with 512 snapshots, and libfsapfs and apfs-fuse both failed on a disk whose newest checkpoint was half written. A clone of a running computer's disk is that kind of disk. The Rust crates are young or unfinished. | high on the facts, medium on the conclusion |
| Is there a transaction id (xid) on each inode or extent that makes a diff cheap? | No. Inode and extent records carry no xid. Every B-tree node carries one, and nodes are never changed in place. So the diff can compare the addresses of the leaf nodes of the two trees. It reads only the index nodes and the leaves that changed, and gets the same result as a full walk. | high: from Apple's reference, and tested on 511 pairs of snapshots of a disk Apple's own APFS wrote |
| What to build | A reader of Silo's own. It reads metadata only, is strict, and refuses anything ambiguous. It diffs by leaf address and falls back to the last good checkpoint. It runs as a separate process with no rights, later inside a small Linux computer. A disk it cannot read becomes a gap, never "nothing changed". | medium. The approach is tested; the speed on a disk with a few million files is not |

The three findings that matter most:

1. **The cheap diff exists, and it does not rest on trusting xids.** APFS never writes an object
   back where it was. A leaf of the file-system tree found at the same block in both checkpoints
   therefore holds the same records. Comparing the two trees means reading their index nodes and
   only the leaves whose addresses differ. On a macOS disk with a few million files that is about
   30 MB instead of about 1.5 GB per comparison (estimated, not measured). Apple's own Time Machine
   stopped diffing snapshots because the diff was slow
   ([Eclectic Light](https://eclecticlight.co/2021/03/11/time-machine-to-apfs-understanding-backups/));
   how Apple's diff worked is not public.
2. **A clone of a running computer's disk can have a broken newest checkpoint, and the readers
   off the shelf then give up.** dissect.apfs ships an image where the newest checkpoint (xid 304)
   points at a block with a bad checksum. libfsapfs and apfs-fuse refuse the whole disk. My
   prototype, like macOS, opens the newest checkpoint that reads cleanly (302). Silo needs that
   fallback every time, and it needs the record to say which checkpoint of the disk it read.
3. **APFS does not reuse inode numbers, so a rename can be seen on a macOS computer.**
   `file-changes-from-outside` writes every rename as a delete and a create, because ext4 gives a
   freed inode number to the next file (4,939 of 10,000 deletes looked like renames in the Linux
   trial). In APFS a new file takes the volume's next object id. Across the 512 snapshots of the
   test disk, 1,022 new inodes appeared, each id larger than the one before, and none came back.
   This challenges a sentence of a decision in force; see the end of this page.

## The case

Main runs Claude Code with every check off. At 14:00 Silo clones main's disk image on your Mac
(`checkpoint-per-system`). At 14:05 it clones it again. Between them the agent ran
`npm install` (40,000 new files), edited 12 source files, ran `sudo rm -rf /usr/local/lib/foo`
and created a local Time Machine snapshot with `tmutil localsnapshot`. You run
`silo changes main --since 14:00`. The engine must name those files, from the two image files,
without starting anything inside main and without trusting anything main wrote.

A macOS computer's disk has the same layout as the internal disk of an Apple silicon Mac
([Eclectic Light](https://eclecticlight.co/2024/10/22/boot-volume-layout-and-structure-in-macos-sequoia/),
not checked on a Silo disk). It is a GPT disk with three APFS containers: the iBoot system
container, the main container and the recovery container. The main container holds the System
volume (sealed, read only), the Data volume (role `APFS_VOL_ROLE_DATA`, where almost every change
lands), Preboot, Recovery and VM. The agent can add volumes to it (`diskutil apfs addVolume`), so
the reader must list every volume and report a volume that appears or goes.

## Why not mount it

**On your Mac.** `mount_apfs -o rdonly,nobrowse` keeps Finder and Spotlight away, but the Mac's
kernel still parses every superblock, object map and B-tree node of the disk. APFS runs in the
kernel; macOS 26 moved exFAT and FAT to user space with FSKit, not APFS
([Eclectic Light](https://eclecticlight.co/2025/11/18/which-local-file-systems-does-macos-26-support/)).
Apple's notes for macOS Tahoe 26.7.2 list CVE-2026-84496 under APFS: "Mounting a maliciously
crafted disk image may cause unexpected system termination or corrupt kernel memory", an
out-of-bounds write ([Apple](https://support.apple.com/en-us/149522), read). Sequoia 15.7.8 lists
another APFS kernel memory bug, CVE-2026-64695 ([Apple](https://support.apple.com/en-us/128071),
read). An agent with `sudo` can write such an image on purpose, and the bugs that are not
published yet are the ones that count. A read-only mount does not narrow this surface: the
parsing happens before anything is written.

**In a second macOS computer.** The trial of 21 November did this. The blast radius is then that
computer, which is right. But the Virtualization framework starts at most two macOS virtual
machines on a Mac; the third fails with "the maximum supported number of active virtual
machines has been reached" ([Apple forums, a DTS engineer](https://developer.apple.com/forums/thread/729580);
the license's section 2B(iii)). Main plus one fork already fill both slots, so a macOS reader
blocks `forking-a-computer` in the second build. It also boots macOS for each comparison.

**In a small Linux computer with a kernel driver.** linux-apfs-rw
([github.com/eafer/linux-apfs-rw](https://github.com/eafer/linux-apfs-rw)) mounts APFS on Linux.
Its blast radius is that computer. It still walks every file to compare, which is the slow part,
and a kernel module from outside Linux is a lot of hostile-input code to carry. A reader in user
space inside the same small computer is better on both counts.

## The readers that exist

All of these were built or run here unless marked. The test disks are the ten images in
dissect.apfs's tests, made by Apple's APFS (their files are dated October 2025, so most likely by
macOS 26).

| Reader | State | What I found |
|---|---|---|
| [libfsapfs](https://github.com/libyal/libfsapfs) (C, libyal, one maintainer) | "Status: experimental" in its README. Last commit 20 November 2026. Fuzzed in OSS-Fuzz as part of the `libyal` project, with ASan, MSan and UBSan, under libFuzzer, AFL and honggfuzz (read in `google/oss-fuzz/projects/libyal`). Its five fuzz targets open a container, volume, file entry, extended attribute and snapshot. | **Could not open a volume with 512 snapshots**: "unable to determine sub node block number". The cause: it looks up the children of the snapshot metadata tree in the object map, but Apple's reference says that tree is physical, so its children are block numbers. A six-line patch fixed it here and all 512 snapshots listed. **Refused the disk whose newest checkpoint is broken.** README lists snapshots and LZBitmap compression as unsupported. CVE-2026-111133, a heap out-of-bounds read in its key parsing, affects versions through 20260921 ([VulnCheck](https://www.vulncheck.com/advisories/libfsapfs-through-20260921-heap-out-of-bounds-read-via-libfsapfs-key-encrypted-key-read-data), not read in full). |
| [apfs-fuse](https://github.com/sgan81/apfs-fuse) (C++) | Last commit March 2023. Snapshots (`snap=xid`), sealed volumes, FileVault, Fusion. No fuzzing that I found. | Read the volume with 512 snapshots. **Refused the broken-checkpoint disk**: "Invalid omap block @ oid 0x6a. Unable to load container." |
| [dissect.apfs](https://github.com/fox-it/dissect.apfs) (Python, Fox-IT) | Version 1.1, last commit June 2026. Snapshots, encryption, fallback to an older checkpoint. | Opened every test disk, including the broken one at xid 302. Too slow and in the wrong language for the engine, but a good second reader to test Silo's reader against. |
| [apfsprogs](https://github.com/linux-apfs/apfsprogs) `apfsck`, `mkapfs` (C) | Last commit 12 November 2026. A strict checker, written to test the Linux driver. | Built here. `mkapfs` makes empty containers for tests. `apfsck` is the closest thing to a list of everything a well-formed disk must satisfy. |
| Rust: [`apfs`](https://docs.rs/crate/apfs/latest), [`apfs-core`](https://docs.rs/crate/apfs-core/latest) | `apfs` 0.4: no snapshots, no encryption. `apfs-core` 0.3: its own README calls it a design skeleton with parser bodies still stubs. | Read on docs.rs, not run. |
| The Sleuth Kit | APFS since 4.8.0 through BlackBag's pool layer; encryption "not complete" in its changelog. In OSS-Fuzz as `sleuthkit` (read). A search result mentions an out-of-bounds read in its APFS keybag parser (not verified). | Not run. |
| Apple | No public library. The [Apple File System Reference](https://developer.apple.com/support/downloads/Apple-File-System-Reference.pdf) is dated 2020-06-22 and has not changed since. | Downloaded and read. It was enough for my prototype to read disks written in 2025. |

The forensic readers share a design goal that is the opposite of Silo's: they try to read as
much as they can of a damaged disk. Silo needs a reader that refuses a disk it cannot read
exactly as macOS would, and says so. A lenient reader is how a file gets hidden. If the agent
writes a B-tree whose keys are out of order, macOS's binary search finds one record and a walk
finds another.

## Is there an xid per inode or per extent?

No. Checked in the reference:

- `j_inode_val_t` has `parent_id`, `private_id`, four times, `internal_flags`, `nchildren` or
  `nlink`, `default_protection_class`, `write_generation_counter`, `bsd_flags`, owner, group,
  mode, `uncompressed_size` and extended fields. No xid.
- `j_file_extent_val_t` has `len_and_flags`, `phys_block_num` and `crypto_id`. No xid.
- `write_generation_counter` is "incremented each time this inode or its data is modified". On
  the test disk it did not move for a file whose times changed (inode 17: modified time changed,
  counter stayed at 2). It cannot be relied on.

But every object has one. Each object starts with `obj_phys_t`, whose `o_xid` is "the identifier
of the most recent transaction that this object was modified in". The object map is keyed by
(object id, xid). And the reference says: "Regardless of their storage, objects on disk are never
modified in place, and modified copies of an object are always written to a new location on
disk." So:

- A node at the same block number in two checkpoints of the same disk holds the same bytes, as
  long as nothing wrote past APFS.
- Each record of the file-system tree sits in exactly one leaf. Take the leaves of both trees.
  Drop those found at the same block in both. The records of what is left, compared, give exactly
  the diff of the whole trees.
- Walking "only objects with xid greater than the checkpoint's" from the root does not work by
  itself. The file-system tree is virtual: an index node names its children by object id, and a
  child that changes gets a new block in the object map, not a new id. The leaf-address diff
  avoids that question: it reads the index nodes, which are about 1% of the tree, and resolves
  each child through the object map, which is small.

**Tested.** I wrote a reader of about 250 lines of Python from the reference alone. It checks the
Fletcher-64 checksum of every block, finds the newest valid checkpoint, and walks the object map,
the snapshot metadata tree and the file-system tree. On the dissect.apfs disk with 512 snapshots
(1,027 files):

| Pairs of snapshots | Same diff as a full walk | Blocks read, leaf-address diff | Blocks read, full walk |
|---|---|---|---|
| 511 neighbours (n, n+1) | 511 of 511 | 6,260 | 45,014 |
| Snapshot 510 to 511 | yes | 12 | 174 |
| 511 neighbours plus 200 random pairs | 711 of 711 | 16,154 | 62,222 |

One file created between two snapshots changed 5 to 7 leaves of 50 to 87: the directory record,
the new inode, the parent's inode and the `.fseventsd` log. The tree here is small, so the saving
is 7 times. It grows with the tree, because the full walk reads every leaf and the leaf-address
diff reads only the changed ones.

**What it would cost on main (estimated, not measured).** Records on the test disks average 69
bytes and there are about 4 per file. That puts about 12 files in a 4 KiB leaf; take 8 to 10 for
real names and extended attributes. For 3 million files:

| | Blocks | Bytes |
|---|---|---|
| Leaves of the file-system tree | about 300,000 to 375,000 | 1.2 to 1.5 GB |
| Index nodes of the file-system tree | about 3,000 | about 12 MB |
| Leaves of the object map (36 bytes per entry) | about 3,400 | about 14 MB |
| Leaf-address diff, both checkpoints | about 13,000 plus 2 per changed leaf | about 50 MB, or about 26 MB when the earlier checkpoint's list of leaves is kept |
| Full walk of both checkpoints | about 700,000 | about 2.8 GB |

An `npm install` of 40,000 files mostly touches neighbouring leaves: new inodes take increasing
ids, so they sit together at the end of the tree. A change to one file in each leaf, such as
`chmod -R` or a tool that touches every file, changes every leaf. Then the diff costs a full walk,
and that walk is the honest answer. Access times rarely cause it: by default APFS updates an
access time only when it is older than the modified time
([Eclectic Light](https://eclecticlight.co/2025/10/24/be-careful-when-interpreting-apfs-timestamps/),
not tested).

### Where the trust comes from

The same block at the same address holds the same bytes only if every write went through APFS.
Root inside main could write to the raw disk and change a leaf in place, keeping its address. On
a macOS computer, System Integrity Protection probably blocks raw writes to the mounted boot disk,
and only recoveryOS can turn it off, which Silo alone can start a virtual machine into. Neither
point is verified. The way that does not depend on them is on your Mac's side: both checkpoints
are APFS clones of one image file, so a block of the image that was not written since the earlier
checkpoint is still shared by the two clones. If the Mac can say which blocks of the two clones
differ (`fcntl(F_LOG2PHYS_EXT)` gives the device offset of a range of a file; untested on clones),
the engine knows from outside which blocks were written. Then:

- "Same address and not written" proves that a leaf is unchanged. No xid needs to be trusted.
- A written block that APFS does not account for (not a changed node, not a new extent) is a
  write that went past APFS. The record says so.
- APFS also overwrites a file's data in place when nothing else shares it: the reference says the
  extent reference tree is "consulted when deciding whether to overwrite an extent". Such a write
  still changes the file's inode, through its times and size, so it shows in the diff. A data
  write past APFS with the times put back shows only through the written blocks. Naming the file
  then needs an index from block to file, which the engine can keep up to date from the extent
  records each diff already reads.

This answers one of the "to find out" items of `file-changes-from-outside`: "whether a file
written and put back with its change time reset can be caught through the blocks written". On
APFS it can, if the Mac test of `F_LOG2PHYS_EXT` holds.

## What the reader has to handle

| Case | What happens on main | What the reader does |
|---|---|---|
| Clone taken during a write | The newest checkpoint may point at blocks not written yet | Opens the newest checkpoint whose objects all check, as macOS does. The record says which xid it read and that a newer one was skipped. Measured on the dissect.apfs disk: 304 skipped, 302 read. |
| Many local snapshots | `tmutil localsnapshot` run 100 times | Reads the snapshot metadata tree as a physical tree. libfsapfs fails here today. |
| Snapshot deleted or restored inside main | `tmutil deletelocalsnapshots`, or a snapshot revert (`apfs_revert_to_xid`) | Diffs the live tree. A revert shows as the files it changed. |
| Clones of files (`cp -c`) | A new inode sharing extents | Shows as a created file. Nothing special. |
| Compressed files | decmpfs in an extended attribute or the resource fork | Not needed: the reader reads metadata, not contents. Reading contents (for showing what changed in a file) is a later step, and there libfsapfs's decompressors can be reused inside the small computer. |
| Sealed System volume | Changes only through a macOS update, as a new sealed snapshot | Compares the booted snapshot and the seal's root hash. A change is reported, then the volume is diffed like any other. |
| A new volume or partition | `diskutil apfs addVolume`, a resized container | Reported as such, then read. |
| FileVault turned on inside | The Data volume's file-system tree is encrypted; the object map is not | A gap: "files of volume Data not read: encrypted". The object map still tells how many nodes changed, and the gap says so. Not "nothing changed". In a macOS 15 computer, FileVault reportedly needs an Apple ID set up when the computer was created ([Eclectic Light](https://eclecticlight.co/2024/07/18/securing-virtual-machines-on-apple-silicon/), not verified for macOS 26). |
| A disk that breaks a rule | Keys out of order, a cycle, a node at the wrong level, an object whose `o_xid` is newer than its checkpoint, a block used twice | No diff for this pair. A gap that names the rule. The next diff compares with the last checkpoint that read cleanly. |
| A file created and deleted between checkpoints | Gone before 14:05 | Not seen, as `file-changes-from-outside` says. The `.fseventsd` logs on the Data volume list it, and the reader can parse them as reports: macOS wrote them, root can delete or edit them. |
| Renames | `git mv`, an editor's atomic save | Same object id, new directory record: a rename. An atomic save gives a new inode under the old name, which is a change by the user's meaning and a delete and a create by the disk's. |

## The design to build

1. **A reader of Silo's own, metadata only.** It reads GPT, the container superblocks and the
   checkpoint area, the object maps, the volume superblocks, the snapshot metadata tree and the
   records of the file-system tree: inodes, directory records, extended attribute names, extents,
   sibling links. It does not decompress, decrypt or read file contents. Written in Zig, which the
   engine already uses, in ReleaseSafe so every bounds check stays. My prototype was about 250
   lines; a strict one is likely 2,000 to 3,000 (a guess).
2. **Strict.** It checks every rule `apfsck` checks that the diff depends on. A block budget, a
   depth limit and a cycle check bound its work. A disk that breaks a rule is a gap, with the rule
   named. It never guesses what macOS would read.
3. **Diff by leaf address,** as above. For each checkpoint the engine keeps the list of leaves
   (about 3 MB for 3 million files), so the next diff reads only the new checkpoint's index nodes,
   its object map and the changed leaves.
4. **Trust from your Mac's side** where the Mac can give it: the blocks of the image written
   between two checkpoints, from the clones. Without it, the record says that writes past APFS are
   not seen.
5. **Where it runs.** First build: a separate process that gets only the two image files, opened
   read only, and a pipe for its output, under a sandbox profile that denies files, network and
   every other process (the same Seatbelt mechanism browsers use for renderers). Its output is a
   strict, small format the engine parses on its own. Second build, once libkrun is in
   (`linux-vm-engine`): the same binary as the only program of a small Linux computer with the two
   images attached read only, no network and one vsock port. Then a bug in the reader stays in
   that computer, which is the boundary Silo already relies on for main.
6. **Contribute the fixes upstream** where it costs little. The libfsapfs snapshot fix is six
   lines and ready.

Alternatives weighed and not recommended:

- **libfsapfs, patched, as the reader.** Less code to write. But it has no leaf-level API, so the
  diff would still walk every file; it is lenient where Silo must be strict; and its checkpoint
  handling has to be rewritten. Worth reusing for reading file contents later.
- **A program inside main that reports changes.** Through FSEvents it would see every file live,
  including those created and deleted between checkpoints. An agent with `sudo` can stop it or
  make it lie, so its lines are reports, and `macos-ships-facts-first` already holds programs
  inside macOS back. Reading `.fseventsd` from the disk gives most of the same reports without
  running anything inside.

## Test plan

Here, on Linux, in CI:

1. **The same as a second reader.** On every dissect.apfs disk, Silo's reader lists the same
   files with the same metadata as dissect.apfs. (Their licence and how to fetch the images go in
   the test's notes. The images are stored with Git LFS.)
2. **Leaf-address diff equals full walk** on all 511 neighbouring pairs and 200 random pairs of
   snapshots, as measured above, and on every mutated disk that passes the rules (point 4).
3. **Fallback.** The broken-checkpoint disk opens at 302, and the output says 304 was skipped.
4. **Faults put in on purpose** (`check-must-catch-a-fault`). From each test disk, write a disk
   that breaks one rule: keys out of order, a duplicate key, a child at the wrong level, a cycle,
   an object map entry newer than its checkpoint, a leaf listed twice, a volume added. For each,
   the reader must give a gap naming the rule, never a diff and never a crash. One mutation must
   hide a file from a lenient walk; the test passes only when the reader reports it.
5. **Fuzzing** of each structure parser and of the whole pipeline, with a block budget so a hang
   is a failure (`fuzz-untrusted-parsers`), seeded with the test disks.

On polarzero's Mac, the measurements no trial has made:

6. **Speed.** A macOS 26 computer with about 3 million files on its Data volume (a few large
   repositories with their `node_modules`). Checkpoints by `cp -c`. Change 10, 1,000, 50,000 files
   and all files. Time the leaf-address diff and the full walk, and count the blocks each read.
   Expected: the diff follows the changes, the walk follows the tree.
7. **Clones during writes.** 100 clones of the image while `fio` writes inside. Count how often
   the newest checkpoint is broken. The reader must open every one.
8. **Blocks written, from the Mac.** `F_LOG2PHYS_EXT` on two clones of a 100 GB image: does it
   say which blocks differ, matching a byte comparison, and how long does it take?
9. **Raw writes from inside.** Can root in main write to the raw device of the boot container
   with System Integrity Protection on? If so, write a file's block past APFS and check that the
   written blocks catch it.
10. **FileVault inside.** Can it be turned on in a Silo computer at all? If so, the reader gives
    the gap, and the next readable checkpoint compares against the last good one.
11. **Many snapshots and a new volume.** `tmutil localsnapshot` 100 times, and
    `diskutil apfs addVolume` with a file in it. Both show in the output.

## Challenges to decisions in force

As `AGENTS.md` asks. Each stays in force until polarzero changes it.

- **`file-changes-from-outside`: "A rename is written as a delete and a create."** The reason
  given is ext4's reuse of inode numbers. APFS takes a new file's id from the volume's next object
  id, and on the test disk 1,022 new ids only grew. On a macOS computer the engine can write a
  rename as a rename, within the grain of checkpoints. Proposed: "A rename is written as a rename
  where the filesystem keeps inode numbers (APFS), and as a delete and a create where it reuses
  them (ext4)." The 4,939 of 10,000 figure stays as the reason for ext4.
- **`file-changes-from-outside`: "The time follows the size of the tree."** True of the walk the
  trials used. On APFS, the leaf-address diff makes the time follow the index of the tree and the
  changes, except when every leaf changed. The 3 to 22 s figures should not be carried over to
  macOS computers as their expected cost.
- **The first build's proposal names libfsapfs, "read-only and fuzzed", as the reader.** It is
  fuzzed, but in my tests it fails on two cases a computer meets in normal use: many local
  snapshots, and a clone taken during a write. It is not a drop-in reader for Silo.
- **`fuzz-untrusted-parsers`** asks for fuzzing for crashes, hangs and broken assertions, and
  tests at every limit. For a filesystem reader that is not enough. The dangerous bug is a reader
  that does not crash and reads a different tree than macOS does, so a file stays hidden.
  Proposed addition: "A parser of a format another program also reads, such as a filesystem,
  rejects any input that the other program could read differently, and its tests include inputs
  built to be read two ways."

## Not verified here

- Speed on a disk with millions of files: estimated from the record sizes of the test disks.
- Whether `F_LOG2PHYS_EXT` tells two APFS clones' blocks apart, and how fast.
- Whether root inside a macOS computer can write to the raw boot disk with System Integrity
  Protection on.
- That a Silo macOS computer's disk has the three-container layout of a Mac's internal disk.
- That the test disks were written by macOS 26 and not 15: their files are dated October 2025.
- Whether FileVault can be turned on in a macOS 26 computer.

## Sources

- Apple File System Reference, 2020-06-22:
  https://developer.apple.com/support/downloads/Apple-File-System-Reference.pdf (read: objects
  never modified in place; `o_xid`; object map lookup; `j_inode_val_t`; `j_file_extent_val_t`;
  `apfs_snap_meta_tree_type` physical; `APFS_FS_ALWAYS_CHECK_EXTENTREF`; `apfs_next_obj_id`).
- Apple security notes: macOS Tahoe 26.7.2, https://support.apple.com/en-us/149522; macOS Sequoia
  15.7.8, https://support.apple.com/en-us/128071 (read).
- Two macOS virtual machines at most: https://developer.apple.com/forums/thread/729580 and
  https://eclecticlight.co/2022/08/04/virtualisation-on-apple-silicon-macs-8-how-apple-limits-vms/
  (search results, not read in full).
- libfsapfs https://github.com/libyal/libfsapfs, apfs-fuse https://github.com/sgan81/apfs-fuse,
  dissect.apfs https://github.com/fox-it/dissect.apfs, apfsprogs
  https://github.com/linux-apfs/apfsprogs, OSS-Fuzz `projects/libyal`
  https://github.com/google/oss-fuzz/tree/master/projects/libyal (all cloned and read; the first
  three run here).
- Time Machine and snapshot diffs:
  https://eclecticlight.co/2021/03/11/time-machine-to-apfs-understanding-backups/ (search result).
- File systems in macOS 26:
  https://eclecticlight.co/2025/11/18/which-local-file-systems-does-macos-26-support/ (read).

The trial's scripts are kept outside the repository, in `apfs/scripts/` of the project folder `research/2026-11-24-first-build-decisions-scripts/`
(`apfsdiff.py` the reader, `allpairs.py` the 711 pairs, `ids.py` the inode ids). The libfsapfs
patch is `apfs/libfsapfs-snapshot-tree.patch` there.
