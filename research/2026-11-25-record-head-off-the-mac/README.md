# The record's head off the Mac, and the writer through a power cut

Research of 25 November 2026, by agents, for polarzero. It builds on the record's format and its
prototype (#386, [`../2026-11-25-record-format/`](../2026-11-25-record-format/README.md)), and is
stacked on it. Two questions #386 left open:

1. `silo verify` misses two changes by design: the last lines cut with the head deleted, and the
   whole record rewritten by a program running as you, head included. Only a copy of the head that
   such a program cannot change shows them. Where does that copy go?
2. #386's crash test kills a process. What does the writer keep through a power cut, on Linux and
   on a Mac?

Nothing here is built and no entry is taken by it.

## What was found

1. **An anchor off the Mac catches both changes `silo verify` misses.** The prototype
   ([`anchor/`](anchor/)) sends the head after each flush, signed, to two kinds of anchor: a Git
   repository whose branch refuses force pushes and deletions, and a small ledger standing in for
   the team's host. The test ([`results/anchor.md`](results/anchor.md)) changes a real record of
   #386's prototype the way a program running as you could:

   | Change | #386 `silo verify` | Git anchor | Ledger |
   |---|---|---|---|
   | Nothing changed | holds | holds | holds |
   | The last 18 lines cut at a line's end, head deleted | holds | **found** | **found** |
   | Line 10 changed, every line after it chained again, head rewritten | holds | **found** | **found** |
   | The same, and the Git anchor attacked | holds | **found** | **found** |
   | The same, and forged heads posted to the ledger | holds | **found** | **found** |
   | Anchored up to line 40, then lines 43 to 48 cut | holds | holds | holds |
   | Signed heads kept on the Mac, deleted with the rewrite | holds | | |

   - The Git remote refused the force push and the branch's deletion. It accepted a new commit that
     replaced every line of the heads file with forged heads, and that changed nothing: the check
     reads every head any commit ever added, so the old heads still contradict the rewritten record.
     What an anchor must refuse is removing a head, not adding one.
   - The ledger refused a head signed by another key (403) and a head older than its last (409). It
     accepted a forged head signed with the Mac's own key, which a program running as you can use
     when the key is not in the Secure Enclave; the old heads still caught the rewrite.
   - The sixth row is the window no anchor covers: lines written since the last head sent. A
     program running as you can cut or rewrite them and stop the next head going out. With a head
     every minute, the window is a minute. The team's host can warn when an awake Mac stops sending.
   - The last row is why a signature alone is not an anchor: heads signed on the Mac and kept there
     are deleted along with the rewrite.

2. **A Git anchor grows by about 1 KB a head.** A signed head is 257 bytes. A day of heads, one a
   minute, each its own commit, took 1.37 MB in the repository after `git gc`, and reading all
   1,440 back took 0.85 s ([`results/cost.json`](results/cost.json)). That is about 160 MB a year
   for eight hours of recording a day, and 500 MB for a Mac that records all day. A head every
   ten minutes divides it by ten and makes the window ten minutes; a new repository each year
   keeps any one small. The ledger keeps the 257 bytes.

3. **On a Mac, the writer as built makes nothing durable.** #386's `format.md` says the flush is
   `F_FULLFSYNC` on a Mac. The prototype calls `std.posix.fdatasync` and `File.sync`, and Zig 0.15.2
   implements both, and `std.posix.fsync`, as a plain `fsync(2)` on macOS
   (`lib/std/posix.zig`, `fsync` and `fdatasync`; `lib/std/fs/File.zig`, `sync`). Apple's
   `fsync(2)` only hands the data to the drive, which may keep it in its cache and write it out of
   order. Go's `File.Sync` has issued `F_FULLFSYNC` on macOS since Go 1.12, and Rust's `sync_all`
   since 2019 (rust-lang/rust#60121); Zig does not. The writer here now asks for `F_FULLFSYNC`
   itself, falling back to `fsync` on a volume that refuses it (SMB), as Go does. It builds for
   `aarch64-macos`; it has not run on a Mac.

4. **A power cut can leave a batch half on disk that #386's start takes for damage.** The power-cut
   test ([`powercut/`](powercut/)) traces every call the writer makes to the disk, builds the states
   a power cut can leave under four models of the disk, starts the writer on each and checks that
   it starts, that `silo verify` passes and that every acknowledged line is there. 205 cut points,
   20 states each, 4,100 states per row ([`results/powercut.jsonl`](powercut/results/powercut.jsonl)):

   | Disk model | Start rule | States that lost a write | Pass | Writer refuses to start | Verify fails after start | An acknowledged line lost |
   |---|---|---|---|---|---|---|
   | linux | as #386 | 1,949 | 4,100 | 0 | 0 | 0 |
   | mac-fsync | as #386 | 4,076 | 290 | 3,376 | 40 | 394 |
   | mac-full | as #386 | 1,585 | 3,981 | 119 | 0 | 0 |
   | mac-barrier | as #386 | 2,890 | 3,817 | 283 | 0 | 0 |
   | linux | cut_any_bad_frame | 1,978 | 4,100 | 0 | 0 | 0 |
   | mac-fsync | cut_any_bad_frame | 4,077 | 275 | 2,999 | 269 | 557 |
   | mac-full | cut_any_bad_frame | 1,557 | 4,018 | 82 | 0 | 0 |
   | mac-barrier | cut_any_bad_frame | 2,865 | 3,834 | 266 | 0 | 0 |
   | linux | after_head | 1,983 | 4,100 | 0 | 0 | 0 |
   | mac-fsync | after_head | 4,071 | 299 | 2,563 | 340 | 898 |
   | mac-full | after_head | 1,583 | 4,100 | 0 | 0 | 0 |
   | mac-barrier | after_head | 2,916 | 4,100 | 0 | 0 | 0 |

   - **linux**: ext4 ordered, `fdatasync` and `fsync` durable before they return. Every state
     passes, as #386 found by killing the process.
   - **mac-fsync**: what #386's writer does on a Mac (finding 3). Acknowledged lines are lost, and
     the record often cannot start again.
   - **mac-full**: every flush `F_FULLFSYNC`. No acknowledged line is lost, but the writer refuses
     to start in some states: the batch being written when the power went is on disk in part, with
     zeros where a sector did not arrive, or a later sector without an earlier one. #386's start
     cuts only a last frame cut short or one ending at the end of the file; this one has zeros or
     other frames after it, so the start calls it damage. The planted bug `cut_any_bad_frame` does
     not fix it either: zeros read as a frame of length 0 whose CRC holds, and the line in it does
     not parse.
   - **mac-barrier**: `F_FULLFSYNC` only on the flush before an acknowledgement, `F_BARRIERFSYNC`
     on every other. No acknowledged line is lost; the same refusals as mac-full.
   - **after_head**, a new start rule: in the last segment, a bad frame or a line that does not
     follow, after the line the head on disk names, is a write that was never acknowledged. The
     start cuts it, keeps the bytes beside the segment and writes a fact, as #386 does for a torn
     last frame. Before the head, it is damage, as in #386. Under mac-full and mac-barrier every
     state passes. What it gives up: a line changed after the head is cut and kept, with a fact,
     instead of stopping the start; an anchor that received that line's head still names it. The
     head should then be written as often as it is sent off the Mac, not every hour. #386's crash test and tamper test give
     the same results under it ([`../2026-11-25-record-format/proto/results/`](../2026-11-25-record-format/proto/results/)).

## What it changes in the plan

| Entry | State | What this adds |
|---|---|---|
| `record-head-off-the-mac` | **new, open** | Where copies of the head go: a Git repository with a protected branch for one person, the team's host for a team; `silo verify` checks every head the anchor ever received |
| `record-damaged-at-start` | open | Notes: a power cut leaves damage-like tails; the after-head rule tells them apart from damage, and the recommended "keep recording" would otherwise flag every power cut |
| `record-file-format` | taken by agents | Notes: the flush must be `F_FULLFSYNC` on a Mac, which Zig's standard library does not do |
| `record-edits-show` (#362) | open in #362 | Not on main, so the note is here: the "place the user chooses" must keep every head it receives, not only the last, or a rewrite replaces it |

## For polarzero to rule

- **`record-head-off-the-mac`**, new: recommended, a Git repository with a protected branch for one
  person and the team's host for a team, a head every minute.
- **`record-damaged-at-start`**: the after-head rule beside the recommended option, so that a
  power cut does not read as damage.

## The options for the anchor

| Anchor | Can a program running as you remove a head? | Who reads it | What leaves the Mac | Cost | Verdict |
|---|---|---|---|---|---|
| Git repository, branch protected by a ruleset | No, with a token that may only push contents. Yes, with your GitHub owner rights | You, or anyone you give the repository | Head hashes and times | A repository and a token | Recommended for one person |
| The team's host (#362) | No: the host is someone else's | The lead, an auditor | The same | A host, which a team has | Recommended for a team |
| Public transparency log: Sigstore Rekor, Sigsum | No | Anyone | The same, public and linked by your key | A public record of when you ran agents; Rekor v2 has no search and Sigstore keeps v1 as the default; Sigsum asks for a domain to limit rate | Later, for records shown outside a team |
| Secure Enclave signature, heads kept on the Mac | Yes: deleting them is enough | | Nothing | | Not an anchor; the key is for signing what goes to one |
| iCloud Drive | Yes: the folder is yours to write, and a deletion syncs | | | | Rejected |
| CloudKit, Silo's own container | Silo alone writes it, but you can delete an app's iCloud data in Settings, which removes every head | | | A provisioning profile, as for the enclave | Not studied further |
| OpenTimestamps | Yes: the proof is kept on the Mac | Anyone with the proof | A hash, to a public calendar | Free, hours to reach Bitcoin | Rejected alone: it proves a head existed, not that a later one did not |
| A witness that expects heads (the team's host, a phone) | | | | | Covers a head that stops coming; part of the team's host |

The Secure Enclave key: Apple lets only an app with a provisioning profile hold a Secure Enclave key
through the keychain, not a bare command-line tool (Apple developer forums, DTS answers in threads
129563, 133390, 786171). So the enclave key depends on Silo shipping as a signed app. It matters for
an anchor that refuses heads from anyone else (the ledger, a log), not for catching a rewrite.

## What was not verified

- **The Mac.** Every model of a Mac here is a model: what APFS and Apple's drives actually keep
  through a power cut was not measured, nor what `F_FULLFSYNC` and `F_BARRIERFSYNC` cost (#359 test
  06 measures the flush). Whether APFS can leave zeros where a sector did not arrive is not known;
  the mac-full and mac-barrier models allow it.
- **The power-cut models are sampled**, 20 states per cut point, not every state. Sectors are 512
  bytes; a drive with 4 KiB atomic writes tears less.
- **GitHub.** The protected branch is a bare repository with `receive.denyNonFastForwards` and
  `receive.denyDeletes`, standing in for a ruleset. That a fine-grained token with contents write
  cannot change a ruleset is from GitHub's documentation of the permissions, not tested.
- **The Secure Enclave** is a P-256 key in a file. Nothing here ran on a Mac or used a keychain.
- **Rekor and Sigsum** were read about, not used. Sigstore's posts disagree on whether Rekor v2 is
  the public default; this report goes by the later one, "Rekor evolution".

## Run it

```sh
cd ../2026-11-25-record-format/proto && zig build --release=safe && cd -
B=../2026-11-25-record-format/proto/zig-out/bin/silorec
python3 anchor/test_anchor.py $B /tmp/anchor            # the anchor table
python3 anchor/cost.py /tmp/cost 1440                   # a day of heads in a Git anchor
python3 powercut/powercut.py $B /tmp/powercut 20 1      # the power-cut table; needs strace
SILOREC_START_RULE=after_head $B crash /tmp/c           # #386's tests under the after-head rule
SILOREC_START_RULE=after_head $B tamper /tmp/t
```

| File | Holds |
|---|---|
| `anchor/anchor.py` | Heads signed and sent to a Git anchor or a ledger; the check of a record against them |
| `anchor/recfmt.py` | Enough of the format to rewrite a record as an attacker would |
| `anchor/test_anchor.py`, `anchor/cost.py` | The tamper cases and the cost |
| `powercut/powercut.py` | The power-cut test |
| `../2026-11-25-record-format/proto/src/writer.zig` | Changed here: `F_FULLFSYNC` on a Mac, and the after-head start rule (default off) |
| `../2026-11-25-record-format/proto/src/main.zig` | Changed here: the commands `workload` and `start`, and `SILOREC_START_RULE` |
