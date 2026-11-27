"""How often a program's traffic-shape markers changed between releases, from sweep.sh's output.

usage: python3 -I changes.py <markers/program dir> [--events]
Prints, per category, how many releases added or removed a marker, and with --events each change
with its release and date. Markers between two consecutive releases only: a release whose package
was not on npm is skipped, and the next one carries its changes.
"""
import json, os, sys

D = sys.argv[1]; EV = "--events" in sys.argv
vs = [l.split() for l in open(os.path.join(D, "versions.txt"))]
vs = [(v, t) for v, t in vs if os.path.exists(os.path.join(D, v + ".json"))]
prev, per_cat, any_change, events = None, {}, 0, []
for v, t in vs:
    cur = json.load(open(os.path.join(D, v + ".json")))
    if prev:
        changed = False
        for k in sorted(set(cur) | set(prev[1])):
            new = sorted(set(cur.get(k, [])) - set(prev[1].get(k, [])))
            gone = sorted(set(prev[1].get(k, [])) - set(cur.get(k, [])))
            if new or gone:
                changed = True; per_cat[k] = per_cat.get(k, 0) + 1
                events.append((v, t, k, new, gone))
        any_change += changed
    prev = (v, cur)
print(f"{os.path.basename(D.rstrip('/'))}: {len(vs)} releases read, {vs[0][0]} ({vs[0][1]}) to {vs[-1][0]} ({vs[-1][1]})")
print(f"  {any_change} of {len(vs)-1} releases changed at least one marker")
for k, n in sorted(per_cat.items(), key=lambda x: -x[1]):
    print(f"  {k}: changed in {n} releases")
if EV:
    for v, t, k, new, gone in events:
        print(f"{v} {t} {k}" + (f" +{new}" if new else "") + (f" -{gone}" if gone else ""))
