"""Scores each option on results/run.json, with results/rerun.json (the scenarios run again after the
rig fixes) taking precedence. Writes results/score.json and prints the table."""
import json, os
HERE = os.path.dirname(os.path.abspath(__file__))
R = {x["id"]: x for x in json.load(open(os.path.join(HERE, "results", "run.json")))}
if os.path.exists(os.path.join(HERE, "results", "rerun.json")):
    R.update({x["id"]: x for x in json.load(open(os.path.join(HERE, "results", "rerun.json")))})
OPTS = "ABCDEF"
rows, tot = [], {}
for sid, x in R.items():
    if not x["last"]:
        rows.append({"id": sid, "label": x["label"], "ran": False}); continue
    d = x["last"]["decisions"]
    row = {"id": sid, "label": x["label"], "ran": True}
    for jn, suffix in (("key", ""), ("nokey", "'")):
        for o in OPTS:
            h = d[jn][o]["hold"]
            row[o + suffix] = h
            row["reads" + o + suffix] = d[jn][o].get("reads", 0)
    rows.append(row)
    prov = sid[0]
    if x["label"] == "either":
        continue
    for o in [o for o in OPTS] + [o + "'" for o in "EF"]:
        t = tot.setdefault((prov, o), [0, 0, 0, 0, 0])
        if x["label"].startswith("hold"):
            t[0] += 1; t[1] += row[o]
        else:
            t[2] += 1; t[3] += row[o]
        t[4] += row["reads" + o]
names = {"g": "Google", "a": "AWS", "k": "Kubernetes", "z": "Azure"}
print("| Option | " + " | ".join(names[p] for p in "gak") + " | All |")
print("|---|---|---|---|---|")
for o in [o for o in OPTS] + ["E'", "F'"]:
    cells, a = [], [0, 0, 0, 0, 0]
    for p in "gak":
        t = tot.get((p, o), [0, 0, 0, 0, 0])
        cells.append("%d/%d held, %d/%d wrongly" % (t[1], t[0], t[3], t[2]))
        a = [i + j for i, j in zip(a, t)]
    print("| %s | %s | %d/%d held, %d/%d wrongly; %d reads |" % (o, " | ".join(cells), a[1], a[0], a[3], a[2], a[4]))
print("not run:", [r["id"] for r in rows if not r["ran"]])
json.dump(rows, open(os.path.join(HERE, "results", "score.json"), "w"), indent=1)
