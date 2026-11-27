"""Sums up one night of run/night.sh. usage: summary.py <out> [label]; prints one JSON object.

Latency per kind of operation, from the programs' side; the processes' peaks from the samples; the
record's growth; what the boundary counted against what the stand-in provider billed; the
precaution's holds; the source's waits and refusals for room in the record."""
import json, os, statistics, sys, collections

O = sys.argv[1]; LABEL = sys.argv[2] if len(sys.argv) > 2 else os.path.basename(O)
def lines(f):
    try: return [json.loads(l) for l in open(os.path.join(O, f)) if l.strip()]
    except FileNotFoundError: return []
def pct(xs, p): return round(sorted(xs)[min(len(xs) - 1, int(len(xs) * p / 100))], 1) if xs else None
ops = lines("ops-c1.jsonl") + lines("ops-c2.jsonl")
marks = {k: v for m in lines("marks.jsonl") for k, v in m.items()}
dur = marks.get("end", 0) - marks.get("start", 0)
by = collections.defaultdict(list)
for o in ops: by[o["k"]].append(o)
kinds = {}
for k, xs in sorted(by.items()):
    ok = [x for x in xs if 200 <= x["status"] < 300]
    st = collections.Counter(x["status"] for x in xs)
    d = {"n": len(xs), "status": dict(sorted(st.items())), "p50_ms": pct([x["ms"] for x in ok], 50), "p90_ms": pct([x["ms"] for x in ok], 90), "p99_ms": pct([x["ms"] for x in ok], 99), "max_ms": pct([x["ms"] for x in ok], 100)}
    if k == "model": d.update(ttfb_p50=pct([x["ttfb"] for x in ok], 50), ttfb_p99=pct([x["ttfb"] for x in ok], 99))
    kinds[k] = d
# During a stall of the recorder, and outside it.
stall = None
if "stall" in marks:
    s0, s1 = marks["stall"], marks["resume"]
    inn = [o for o in ops if s0 <= o["t"] < s1]
    stall = {"seconds": round(s1 - s0, 1), "ops": len(inn), "ok": sum(1 for o in inn if 200 <= o["status"] < 300),
             "refused_403": sum(1 for o in inn if o["status"] == 403), "failed_0": sum(1 for o in inn if o["status"] == 0),
             "max_ms": max([o["ms"] for o in inn], default=None),
             "by_kind": {k: dict(collections.Counter(o["status"] for o in inn if o["k"] == k)) for k in sorted({o["k"] for o in inn})}}
samples = lines("samples.jsonl")
def peak(proc, key): return max([s[proc][key] for s in samples if s.get(proc)], default=None)
def cpu(proc):
    xs = [s for s in samples if s.get(proc)]
    return round((xs[-1][proc]["cpu_s"] - xs[0][proc]["cpu_s"]) / (xs[-1]["t"] - xs[0]["t"]) * 100, 1) if len(xs) > 1 else None
trace = lines("trace.jsonl")
calls = [t for t in trace if t.get("kind") == "call"]
counted = sum(t.get("cost", 0) for t in calls)
# The stand-in's bill against the boundary's count, in tokens of each kind.
billed_calls = [l for l in lines("ledger.jsonl") if "input" in l and "provider" in l]
KEYS = ("input", "cache_read", "cache_write_short", "cache_write_long", "output")
billed_tok = {k: sum(l.get(k, 0) for l in billed_calls) for k in KEYS}
counted_tok = {k: sum(t.get(k, 0) for t in calls) for k in KEYS}
src = [t for t in trace if t.get("kind") == "source"]
last_src = {}
for t in src: last_src[t["source"]] = t
held = [t for t in lines("precaution.jsonl")]
rec_bytes = [s["record_bytes"] for s in samples]
out = {
    "label": LABEL, "seconds": round(dur, 1), "ops": len(ops), "ops_per_s": round(len(ops) / dur, 1) if dur else None,
    "kinds": kinds, "stall": stall,
    "boundary": {"rss_peak_mb": peak("boundary", "rss_mb"), "hwm_mb": peak("boundary", "hwm_mb"), "threads_peak": peak("boundary", "threads"), "fds_peak": peak("boundary", "fds"), "cpu_pct": cpu("boundary")},
    "recorder": {"rss_peak_mb": peak("recorder", "rss_mb"), "cpu_pct": cpu("recorder")},
    "precaution": {"rss_peak_mb": peak("precaution", "rss_mb"), "cpu_pct": cpu("precaution"), "asked": len(held),
                   "verdicts": dict(collections.Counter(h["verdict"] for h in held)), "detector_p50_ms": pct([h["detector_ms"] for h in held], 50), "detector_p99_ms": pct([h["detector_ms"] for h in held], 99)},
    "record": {"bytes": rec_bytes[-1] if rec_bytes else None, "bytes_per_s": round(rec_bytes[-1] / dur) if rec_bytes and dur else None,
               "sources": {k: {x: v[x] for x in ("acked", "last", "dropped", "decisions_waited", "decisions_refused")} for k, v in last_src.items()}},
    "model_calls": {"served": len(billed_calls), "counted": len(calls), "counted_usd": round(counted, 4), "billed_tokens": billed_tok, "counted_tokens": counted_tok},
}
print(json.dumps(out))
