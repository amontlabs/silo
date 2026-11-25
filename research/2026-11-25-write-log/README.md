# The write log: libkrun's hook, Silo's side of it, and the checkpoint reader

An agent's work, 25 November 2026, for polarzero and for the agents who will build. Option B's specs
(#381) need a hook in libkrun's block device that tells Silo about each write of main, and a reader
of ext4 checkpoints. #381 specified the hook but could not write it. This folder writes it against
libkrun, builds it and tests it; writes Silo's side of it in Zig; and turns #381's trial ext4 reader
into one a checkpoint path can use. It is stacked on #381 by merge and changes no verdict.

## What was found

1. **The hook is written, and libkrun builds with it.** Two commits on libkrun's `main` of
   23 November 2026, in [`libkrun/`](libkrun/). The C call is
   `krun_block_device_set_write_hook(handle, hook, user)`. libkrun calls
   `hook(user, offset, len)` before each write, write-zeroes and discard, on the block device's
   thread, with a range already checked to lie inside the disk. A return other than 0 fails the
   request and leaves the disk as it was. 19 unit tests pass in debug and release builds
   ([libkrun/results.txt](libkrun/results.txt)).
2. **libkrun had a bug the hook needed fixed first.** The write, discard and write-zeroes paths
   computed `sector * 512` without a check. For a sector of 2^55 or more, a release build wrapped the
   offset: a write to that sector landed on sector 0 and the guest was told it worked. A debug
   build panicked on the device's thread. The first commit refuses such requests. It is a fix on its
   own and does not need the hook.
3. **A write in flight at a checkpoint's moment needs a rule that #381's spec lacks.** #381 puts the
   moment of a `delta` checkpoint "between two requests". libkrun calls the hook before a write
   lands and gives no call after it, so the moment can fall between the two. Without a rule, the
   checkpoint copies the block before the write lands, and the next checkpoint misses that write
   even though it completed long before. The rule: libkrun changes a disk from one thread, one
   request at a time, so at most one write is in flight, the last range the hook saw. The moment
   marks that range again for the next checkpoint. A scripted test shows the miss without the rule.
   A simulated night of 20,000 requests and about 20 checkpoints failed in each of 5 runs without
   the rule and passed in each of 10 runs with it ([writelog/results.txt](writelog/results.txt)).
4. **The hook costs main little.** Silo's side, in [`writelog/`](writelog/), took 30 to 40 ns per
   4 KiB write with no checkpoint being made, about 0.1 µs while one is copied, and 1.6 to 1.8 µs at
   the median when it first copies the block itself. #381's target is under 2 µs at the median on a
   Mac (its test L6). These are Linux numbers, in the page cache.
5. **A checkpoint can be read in place, through its deltas.** The reader in [`reader/`](reader/)
   reads a `delta` checkpoint as the first copy with every delta up to it laid over it, without
   building the disk. It finds main's root filesystem in the GPT disk by its label,
   `cloudimg-rootfs`, as Ubuntu's image names it.
6. **The journal's checksums are checked.** The reader checks jbd2's v3 checksums of every
   descriptor, revoke, commit and data block it replays. A bad descriptor, revoke or commit block
   ends the log there. A bad data block is not replayed and is named in a gap. On 6 copies of a
   running disk taken during a night's work, with 1,231 journal blocks to replay, the reader gave
   every path, inode, type, size and mode the kernel gives after `e2fsck` replays the journal.
   150 random bit flips in the journal gave 141 gaps, 5 disks refused by a named rule, 4 clean
   reads, and no crash.
7. **Comparing from the write log reads a small part of the disk.** On Ubuntu 26.04's own disk grown
   to about a million paths, 1,000 files changed between two copies of the running disk. Given the
   blocks written between them, the comparer read 13 MiB and gave the same 1,362 lines as a full
   walk of both copies, which read 533 MiB ([reader/results.txt](reader/results.txt)). #381's
   target is under 20 MB on a disk of 3 million files. On the small test disk the full walk reads
   almost nothing from the disk, since the journal replay already holds all recent metadata.
8. **A file rewritten in place shows up only through the write log.** One block of a file rewritten
   with the same size and extents shows only a changed mtime to a full walk. The write log names the
   file, whatever its times say.

## The folders

| Folder | Holds |
|---|---|
| [`libkrun/`](libkrun/) | The two commits as patches for `git am`, and their test runs, before and after |
| [`writelog/`](writelog/) | Silo's side of the hook in Zig 0.17.1: the bitmap of blocks written, `delta` checkpoints by copy before write, the C function given to libkrun, tests and a benchmark |
| [`reader/`](reader/) | `ckptread.py`, the reader; `test_ckptread.py`, its tests against the kernel; `scale.py`, the measure on Ubuntu's disk |

Run them: `git am libkrun/*.patch` on libkrun at the commit above, then `cargo test --features blk
--lib virtio::block` in `src/devices`. `zig build test` and `zig build bench` in `writelog/`.
`sudo python3 -I test_ckptread.py` in `reader/` (root for loop devices; no KVM).

## What it changes in the plan

Agents' notes; no answer, `sets` or verdict changed:

- `libkrun-patches`: the patch exists, and the first commit is a fix upstream can take alone.
- `engine-patch-review`: the patch's hostile test, and that libkrun has no fuzz targets yet.
- `checkpoint-per-system`: the rule for the write in flight at a checkpoint's moment.
- `file-changes-from-outside`: reading only the blocks written, measured on ext4.

`plan check` passes.

## What was run, and what was not

- **Ran**, in a Linux container with no KVM: libkrun's unit tests of the block device, debug and
  release; the Zig tests and benchmark; the reader against the kernel on loop devices; the measure
  on Ubuntu 26.04's image.
- **Not run**: no virtual machine called the hook. The container has no `/dev/kvm`, and no NBD or
  ublk device through which real kernel writes could reach Silo's side. The kit's tests K07 and K08
  (#381) on a Linux host with KVM are the next step, then L6 on the Mac.
- libkrun's full clippy matrix of CI did not run: its init blob does not build here. Clippy on the
  two crates the patch touches is clean for its files.
- The reader's handling of a bad journal block is a policy chosen here. It was not compared with
  what the kernel does with the same planted faults. Only jbd2 checksums v3 were exercised; v2 is
  written and untested.
- The reader is Python, so its times say nothing about Silo's Zig reader. Its counts of blocks read
  do.
- The 3 million files of #381's target were not reached: the measure used about a million.
- The commits carry an agent's `Signed-off-by`. libkrun's DCO needs the person who sends them.

## For the README's "research at a glance"

> libkrun's write hook written and tested (2 commits, one an upstreamable fix for a guest write to
> sector 2^55 that landed on sector 0), Silo's Zig side measured at 30 ns to 1.8 µs a write, a rule
> found for the write in flight at a checkpoint's moment, and the ext4 reader made a checkpoint
> reader: with the write log it compared a million-path disk reading 13 MiB instead of 533 (#398).

## For polarzero to rule

- **Send the two commits to libkrun**, in that order, under your sign-off. The first fixes a bug
  any libkrun user can hit; the second is the hook `libkrun-patches` asks to upstream. Recommended:
  send the first now, the second once K07 has run on a KVM host.
- **The in-flight rule against a second call after each write.** Recommended: the rule, which keeps
  libkrun's change to one call. A second call is needed only if libkrun starts writing a disk from
  several threads ([checkpoint-per-system notes](../../plan/going-back/decisions/checkpoint-per-system.md)).
- **When a copy fails during a checkpoint**, this prototype fails the checkpoint and lets main's
  write go on. The other way, failing main's write, keeps the checkpoint but shows main a disk
  error. Recommended: fail the checkpoint, and say so in the record.
