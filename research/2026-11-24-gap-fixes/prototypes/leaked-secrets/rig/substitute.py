"""Would swapping a found credential for a placeholder in every model request cost prompt-cache hits?
Replays a run's requests, swaps each value the exact rules find for one placeholder per value, and
counts, for each request, whether the earlier request's messages are still a byte-identical prefix.

Run: python3 substitute.py <out of run_cc.sh>
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from detector import scan_fast, make_placeholder


def strip(o):
    if isinstance(o, dict):
        return {k: strip(v) for k, v in o.items() if k != "cache_control"}
    if isinstance(o, list):
        return [strip(x) for x in o]
    return o


reqs = [json.loads(l)["body"] for l in open(f"{sys.argv[1]}/requests.jsonl")]
reqs = [r for r in reqs if "messages" in r]
swap = {}
for r in reqs:  # the boundary would learn values in this order, one new message at a time
    raw = json.dumps(r["messages"])
    for h in scan_fast(raw):
        if h["grade"] in ("checked", "prefix", "shape"):
            v = raw[h["start"]:h["end"]]
            if h["rule"] == "database-url":
                continue  # the password inside, not the whole URL, would be swapped; skipped here
            swap.setdefault(v, make_placeholder(v[:4], len(v) - 4, h["fp"]))


def sub(s):
    for v, p in swap.items():
        s = s.replace(v, p)
    return s


def prefix_ok(prev, cur):
    a = [json.dumps(strip(m)) for m in prev["messages"]]
    b = [json.dumps(strip(m)) for m in cur["messages"]]
    return b[:len(a) - 1] == a[:-1]  # all but the last message of prev (Claude Code rewrites it)


orig = reqs
new = [json.loads(sub(json.dumps(r))) for r in reqs]
o = sum(prefix_ok(orig[i - 1], orig[i]) for i in range(1, len(orig)))
n = sum(prefix_ok(new[i - 1], new[i]) for i in range(1, len(new)))
left = sum(v in json.dumps(r) for r in new for v in swap)
print(f"{len(swap)} values swapped; prefix kept between consecutive requests: original {o}/{len(orig)-1}, swapped {n}/{len(new)-1}; values left in swapped bodies: {left}")
