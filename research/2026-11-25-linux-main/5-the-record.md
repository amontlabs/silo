# Step 5 under B: the record of a Linux main, and the morning

Part of option B's specs ([README](README.md)). It takes the place of #370's
`5-the-record.md` for a Linux main and builds the same features: `file-changes-in-the-record`,
the rest of `record-of-a-computer`, and, as #373 moves them, `look`, `put-back` and `import` right
after the first build. It needs step 4 under B.

What stays as #370 wrote it: the comparer's place (a source of the recorder, never through the
core), falling behind, the closing `compared` line of each pair, the command's views and their
folding, `silo verify`, put-back as an operation through the running computer, import, the record
of step 5 and its crashes. This page says what differs: the reader reads ext4, and it must replay
ext4's journal.

## What you can do when it is done

```
$ silo
main · running · 8 h since you last looked (Mon 22:58)

Needs you
  claude  «All tests pass.»  ◇ the last whole `npm test` exited 1 at 03:12       silo receipt c-7f2a
  codex   deleted ~/work/shared-fixtures/ · 3,412 files · outside your repositories    02:40 → 02:45
  codex   sent 2.1 MB to paste.rs · first time seen                                     02:47
  still running from the night: node dist/server.js (pid 8812, since 01:14, started by claude)

Since Mon 22:58
  96 checkpoints · 7,412 files changed · 3,412 deleted · disk +2.1 GB
  The Mac stayed awake. Main did not restart.

Outside your repositories (~/work/app, ~/work/client-b)
  deleted   ~/work/shared-fixtures/                       3,412 files  −1.2 GB   02:40 → 02:45
  changed   ~/.gitconfig                                                         02:20 → 02:25
  new       ~/.config/systemd/user/worker.service                                03:40 → 03:45
```

Paths are main's: `~` is `/home/polarzero`. The line "still running" comes from step 7's probe and
is a report; under A it is "not known yet". Which conversation changed which file is still not
claimed, as #373 says.

## The disk reader for ext4

`file-changes-from-outside` as #352 file 09 words it: Silo's own reader, never a mount, a gap for
what it cannot read. #370 specifies an APFS reader; under B the reader reads ext4 instead, which
is simpler in three ways and harder in one.

| | APFS (A) | ext4 (B) |
|---|---|---|
| The format's documentation | Apple's reference, and #352's reading of what it leaves out | The kernel's own documentation, and e2fsprogs as a second reader |
| File contents for `look` and `put-back` | zlib, LZVN and LZFSE in `decmpfs` | Plain extents. No compression in ext4 |
| Encryption inside | FileVault turned on in main: a gap for a whole volume | fscrypt on a folder: names and contents of that folder are a gap; the rest reads |
| A copy of a running disk | Copy-on-write: the newest whole APFS checkpoint is a consistent tree | **Writes in place, with a journal: the copy is consistent only after its journal is replayed** |

### The journal has to be replayed

ext4 writes its metadata first into its journal, and into its place later. A copy of a running
disk, which every checkpoint is, holds changes that exist only in the journal. Main's kernel
replays them at the next boot. A reader that ignores the journal reads the disk as it was before
them.

Measured on 25 November 2026 ([proto/results.txt](proto/results.txt)): an ext4 image mounted in
a Linux container, a burst of changes, then copies of the image while it is still mounted.

- **7 s after the changes, a reader that ignores the journal saw none of them**: 100 deleted files
  still there, 200 new files absent. At 30 s, still none. At 120 s, all of them, once the kernel had
  written its metadata in place.
- **On Ubuntu 26.04's own root filesystem, the trial reader listed the same 107,853 paths as the
  kernel**, with the same inode, type, size and mode, in 1.4 s.
- **With the journal replayed in memory, the trial reader matched the kernel exactly**: 3,350 and
  1,700 paths, inode, type, size and mode, against the same copies after `e2fsck` replayed them and
  the kernel mounted them read only. The diff of the two copies found the 1,960 changes; without
  the replay it found none.

With checkpoints every 5 minutes, most changes of the last minute or two before a checkpoint would
otherwise be missing from it, then show in the next one, with the wrong window. So the reader
replays the journal, always, and never writes it back.

### What it reads

The trial in [proto/ext4read.py](proto/ext4read.py) is the shape; Silo's reader is Zig, as #370's,
with the same rules: `ReleaseSafe`, a memory cap fixed at start, a budget of blocks, a limit of
depth, a check for cycles, fuzzed, and a gap naming the rule for any disk that breaks one.

1. **The partition table.** Ubuntu's cloud image is GPT with the root filesystem labelled
   `cloudimg-rootfs`. The reader finds it by its label and type. Another partition that holds a
   filesystem is named in a gap, `partition not read`.
2. **The superblock and the group descriptors.** Features the reader does not know refuse the
   disk with `ext4-feature <name>`. Known: the features of Ubuntu 26.04's root filesystem, read
   on 25 November 2026: `has_journal`, `ext_attr`, `resize_inode`, `dir_index`, `orphan_file`,
   `filetype`, `extent`, `64bit`, `flex_bg`, `metadata_csum_seed`, `sparse_super`, `large_file`,
   `huge_file`, `dir_nlink`, `extra_isize`, `metadata_csum`; its journal has revokes, 64-bit tags
   and checksums v3. Refused until read: `inline_data`, `encrypt` on the whole filesystem, `casefold`.
3. **The journal.** jbd2's three passes, in memory: find the last committed transaction from the
   journal's start, collect the revoke records, then lay the journal's copies of blocks over the
   disk's blocks. The trial skips the checksums; Silo's reader checks each descriptor, block and
   commit checksum and stops at the first that fails, as the kernel does.
4. **The tree.** Inodes, extent trees, directories (linear and hashed, read as blocks of entries),
   symbolic links. Names as bytes.
5. **For `look` and `put-back`, contents.** Extents, read from the checkpoint. An extent marked
   unwritten reads as zeroes.

What it gives for each case main meets:

| In main | The reader gives |
|---|---|
| Ordinary work | The paths created, changed and deleted |
| A checkpoint taken while ext4 is writing | The disk as main will see it at its next boot: the journal replayed |
| fscrypt turned on for a folder | A gap: "files of ~/secret not read: encrypted", and every other path |
| A disk repartitioned or reformatted by root | A gap naming what was found; the next comparison is against the last checkpoint that read cleanly |
| A loop file, a Docker volume, a second filesystem inside a file | The file, as a file. What is inside it is not read |
| A file created and deleted between two checkpoints | Nothing, as #370 says |

### Comparing two checkpoints

ext4 writes in place, so #370's comparison "by leaf address" (an APFS node never moves once
written) does not apply. The comparer needs the set of disk blocks that changed between the two
checkpoints, and reads only the metadata among them.

| Strategy of step 4 | Where the set of changed blocks comes from |
|---|---|
| `delta` | `ckpt/<id>/writes`: the bitmap the write log swapped out at the checkpoint's moment. Exact, and a fact: it is what libkrun wrote, seen in the helper |
| `clone` | The blocks of the two `disk.raw` clones that APFS no longer shares, by #359 test 05's method (`F_LOG2PHYS_EXT` on both clones) |
| Either, as a fallback | A full read of every inode table: about 1 GB for main's 32 GiB disk, since Ubuntu's image has one inode per 8 KiB (327,680 inodes for 2.5 GiB) and growing the filesystem keeps that ratio. Used for the first comparison after a gap |

From the set, the comparer reads:

- the inode-table blocks in the set: every inode in them is compared with its old copy;
- the directory blocks in the set: every entry in them is compared with the old block;
- the journal blocks in the set: their transactions are replayed, and the blocks they write are
  added to the set before the two steps above.

A changed inode that is not a directory gives its path from the directory entries that name it,
read from a cache of `(inode, generation) → parent and name` kept per checkpoint in
`ckpt/<id>/names` (the counterpart of #370's `leaves`), rebuilt by a full walk when missing.

### What a file fact holds under B

#370's file fact, with the APFS object id replaced:

```json
{"v":1,"seq":20871,"prev":"…","time":"2026-11-25T02:45:03.120Z","computer":"c1","source":{"id":"disk","count":"k:91","n":5512},"is":"fact","kind":"file","between":[412,413],"change":"deleted","type":"dir","path":"/home/polarzero/work/shared-fixtures","inode":[1311,3518211022],"size":[0,0],"what":[]}
```

- `inode` is the inode number and its generation. ext4 gives a freed inode number to the next new
  file, so the number alone does not name a file across checkpoints; the generation, which the
  kernel sets at each allocation, does. A rename is still written as a delete and a create
  (`file-changes-in-the-record`), and both lines carry the same pair, so a reader can join them.
- **Content changed is a fact under `delta`.** A file's content changed when one of its data
  blocks is in the write log's set, whatever its times say. Under `clone` it is a block of the file
  that APFS no longer shares. A file rewritten with the same bytes is shown as changed; that is
  what the disk did.
- Times inside main (`mtime`, `ctime`) are what main's clock and `touch` say. They are not used to
  decide what changed, and are not in the fact.

### Shipped data

`engine/data/linux-noise.zon` replaces `macos-noise.zon`: what Ubuntu writes by itself on an idle
night (journald's files, `apt`'s lists, `/var/lib/systemd/timers`, `snapd` if present), measured
by the kit's test K16, an idle night on a Linux host, then on the Mac. `rebuildable.zon` is the
same file as A's: `node_modules`, `.venv`, `target` and their lock files do not depend on the
system.

## Put-back and import

As #370 and #365 specify. Two simplifications:

- `sudo tar -x --same-owner` keeps owners, modes and extended attributes on Linux as on macOS.
  There is no `decmpfs` to read, so a file the reader cannot copy is only an encrypted one.
- `silo import` copies into `/home/polarzero/Imports/` by default.

## Cost targets

| Measure | Target |
|---|---|
| A comparison of a pair with 1,000 changed files, on a disk of 3 million files, under `delta` | under 2 s, reading under 20 MB |
| The same under `clone` | under 5 s, reading under 50 MB, as #370 |
| A full read of the inode tables, the fallback | under 30 s for main's 32 GiB disk |
| The reader's memory | under 256 MB, fixed at start |

The `delta` target is lower than #370's because the set of changed blocks is given, not found.
Both are proposals; nothing has run at that scale.

## Tests

| Test | Checks | Where |
|---|---|---|
| The reader against the kernel, on copies of a mounted image taken while it is mounted, as [proto/check.sh](proto/check.sh) does for the trial | Every path, inode, type, size and mode equal to `e2fsck`'s replay mounted read only | CI on Linux (a loop device, no KVM) |
| The same with every ext4 feature of Ubuntu's image, and with a journal of each format (32 and 64-bit tags, checksums v2 and v3, revokes, a wrap of the journal) | Equal | CI |
| Each journal fault planted: a bad commit checksum, a sequence gap, a revoke past the end, an escaped block | The replay stops where the kernel stops, or a gap names the rule; never a crash | CI |
| One planted fault per rule of the tree (a cycle, an extent past the end, an entry longer than its block, a depth beyond the limit) | A gap naming the rule | CI |
| Fuzzing each parser and the whole reader, with a block budget | `fuzz-untrusted-parsers` | CI on fixed seeds, nightly longer |
| The comparer from the write log's set against a full walk of both checkpoints, over 500 random pairs from a recorded night | Equal results | CI on Linux |
| Kit K11: a night's work in main with checkpoints every 5 minutes, each pair compared both ways | Equal; the set-based comparison reads what changed, not what exists | a Linux host with KVM |
| Kit K12: fscrypt turned on for a folder in main | A gap for that folder, every other path read | a Linux host with KVM |
| #365's put-back scenario on a Linux main | The folder comes back; the 2 edits are shown | a Linux host with KVM, then the Mac |
| #359 test 05 on `disk.raw` | The blocks two APFS clones differ in, for `clone` | polarzero's Mac |
| The reader's sandbox on the Mac (`sandbox_init`) | As #370 | polarzero's Mac |

## Done when

#370's conditions, with the APFS cases replaced:

- the reader is fuzzed, and its walk of any checkpoint equals the kernel's after a replay of the
  journal;
- on main after a night, `silo changes` matches a full walk of both checkpoints, and costs what
  changed;
- fscrypt on a folder produces a gap for that folder, never "nothing changed".
