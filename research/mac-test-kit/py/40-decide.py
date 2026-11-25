"""Test 40's outcome from its runs (bench.jsonl): one line per measure (M), fact (F) and the
outcome (O), tab-separated, as test 31's py/31-ckpt.py."""
import json, sys

LIMIT_MS, RATE = 50.0, 1000.0
runs = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
rec = [r for r in runs if not r.get("sqlite")]
sql = [r for r in runs if r.get("sqlite")]


def one(load, mode, per, blob):
    for r in rec:
        if (r["load"], r["flush"], r["per_flush"], r["blob_every"]) == (load, mode, per, blob):
            return r
    return None


def out(*f):
    print("\t".join(str(x) for x in f))


for r in rec:
    tag = "%s_%s_per%d_blob%d" % (r["load"], r["flush"], r["per_flush"], r["blob_every"])
    out("M", tag + "_ack_p99_ms", r["ack_p99_ms"], "ms", "wait before lines are acknowledged, 99th percentile")
    out("M", tag + "_lines_per_s", r["lines_per_s"], "lines/s", "")
    if r["blob_every"]:
        out("M", tag + "_blob_p99_ms", r["blob_p99_ms"], "ms", "a new blob made durable")
for r in sql:
    out("M", "sqlite_%s_fullfsync%d_commit_p99_ms" % (r["load"], r["fullfsync"]), r["p99_ms"], "ms", "one commit, WAL, synchronous FULL, SQLite " + r["version"])

need = [one("busy", m, p, b) for m, p, b in (("full", 1, 0), ("full", 1, 5), ("barrier", 1, 5), ("full", 16, 0))]
if any(n is None for n in need):
    out("O", "inconclusive", "a run is missing from bench.jsonl")
    sys.exit()
full1, fullblob, barblob, full16 = need
# A line with a new blob waits for the blob, then for the line's flush.
blob_wait = lambda r: r["blob_p99_ms"] + r["ack_p99_ms"]
s = ("busy: F_FULLFSYNC per line p99 %.1f ms (%.0f lines/s); with a new blob every 5 lines %.1f ms per blob, "
     "%.1f ms under the barrier rule; grouped by 16 %.0f lines/s"
     % (full1["ack_p99_ms"], full1["lines_per_s"], fullblob["blob_p99_ms"], barblob["blob_p99_ms"], full16["lines_per_s"]))
if full1["ack_p99_ms"] <= LIMIT_MS and blob_wait(fullblob) <= LIMIT_MS:
    out("O", "full-affordable", s)
elif full1["ack_p99_ms"] <= LIMIT_MS and blob_wait(barblob) <= LIMIT_MS:
    out("O", "barrier-for-blobs", s)
elif full16["lines_per_s"] >= RATE:
    out("O", "group-lines", s)
else:
    out("O", "too-slow", s)
