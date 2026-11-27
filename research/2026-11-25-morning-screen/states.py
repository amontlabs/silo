"""The states of the page that one night does not show, made from its JSON.

    python3 states.py <morning.json> <out folder>

empty     nothing needs you: the night's runs, no alert, no answer that needs you
hundreds  the night's queue and runs repeated over 60 computers (made up: one night, renamed)
"""
import copy, json, os, sys

d = json.load(open(sys.argv[1]))
out = sys.argv[2]
e = copy.deepcopy(d)
e["queue"], e["away"] = [], []
e["runs"] = [r for r in e["runs"] if r["state"] == "ended"]
json.dump(e, open(os.path.join(out, "state-empty.json"), "w"), ensure_ascii=False)
h = copy.deepcopy(d)
q, runs = [], []
for i in range(60):
    comp = f"c{i:02d}"
    for x in d["queue"]:
        y = copy.deepcopy(x)
        y["id"] = f"{comp}:{x['id']}"
        y["computer"] = comp
        q.append(y)
    for r in d["runs"]:
        y = dict(r, computer=comp)
        runs.append(y)
h["queue"], h["runs"], h["away"] = q, runs, d["away"] * 60
h["night"] = dict(d["night"], runs=len(runs), turns=d["night"]["turns"] * 60)
json.dump(h, open(os.path.join(out, "state-hundreds.json"), "w"), ensure_ascii=False)
print(len(q), "rows need you in hundreds")
