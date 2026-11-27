"""Recall and precision per rule on the synthetic corpus. Run: python3 eval_corpus.py corpus.jsonl"""
import collections, json, sys
from detector import scan

tp = collections.Counter(); fn = collections.Counter(); fp = collections.Counter()
near = collections.Counter(); ent = collections.Counter()
for line in open(sys.argv[1]):
    s = json.loads(line)
    hits = [h for h in scan(s["text"], entropy_mode="keyword")]
    rules = {h["rule"] for h in hits if h["grade"] in ("checked", "prefix", "shape")}
    e = any(h["grade"] == "entropy" for h in hits)
    if s["expect"]:
        if s["expect"] in rules:
            tp[s["expect"]] += 1
        else:
            fn[s["expect"]] += 1
            if e:
                ent[s["expect"]] += 1
        for r in rules - {s["expect"]}:
            fp[r] += 1
    else:
        for r in rules:
            fp[r] += 1
        near[(s["kind"], "rule" if rules else ("entropy" if e else "nothing"))] += 1
print(f"{'rule':28} {'found':>6} {'missed':>6} {'missed, entropy caught':>23} {'wrong':>6}")
for r in sorted(set(tp) | set(fn) | set(fp)):
    print(f"{r:28} {tp[r]:6} {fn[r]:6} {ent[r]:23} {fp[r]:6}")
print("\nnear-misses and placeholders: what fired")
for (k, what), n in sorted(near.items()):
    print(f"  {k:40} {what:8} {n}")
