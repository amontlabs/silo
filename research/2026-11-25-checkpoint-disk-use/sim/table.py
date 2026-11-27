#!/usr/bin/env python3
"""Reach on the last morning, per option and case, from results/stretch*.jsonl."""
import glob, json
rows = [json.loads(l) for f in sorted(glob.glob("../results/stretch*.jsonl")) for l in open(f)]
opts = list(json.load(open("options.json")))
cases = ["light/mac-512", "medium/mac-512", "heavy/mac-512", "macos-no-trim/mac-512", "medium/mac-1tb"]
R = {}
for r in rows:
    w, h, _, o = r["name"].split("/")
    R[(f"{w}/{h}", o)] = r
print("| Option | " + " | ".join(cases) + " |")
print("|---|" + "---|" * len(cases))
for o in opts:
    cells = []
    for c in cases:
        r = R.get((c, o))
        if not r: cells.append(""); continue
        g = r["gap_h_last"]; holes = "" if r["nights_whole_at_morning"] == r["mornings"] else " *"
        cells.append(f"{r['reach_last_morning_d']}{holes}")
    print(f"| `{o}` | " + " | ".join(cells) + " |")
