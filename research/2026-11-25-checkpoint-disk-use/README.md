# Stretching how far back checkpoints reach on a Mac

An agent's research, 25 November 2026. #389 found that a 512 GB Mac keeps only 2 to 4 days of
checkpoints when an agent rewrites 2 GiB an hour. This report measures, on Linux, four ways to make
the same disk hold more days, and puts each one into #389's simulation to give the days back it
gains. It stacks on #389 by merge and reuses its simulation unchanged.

The four ways:

1. Compressing and deduplicating what checkpoints hold.
2. Leaving rebuildable folders out of checkpoints (`target`, `node_modules`, package caches).
3. What APFS clones can and cannot do here (research only, no Mac).
4. Moving old checkpoints to an external disk.

Every number was measured in a Linux container (kernel 6.18, 4 cores, ext4 on a loop device, no
KVM). Nothing was measured on a Mac.

## What it found

1. **Compression gives the most days for the least work.** zstd at level 3, in frames of 64 KiB,
   stored what three days of agent work held in 24% of its size. In #389's simulation the 512 GB
   Mac at 2 GiB an hour goes from 4.3 to 26.3 days back. lz4 stored it in 34% (17.3 days). One core
   compressed 316 MiB a second, over 500 times the rate of 2 GiB an hour.
2. **Compression only helps checkpoints Silo writes as its own files.** An APFS clone cannot be
   compressed or deduplicated: APFS has no block dedup, and its file compression undoes itself when
   a file is written. In #365's `delta`, the oldest checkpoint is a clone and only the deltas after
   it are Silo's files. That clone held 25% of the checkpoints' space at 2 GiB an hour, and 68% on a
   macOS computer without TRIM, so compression gives 13.3 days instead of 26.3 there, and 3.3 instead
   of 11.3 without TRIM. Copying the old blocks before they are overwritten (the opposite direction
   to #365's `delta`) leaves no clone. Every byte held is then in Silo's files. It also turns removing
   the oldest checkpoint into deleting one file.
3. **Rebuildable folders were 87% of what checkpoints held.** Over three days of installs, builds
   and edits, Rust's `target` held 49% of the old blocks, `node_modules` 24%, a Python virtual
   environment 13%, and the package caches under 1%. Code, Git and file system metadata held the
   rest. Leaving all of them out takes the 512 GB Mac from 4.3 to 25.3 days. Leaving out only what a
   setting can move (`target`, caches, virtual environments) gives 10.3.
4. **A folder cannot be left out by mounting a second disk on it.** With a disk mounted on
   `node_modules` or `target`, `rm -rf node_modules`, `rimraf`, `cargo clean`, `git clean -fdx` and
   `mv` each failed with "Device or resource busy". The contents were deleted, but the command exited
   with an error, so a script chained with `&&` stopped. A symbolic link fails another way: the
   first `npm install` replaced the link with a real folder. What works is pointing each tool's own
   setting at a second disk (`CARGO_TARGET_DIR`, the npm, pip and Cargo caches, Xcode's DerivedData
   location). `node_modules` has no such setting in npm.
5. **Deduplication matters for reinstalls, not for builds.** A second and third `npm install`
   after deleting `node_modules` held blocks that were 81% identical to the first. A rebuild of
   `target` was not identical to the one before. On its own, dedup stored 60% (8.3 days). Combined
   with zstd it stored 16%, and the 512 GB Mac reaches the 30 days of the age rule. It is the one
   way that keeps `node_modules` in checkpoints at a small cost.
6. **An external disk of 500 GiB reaches 30 days at 2 GiB an hour without changing anything
   else.** With checkpoints as Silo's own files, moving old ones out copies only those files. It
   gives 16.3 to 17.3 days at 5 GiB an hour or without TRIM, and 30 days with compression too.
   Restoring an old checkpoint needs that disk connected.

## Days back per way, on #389's simulation

[`sim/stretch.py`](sim/stretch.py) runs #389's
[`retention.py`](../2026-11-25-checkpoint-retention/sim/retention.py) with three knobs added from
outside, and with #389's recommended policy (`30d-half-oldest`: the age rule of
`thinning-checkpoints`, oldest first over a limit of half the free space, keeping 50 GiB free):

- `store_ratio`: bytes on the Mac for each byte checkpoints hold (compression and dedup).
- `excluded`: share of the computer's rewrites that land in folders left out, applied to the steady
  writes and to the nightly reinstalls.
- `external_gib`: room on an external disk for the checkpoints the Mac's limit would remove.

Reach in days on the last of 30 mornings. 29.3 means every day the age rule keeps, which is the
most there is. "No TRIM" is a macOS computer whose TRIM does not reach the image, at 2 GiB an hour.

| Way | Ratio, share | 512 GB, 0.5 GiB/h | 512 GB, 2 GiB/h | 512 GB, 5 GiB/h | 512 GB, no TRIM | 1 TB, 2 GiB/h |
|---|---|---|---|---|---|---|
| Nothing (#389) | | 17.3 | 4.3 | 1.2 | 2.4 | 14.3 |
| lz4 | 0.34 | 29.3 | 17.3 | 6.3 | 8.3 | 29.3 |
| zstd | 0.24 | 29.3 | 26.3 | 10.3 | 11.3 | 29.3 |
| zstd, if real data compresses only half | 0.50 | 29.3 | 10.3 | 2.3 | 5.3 | 29.3 |
| Dedup | 0.60 | 29.3 | 8.3 | 2.1 | 4.3 | 26.3 |
| Dedup and zstd | 0.16 | 29.3 | 29.3 | 18.3 | 17.3 | 29.3 |
| zstd, the oldest checkpoint a clone (#365's `delta`) | 0.43 | 29.3 | 13.3 | 4.3 | | 29.3 |
| The same without TRIM | 0.76 | | | | 3.3 | |
| Dedup and zstd, the oldest a clone | 0.37 | 29.3 | 15.3 | 5.3 | | 29.3 |
| Leave out what a setting can move | 63% | 29.3 | 10.3 | 3.3 | 7.3 | 28.3 |
| Leave out every rebuildable folder | 87% | 29.3 | 25.3 | 10.3 | 29.3 | 29.3 |
| Move by setting, and zstd | 63%, 0.25 | 29.3 | 29.3 | 20.3 | 29.3 | 29.3 |
| Leave out all, and zstd | 87%, 0.25 | 29.3 | 29.3 | 29.3 | 29.3 | 29.3 |
| External disk, 500 GiB | | 29.3 | 29.3 | 17.3 | 16.3 | 29.3 |
| External disk, 500 GiB, and zstd | 0.24 | 29.3 | 29.3 | 29.3 | 29.3 | 29.3 |

The ratios come from the capture below. "The oldest a clone" mixes the share that clone holds
([`sim/basefrac.py`](sim/basefrac.py): 25% at 2 GiB an hour, 28% at 5, 16% on the 1 TB Mac, 68%
without TRIM) at a ratio of 1 with the rest at the measured ratio. The external disk is modelled as
more room for the oldest checkpoints. The Mac's limit is unchanged. Every run's line is in
`results/stretch*.jsonl` and the table in [`results/reach.md`](results/reach.md).

## The capture: what three days of agent work leave in checkpoints

[`capture/`](capture/). A 24 GiB ext4 disk image on a loop device stands for the computer's disk.
[`capture/workload.sh`](capture/workload.sh) plays an agent's work in 28 steps, and
[`capture/ckcap.py`](capture/ckcap.py) takes a checkpoint after each one. Before each checkpoint it
runs `fstrim`, so freed blocks become holes, as on a Linux computer whose TRIM reaches the image.
For each 4 KiB block whose content changed since the previous checkpoint and was not zero, it keeps
the old content. That old content is what a checkpoint holds once the block is rewritten, the same
quantity #389 simulates. It also records which file owned each block, through FIEMAP.

The work: 3.2 GiB of system files, then `fastify` (npm), `ky` (TypeScript), `ripgrep` (Cargo) and a
Python virtual environment with NumPy and pandas. Then three rounds, each with: an edit and an
incremental Rust build, an edit and ESLint, `cargo test --no-run`, `rm -rf node_modules && npm
install`, an edit and a TypeScript build, a release build (round 2: `cargo clean` and a full build),
`pip install --force-reinstall pandas requests`, and `git gc`.

Across the 28 checkpoints the old blocks came to 2,286 MiB. By owner:

| Owner of the old block | MiB | Share |
|---|---|---|
| `target` (Cargo) | 1,128 | 49% |
| `node_modules` | 553 | 24% |
| A virtual environment | 305 | 13% |
| File system metadata (journal, directories, inode tables) | 282 | 12% |
| npm cache | 9 | 0.4% |
| Git | 8 | 0.4% |
| Code | 2 | 0.1% |

About 60 MiB of the metadata in each `npm install` round is the directories of `node_modules`. A
folder left out takes its directories with it, so 87% is a floor for that case.

Stored size, as a share of the 2,286 MiB ([`capture/analyze.py`](capture/analyze.py)):

| Store | MiB | Share |
|---|---|---|
| lz4, 64 KiB frames | 785 | 34% |
| zstd 1, each 4 KiB block alone | 703 | 31% |
| zstd 3, 64 KiB frames | 553 | 24% |
| zstd 3, 1 MiB frames | 492 | 22% |
| Dedup by 4 KiB block, against every block already stored and the first full checkpoint | 1,367 | 60% |
| Dedup, then zstd 3 | 364 | 16% |
| Every rebuildable folder left out, then zstd 3 | 27 | 1.2% |

Dedup saved 4% to 22% of `npm install` and `pip install` in the first round, and 81% to 95% in the
next two, which write the same files again. A `cargo build` after `cargo clean` was 18% identical to the build
before. Per step: [`results/capture-analysis.jsonl`](results/capture-analysis.jsonl).

Speed on one core, on the 858 MiB `cargo clean` step: zstd 3 316 MiB/s, zstd 1 411 MiB/s, lz4
626 MiB/s, zstd 3 decompression 721 MiB/s. Decompressing 20 GiB of held blocks takes about 30 s on one core.

## Leaving folders out: what works

`rule-that-names-paths` says how to leave a folder out of a checkpoint of the whole disk, while it
stays an ordinary folder, "is to find out". Tested here, in this container:

| Way | Result |
|---|---|
| A second disk mounted on the folder | The tools empty it, then fail: `rm -rf` and `mv` "Device or resource busy", `rimraf` EBUSY, `cargo clean` exit 101, `git clean -fdx` "failed to remove". A script with `&&` stops |
| A symbolic link to a folder on a second disk | The first `npm install` replaced the link with a real folder, so nothing was left out |
| Each tool's own setting, pointing at a second disk | Works, because the tool creates and deletes a folder inside that disk: `CARGO_TARGET_DIR`, `CARGO_HOME`, `npm_config_cache`, `PIP_CACHE_DIR`, `XDG_CACHE_HOME`, `GOCACHE`, `GRADLE_USER_HOME`, Xcode's custom DerivedData location, uv's `UV_PROJECT_ENVIRONMENT` |
| Silo reading the guest's file system from outside, and skipping the blocks of named folders | Not built. It needs a reader for ext4 and APFS, and a restore then has to delete those folders inside the restored disk |

The setting is the one way that holds up. Silo gives each computer a second disk that no checkpoint
holds, and the computer's recipe sets those variables. It reaches `target`, the caches and
virtual environments made through uv or Poetry: 63% of what this capture held, or 50% without the
virtual environment. It does not reach `node_modules` under npm, or a `.venv` made by `python -m
venv`. Dedup with compression covers those: the second and third `npm install` stored 1% of their
size.

On a restore, `partial-checkpoint-says-so` recommends that left-out folders come back empty. The
second disk makes that one step. Silo replaces it with an empty one. Caches are a different case
from build output: npm, pip and Cargo address their caches by content hash, so a cache from later
never gives a build the wrong file. Keeping the cache disk across a restore saves the downloads. The
build output still has to go.

## APFS clones (research only)

- APFS clones share blocks at the granularity of the file system's block, 4 KiB by default
  ([Apple File System Reference](https://developer.apple.com/support/downloads/Apple-File-System-Reference.pdf)).
  Overwriting part of a cloned image copies only the blocks written
  ([Eclectic Light](https://eclecticlight.co/2023/05/04/cow-and-clones-how-they-save-space-and-ssd-wear/)).
- APFS does not deduplicate. Two identical blocks written separately take twice the space. Only a
  clone shares them.
- APFS file compression (`decmpfs`) stores a file's data in an extended attribute and is
  decompressed when the file is written ([fscmp](https://github.com/Siguza/fscmp)). It does not apply
  to a disk image that a virtual machine writes.
- A clone cannot leave the volume. Copying it to another disk copies all of it
  ([Eclectic Light](https://eclecticlight.co/2024/03/20/apfs-files-and-clones/)). This is the full
  copy `checkpoint-storage` names as the cost of an external drive.
- ASIF, the sparse image format of macOS 26 for the Virtualization framework, is a sparse file on
  APFS. No public source says whether it frees space when the guest TRIMs, whether it can be cloned
  block by block, or what its internal block size is
  ([Eclectic Light, June 2025](https://eclecticlight.co/2025/06/12/macos-tahoe-brings-a-new-disk-image-format/),
  [September 2025](https://eclecticlight.co/2025/09/17/should-you-use-tahoes-new-asif-disk-images/)).
  It would need its own Mac test before Silo uses it.

So on a Mac, everything that saves space beyond clones has to come from Silo's own store: delta files
it writes, compresses and deduplicates itself. That is the case for `delta` over `full` in
`checkpoint-per-system`.

## Old blocks copied before they are overwritten

#365's `delta` keeps the first checkpoint as a clone and writes later checkpoints as forward deltas,
the new content of the blocks written. The other direction keeps the live image as the only full
disk and copies each block's old content into the open checkpoint's file before the first write to
it (#365's `cbw` step 3 already does this for one block). Each file then holds what its checkpoint
alone needs.

| | Forward deltas, oldest a clone (#365) | Old blocks copied before the write |
|---|---|---|
| Space that can be compressed | All but the oldest checkpoint (75% at 2 GiB/h, 32% without TRIM) | All of it |
| Removing the oldest checkpoint | Merge the next delta into the clone, which writes into blocks shared with the live image | Delete its file |
| Moving old checkpoints to another disk | The clone has to be copied in full | Move their files |
| Reading a checkpoint | The clone and every delta after it | The live image and every file newer than it |
| A write to a block not yet copied this period | Not delayed | Waits for one 4 KiB copy, once per block per period |
| If the live image is damaged | Older checkpoints survive | Every checkpoint is damaged too |

The last line is the cost. It can be bounded by keeping one full copy, compressed, as an anchor, and
renewing it weekly. That anchor is also what an external disk starts from. Not built here: it is
the next prototype for #365's engine.

## Moving old checkpoints to an external disk

- With checkpoints as Silo's own files, moving the oldest ones out copies those files only. The
  Mac's limit stays the same, and the external disk takes what the limit would have removed.
- What leaves the Mac stays out of the computers' reach, as `checkpoint-storage` requires. The
  external disk is not: anyone with the disk in hand can read it. The files hold the computer's
  secrets, so they are encrypted with a key kept in the Mac's Keychain.
- A restore to a checkpoint on the external disk needs that disk connected. When it is not, the
  Timeline shows those checkpoints as unreachable.
- A cloud store works the same way. At 2 GiB an hour and zstd's 24%, that is about 0.5 GiB an hour, or
  1.2 Mbit/s of upload, plus a first anchor. What it costs is a business question, kept out of this
  repository.

## What it changes in the plan

Agent notes appended to open entries. No verdict written, no list changed, no new entry.

| Entry | State | What the notes add |
|---|---|---|
| `checkpoint-per-system` | open | APFS clones cannot be compressed or deduplicated, and compression takes the 512 GB Mac from 4.3 to 26.3 days only with checkpoints as Silo's own files. Proposes old blocks copied before the write, so the oldest is not a clone |
| `rule-that-names-paths` | open | The way to build it: a second disk and each tool's own setting. A disk mounted on a folder breaks `rm -rf` and `cargo clean`. A symbolic link is replaced by npm. Days gained per way |
| `partial-checkpoint-says-so` | open | Caches addressed by content can stay across a restore. Build output still comes back empty |
| `checkpoint-storage` | open | An external disk needs no full copy when checkpoints are Silo's files. 500 GiB reaches 30 days at 2 GiB an hour. Encrypted, and unreachable when disconnected |
| `thinning-checkpoints` | open | With zstd the default limit reaches 26 days on a 512 GB Mac at 2 GiB an hour, so the 30 days of the age rule become reachable on most Macs |

## For you to rule

- **`checkpoint-per-system`'s answer, an APFS clone per checkpoint, gives up compression.** On a
  512 GB Mac at 2 GiB an hour that is 4.3 days back instead of 26.3. The answer should say
  checkpoints are Silo's own compressed files (`delta`), with the first full one as an anchor.
- **`checkpoint-storage` says checkpoints cannot move to an external drive without a full copy.**
  That holds for clones only. With Silo's own files, the oldest move as files.

## What was not verified

- The workload is installs, builds and edits of three open source projects. It has no databases, no
  media, no Docker images and no simulators, which compress worse. The row "if real data compresses
  only half" brackets that. A real night of agents on a Mac (#365, #389) replaces these ratios.
- The guest was Linux on ext4. A macOS computer on APFS writes its own metadata and, without TRIM,
  never frees blocks in the image. Its ratios are not measured.
- The simulation scales held bytes by one ratio. Real ratios vary by checkpoint, and dedup grows
  with the number of checkpoints kept.
- Leaving folders out was applied to the simulated write rate as one share. It assumes agents' writes
  look like this capture's.
- The second disk was tested with mounts on Linux. Whether a macOS computer can take a second disk
  and point Xcode's DerivedData at it is a Mac test.
- Old blocks copied before the write were not prototyped. The write wait comes from #365's `cbw`
  measurement of one 4 KiB copy.
- The external disk is modelled as extra room. Restore time from a USB disk was not measured.

## Running it

```
# As root on Linux, with Python 3, NumPy, zstandard and lz4. About 20 minutes.
truncate -s 24G live.img && mkfs.ext4 -q live.img && mkdir mnt && mount -o loop live.img mnt
python3 capture/ckcap.py caps live.img mnt s-empty
capture/workload.sh                       # edit E= at the top to point at the folder above
python3 capture/analyze.py caps > results/capture-analysis.jsonl

cd sim
python3 stretch.py --part=0/4 > ../results/stretch0.jsonl   # four parts side by side, about 5 minutes
python3 basefrac.py > ../results/base-share.jsonl
python3 table.py > ../results/reach.md
```
