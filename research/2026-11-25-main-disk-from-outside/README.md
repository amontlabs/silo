# Reading main's disk from outside, when main may be hostile

Research of 25 November 2026, backlog item 10 of the day: "an APFS reader that checks every decoded
file against a size or hash, fuzzed", and the ext4 journal of #381. Everything ran on Linux, with no
KVM and no Mac. The prototypes are in [`proto/`](proto/); [`proto/run.sh`](proto/run.sh) runs every
measure from nothing, and [`proto/results.txt`](proto/results.txt) holds the numbers below.

Silo reads main's disk from outside for `file-changes-from-outside` and, in #370's specs, for `look`
and `put-back`. Root in main can write any byte of that disk. So the reader reads bytes an agent may
have crafted, with Silo's rights on the Mac.

## What was found

1. **A size check does not catch a corrupted LZVN or LZFSE block.** #390 found that one changed
   byte decodes to wrong bytes with no error, and asked the reader to check every file "against
   the size, or a hash Silo keeps". Measured over 361 blocks of 64 KiB from source code, prose,
   JSON, binaries and images, with 200 single-bit flips each:

   | Format | Wrong bytes with no error, any size accepted | With #390's exact size check | Also requiring the whole input read |
   |---|---|---|---|
   | LZFSE | 72.9% | 64.1% | 64.1% |
   | LZVN | 78.5% | 69.0% | 69.0% |
   | zlib (checks its Adler-32) | | 0.0% | |

   A third of an LZVN block's bytes are literals, copied to the output as they are; no rule of the
   format can check them. The rest of the flips land in lengths and distances, which often still
   give the right size.

2. **There is no hash to check against, and macOS reads the same wrong bytes.** APFS checksums its
   metadata, not file data: a plain file with one bit flipped reads wrong, silently, in macOS too.
   A file compressed in its xattr (up to one block) sits inside a tree node, so the node's Fletcher-64
   catches a flip there (from the format; not measured here); one compressed in a resource fork
   does not. A hash "Silo keeps" of a file
   would have to be computed inside main, when the file is written. So the reader's bar is the
   bytes macOS gives for the same disk, with the same refusals, not the bytes first written. Two
   things can still make the reader differ from macOS, and both are tested below: a bug in the
   reader, and a decoder that reads a crafted block differently from Apple's.

3. **Apple's open-source lzfse reads past the end of a crafted LZFSE block.** A coverage-guided
   fuzzer ([`proto/fuzz_lzfse.c`](proto/fuzz_lzfse.c), lzfse built with AddressSanitizer) found it
   within seconds: `fse_in_checked_flush64` (lzfse_fse.h:372, called from `lzfse_decode_lmd`) loads
   8 bytes at a position up to 7 bytes before the end, so it reads up to 7 bytes past the input.
   The input is [`proto/lzfse-overread.input`](proto/lzfse-overread.input) (replay:
   `fuzz_lzfse lzfse-overread.input`). The bytes read past the end are masked off and do not reach
   the output, as far as the code shows. The risk is a crash: a reader that decodes straight from a
   mapping of the disk image, at the end of a page, takes a fault. With 8 zero bytes after every
   input, 36.4 million inputs over 15 minutes found nothing more, in LZFSE or LZVN. lzfse's last
   commit is from 2017; no report of this was found. Reporting it upstream is public, so it is
   polarzero's call.

4. **#390's probe crashes or exhausts memory on crafted disks, 8 faults at 6 places.** A fuzzer of the whole
   reader ([`proto/fuzz_reader.py`](proto/fuzz_reader.py), atheris) patches the blocks the probe
   reads and recomputes their checksums, as root can, then lists and reads every file as `look`
   would. In 69,696 inputs it found (a `MemoryError` and an `OverflowError` at the same line count as two):

   | Fault | Where | What an agent gets |
   |---|---|---|
   | An extent whose logical offset is huge: the probe fills the hole with zeroes | apfscontent.py:246 | Gigabytes allocated, or an `OverflowError` |
   | A stream whose size is huge | apfscontent.py:262 | The same |
   | An extent whose length is huge | apfscontent.py:249 | The same |
   | A key shorter than 8 bytes | apfscontent.py:144 | The whole listing stops |
   | A directory entry's value shorter than 18 bytes | apfscontent.py:206 | The whole listing stops |
   | An index value of a tree node shorter than 8 bytes | apfsread.py:291 | The whole listing stops |

   One file an agent crafts makes `look` fail for every file, or uses the Mac's memory up. It is the
   rule `parsers-fuzzed-and-compared` names (#376): a crash is found by fuzzing, and here fuzzing
   found it.

5. **Six rules make the same fuzzing find nothing.**
   [`proto/content_reader.py`](proto/content_reader.py) is the probe rewritten with them. It reads
   the same 1,067 files of dissect.apfs's three test disks to the same SHA-256 as the probe, every
   compressed method included. Under the same fuzzer and seed for the same time, it showed no fault
   (`results.txt` has the run counts).

   - **R1** Every length read from the disk is checked before anything is unpacked from it.
   - **R2** No size from the disk decides an allocation: a file, a stream or a hole is bounded by
     the container's size and a read budget before a byte is allocated.
   - **R3** A file's extents are in order, do not overlap, have a length, and end inside the
     container.
   - **R4** decmpfs's table of blocks is checked whole (count from the size, every block inside the
     fork, none empty) before any block is decoded.
   - **R5** The decoder gets its input with 8 zero bytes after it, and must give exactly the
     block's size. A short decode is a gap: QEMU's DMG driver shows what happens otherwise
     ([qemu#4662](https://gitlab.com/qemu-project/qemu/-/work_items/4662): a short LZFSE decode
     hands stale heap bytes to a guest).
   - **R6** The walk has a depth limit, a budget of entries, and refuses a directory reached twice.

6. **On ext4, a reader that skips the journal's checksums shows a disk the kernel will not mount.**
   #381's trial replays the journal and skips its checksums. With the checksums of
   journal_checksum_v3 (which ext4 turns on with `metadata_csum`, as on Ubuntu's image) verified
   as the kernel's `recovery.c` does ([`proto/ext4read.py`](proto/ext4read.py)), each planted
   fault ([`proto/journal-faults.sh`](proto/journal-faults.sh)) gives:

   | Fault planted in the journal | Kernel (loop mount) | e2fsck | #381's trial | With the checksums |
   |---|---|---|---|---|
   | None | mounts, 302 paths | replays | same 302 paths | same 302 paths |
   | One byte of a data block | refuses to mount | "Invalid checksum recovering data block" | a tree, 303 lines off | gap `journal-data-checksum` |
   | One byte of a descriptor block | refuses | "Journal checksum error" | a tree | gap `journal-checksum` |
   | One byte of a commit block | refuses | "transaction 2 was corrupt, replay was aborted" | a tree | gap `journal-commit-checksum` |
   | One byte of the journal's superblock | refuses | "Journal superblock is corrupt" | a tree | gap `journal-superblock-checksum` |
   | A sequence number out of order | mounts, 401 paths: replay stops at the gap | replays | same 401 paths | same 401 paths |

   "A tree" means the trial showed paths as if the disk were fine, while main itself would not
   start from that disk without `fsck`. The checked reader says so instead, and matches the kernel
   wherever the kernel mounts.

## What it changes in the plan

- **`file-changes-from-outside`** (open): its notes ask "how the host reads a disk that a hostile
  agent wrote". This answers part of it: the six rules, the decoder padding, and the bar of
  "the bytes macOS gives". A note is added to the entry.
- **`fuzz-untrusted-parsers`** (taken by agents), and its replacement `parsers-fuzzed-and-compared`
  proposed in #376: the probe's faults are the kind the taken answer is about, and finding 2 is a
  case for the replacement's added sentence (the reader must read what macOS reads). A note is
  added to the taken entry; nothing in its answer changes.
- **#370's reader spec and #390's test 39** (research, not entries), challenged:
  - "Check what it reads against the size, or a hash Silo keeps" (#390): the size catches about
    one flip in ten, and no such hash exists without a program inside main. Proposed in its place:
    the reader must give what macOS gives. On the Mac it decodes with `libcompression`, Apple's
    system library, instead of a copy of lzfse; liblzfse is
    for Linux tests only. Test 39 already compares against what main itself reads. Adding crafted
    blocks to it (the fuzzer's corpus, written into main by root) would show whether
    `libcompression` and the kernel ever disagree; that needs the Mac.
  - The reader decodes in a process with a memory cap, as #370 says. R2 and R5 are needed anyway:
    a cap turns the memory faults into a stopped reader, which is still a failed `look`.
- **New, for polarzero: should a checkpoint carry a hash of its blocks?** The host's APFS does not
  checksum data either, so a block of a checkpoint that rots on the Mac's disk goes unseen, and
  `put-back` restores it. Silo is the one writer of its checkpoints, so it is the one place a hash
  can be kept. SHA-256 of 4 KiB blocks ran at 1.1 GB/s on one core here (Linux, SHA extensions);
  not measured on a Mac. Options: no hash (as now); a SHA-256 per block written, kept with the
  checkpoint and checked on every read of it; a hash per checkpoint file, checked only on restore.
  Recommended: per block written, because `look` reads single blocks and the writes of a checkpoint
  are already listed (#365's write log). Not written as an entry here; it needs a place in
  `going-back`'s order that the integration branch should choose.

## Not verified

- Nothing ran on a Mac. `libcompression` was not fuzzed, and whether the kernel's decmpfs reads a
  crafted block as `libcompression` does is unknown.
- Whether root inside main can write a crafted decmpfs file at all, with SIP on (the xattr is
  protected from users; a raw write to the disk device may be refused). If it cannot, findings 3
  to 5 matter only for a disk an agent prepares some other way, such as an image it attaches.
- The fuzzers are a few minutes each, on one disk image of 4 MiB with 20 files. A longer run on
  main's real disk is test 39's work on the Mac.
- ext4: only the journal's checksums. Inode, extent and directory checksums (`metadata_csum`) are
  not checked by the trial or here; the kernel checks them when it reads each block.
- The over-read in lzfse was read in the code to be harmless to the output; that was not proven.

## The research at a glance

Main's disk from outside: a size check misses 64% of corrupted LZFSE and 69% of LZVN blocks, and
no file hash exists to check against, so the reader's bar is what macOS reads; Apple's lzfse reads
up to 7 bytes past a crafted block (fixed by padding); #390's probe crashes or exhausts memory on
crafted disks (8 faults) and six rules fix all of them under the same fuzzing; on ext4, verifying the
journal's checksums makes #381's reader refuse exactly the 4 disks the kernel refuses.

## For you to rule

- Report the lzfse over-read upstream (public, so yours to send).
- Challenge to #390 and #370: decode with `libcompression` on the Mac, and judge the reader against
  what macOS reads, not against a hash Silo cannot have.
- A hash per checkpoint block, as above: a new open choice for `going-back`.
