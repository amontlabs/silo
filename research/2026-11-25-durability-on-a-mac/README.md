# Durability on a Mac's terms

Research of 25 November 2026, by agents, for polarzero. #394 found that Zig's `fsync`,
`fdatasync` and `File.sync` are a plain `fsync(2)` on macOS, which leaves data in the drive's cache,
and fixed the record's writer. This audits every other prototype that keeps state, fixes each gap
after a test shows it failing, and adds two Mac kit tests that measure what the fixes cost and
check them through a power cut. Nothing is built and no entry is taken by it.

The branch is stacked on #391 by merge (#391's second pass, which holds #390 and #392 to #396);
`plan check` passes.

## What was found

1. **The spend budget forgot everything at a restart.** #374's boundary keeps its ledgers in memory.
   [`spend/restart.sh`](spend/restart.sh) runs #374's stand-in provider with a $1 budget:

   | Case | Without the journal | With it |
   |---|---|---|
   | 23 calls reach the budget, the boundary is killed (SIGKILL) and started again, one more call | admitted | refused, $1.01 spent |
   | The boundary is killed while a long call streams, started again, one more call | admitted | refused, $1.29 counted |

   ([`results/restart-before.jsonl`](results/restart-before.jsonl),
   [`results/restart-after.jsonl`](results/restart-after.jsonl).) A crash, a reboot or a power cut
   therefore broke `budget-stops-what-it-meters`. The fix is a spend journal
   (`2026-11-24-spend-budgets/boundary/src/journal.zig`, set by `"journal"` in the config): a line
   for each call that leaves and each settlement, flushed (`F_FULLFSYNC` on a Mac) before the call
   goes on. At start every ledger gets its spend back; a call that left and never settled counts at
   its reserve, the most it can cost. A line cut by a power cut at the end of the file is dropped; a
   bad line followed by a good one stops the start as damage. Unit-tested, and both boundaries
   (#374's and #395's) use it. It costs two flushes per model call.

2. **The price table's rollback guard could be read empty.** The boundary notes the version it last
   took in `<table>.seen` and refuses an older signed table. The file was written in place; a test
   reading it while it was rewritten found it empty or cut **9,668 times in 3,000 writes**. A start
   after a power cut at that moment finds no version and takes an older signed table, with lower
   prices. It is now written aside, flushed and renamed; the same test finds no torn read.

3. **The team ledger's commits were plain `fsync` on a Mac.** SQLite with `synchronous=FULL` asks
   for `F_FULLFSYNC` only with `PRAGMA fullfsync` and `checkpoint_fullfsync`, both off by default.
   [`ledger/test_sqlite_flush.py`](ledger/test_sqlite_flush.py) failed on #388's ledger and passes
   with both set; #388's own tests still pass (central, and the ledger killed 25 times).

4. **The anchor ledger of #394 used Python's `os.fsync`**, plain `fsync` on a Mac too.
   [`anchor/test_ledger_flush.py`](anchor/test_ledger_flush.py) gives Python's `fcntl` an
   `F_FULLFSYNC` on Linux and records each flush: three heads answered 201 after a plain `fsync`
   before, after `F_FULLFSYNC` now. #394's anchor test gives the same table.

5. **#395 did not build once #385 was in.** #385 added the models a request asked for to
   `spend.costOf`; #395's boundary still called it with four arguments. Fixed on this branch.

6. **The record's writer can now be measured on a Mac.** `SILOREC_FLUSH=full|barrier|fsync` picks
   the flush (`barrier` is #394's mac-barrier model: `F_FULLFSYNC` before an acknowledgement,
   `F_BARRIERFSYNC` otherwise), and `silorec bench` times each acknowledgement and each new blob.
   A new blob is three flushes (the file and two folders) before its line can be acknowledged. On
   this Linux container, while another program wrote, a line took 0.6 ms at the 99th percentile and
   a new blob 117 ms: the folder flushes wait for the other program's data. Whether a Mac does the
   same is test 40's question.

### What needed nothing, or only a note

| Prototype | What it keeps | State |
|---|---|---|
| Checkpoint catalog (#365) | Catalog, checkpoints, the swap | Already `F_FULLFSYNC` on a Mac. Two notes: it fails rather than fall back to `fsync` on a volume that refuses `F_FULLFSYNC` (SMB, some external disks), and its `writeFile` ignores a short `pwrite` |
| Checkpoint put-back (#365) | Files restored into a computer | Written aside and renamed, never flushed. A power cut after a put-back can leave the old file; the put-back can be redone from the checkpoint, so nothing is lost |
| The boundary's own log (`RECORD`, #374) | One JSON line per event, for the reports | Never flushed. It is a research log, not the record; the record goes through #395's recorder and #386's writer |
| Git leases (#388), Git anchor (#394) | Commits pushed to a remote | What counts is on the remote; the Mac's clone can be lost |
| Morning review (`record.py`, #371) | A report written aside and renamed | Not flushed; a report can be made again |
| Mac kit (`nbdserve.py`, C probes) | Test images | Already `F_FULLFSYNC` |

## The Mac kit: tests 40 and 41

- **Test 40** (`long`, about 15 minutes): the record's writer on APFS in each flush mode, one line
  per flush and 16, with a new blob every 5 lines, quiet and while another program writes, and a
  SQLite commit with and without `fullfsync`. Outcomes: `full-affordable`, `barrier-for-blobs`,
  `group-lines`, `too-slow`, with a 50 ms limit at the 99th percentile, as test 06.
- **Test 41** (`vm`, about 40 minutes): a power cut under the writer. The computer's disk is served
  by [`py/cutdisk.py`](../mac-test-kit/py/cutdisk.py), a stand-in drive: a write stays in its cache
  until a flush, and a cut writes a random half of the cache in a random order and loses the rest.
  The computer runs the real APFS and the writer's real flush calls; the acknowledgements stream to
  the Mac over SSH. After each cut the computer starts again from what the drive kept, and the
  record is started and verified. Outcomes: `full-holds-fsync-loses`, `barrier-loses`,
  `fsync-holds`, `full-loses`, `no-start`. The flushes the drive saw per acknowledgement say
  whether `F_BARRIERFSYNC` reaches a virtual disk at all.

Both ran on Linux only, as analogues: test 40 gave `group-lines` there (numbers that say nothing of
a Mac), test 41 checked its parsing on a writer killed with SIGKILL. The stand-in drive has its own
check, with a planted flush that does nothing, which it catches (`linux/test-41.sh`).

## What it changes in the plan

| Entry | State | Note added |
|---|---|---|
| `what-a-budget-promises` | open | A restart forgot the budget; the spend journal keeps it |
| `pooled-budget-ledger` | open | The ledger's SQLite needs `fullfsync` on a Mac |
| `where-prices-come-from` | open | The version last taken is written whole |
| `record-head-off-the-mac` | open | The anchor answers "kept" after `F_FULLFSYNC` |
| `record-file-format` | taken by agents | The flush modes, and what tests 40 and 41 measure |

The kit's `decisions.txt` maps tests 40 and 41 to `record-file-format`, measured.

## For polarzero to rule

- **A call cut off by a restart counts at its reserve** (`what-a-budget-promises`). The provider
  may have billed all of it, part of it or none; the reserve is the most. Counting it at its reserve
  can refuse a call a budget could still afford, counting it at zero can let spend pass the budget.
  Recommended: the reserve, since a budget that a restart raises is not a limit.

## A challenge

Every gap here comes from one thing: each prototype calls its language's flush and trusts it. Go and
Rust give `F_FULLFSYNC` on a Mac; Zig, Python and SQLite's defaults do not. Silo should have one
flush function, `F_FULLFSYNC` with an `fsync` fallback, that every writer calls, and a check that
fails on a direct `fsync`, `fdatasync`, `File.sync` or `os.fsync` in Silo's code outside it. No entry
says so today; it belongs next to `record-file-format`.

## What was not verified

- **The Mac.** Nothing ran on a Mac. Tests 40 and 41 are untested there, most likely the VM steps
  of 41 (a cut under `--disk-nbd`, and a start from the image afterwards).
- **Test 41's drive is a model** of a drive with a volatile cache, not a real power cut: a real
  drive may tear inside a 4 KiB block, or keep a larger share of its cache.
- **The journal's cost** was not measured on a Mac (test 40 measures the same flush in the writer).
  It grows by two lines per call and is never compacted.
- **The spend journal through a power cut** was tested by kills and by a torn tail in a unit test,
  not by #394's power-cut models.

## Run it

```sh
cd ../2026-11-24-spend-budgets/boundary && zig build && cd -
spend/restart.sh /tmp/restart                          # the budget across a restart
python3 -I ledger/test_sqlite_flush.py ../2026-11-25-team-ledger/ledger/server.py
python3 -I anchor/test_ledger_flush.py ../2026-11-25-record-head-off-the-mac/anchor
(cd ../2026-11-24-spend-budgets/boundary && zig build test)   # journal and .seen tests
cd ../mac-test-kit && ./kit run 40 41                  # on a Mac; Linux runs the analogues
```
