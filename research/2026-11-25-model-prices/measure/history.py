"""Price events of the first-party rows of LiteLLM's table, grouped by commit, with labels.

usage: history.py <events.json from walk_litellm.py> <labels.json>
"""
import json, sys, collections, datetime, statistics

ev = json.load(open(sys.argv[1]))["events"]
labels = json.load(open(sys.argv[2]))
since = {}
groups = collections.OrderedDict()
for e in ev:
    k = e["model"]
    if e["kind"] == "new": since[(k, None)] = e["day"]; continue
    if e["kind"] != "change": continue
    start = since.get((k, e["field"])) or since.get((k, None))
    since[(k, e["field"])] = e["day"]
    g = groups.setdefault(e["commit"][:9], {"day": e["day"], "providers": set(), "models": set(), "age": [], "ratio": []})
    g["providers"].add(e["provider"]); g["models"].add(k)
    g["age"].append((datetime.date.fromisoformat(e["day"]) - datetime.date.fromisoformat(start)).days)
    if e["old"] and e["new"]: g["ratio"].append(max(e["old"], e["new"]) / min(e["old"], e["new"]))
by = collections.Counter(); ages = []; ratios = []
print(f"{'day':10}  {'label':10}  {'models':>6}  {'age d':>5}  {'x':>5}  what")
for c, g in groups.items():
    lab, what = labels.get(c, ["?", "not labelled"])
    by[(lab, g["day"][:4])] += 1
    age = max(g["age"]); r = max(g["ratio"]) if g["ratio"] else 0
    if lab == "correction": ages.append(age); ratios.append(r)
    print(f"{g['day']}  {lab:10}  {len(g['models']):>6}  {age:>5}  {r:>5.1f}  {what}")
print()
for y in ("2023", "2024", "2025", "2026"):
    print(f"{y}: {by[('provider', y)]} provider changes, {by[('correction', y)]} corrections")
print(f"corrections: {len(ages)}; age of the wrong value, median {statistics.median(ages)} days, 75th percentile {sorted(ages)[int(len(ages)*.75)]}, max {max(ages)}; "
      f"size of the error, median {statistics.median(ratios):.1f}x, max {max(ratios):.0f}x")
