"""Prints the overshoot table of overshoot.sh's runs: one line per policy, over the seeds."""
import glob, json, subprocess, sys, statistics as st, collections
model = sys.argv[1]; limit = float(sys.argv[2])
rows = collections.defaultdict(list)
for d in sorted(glob.glob(f"out/over-{model}-*")):
    pol = d.split(model + "-")[1].rsplit("-", 1)[0]
    r = json.loads(subprocess.check_output([sys.executable, "run/compare.py", d, "--json"]))
    dec = r.get("decompose_main", {})
    rows[pol].append((r["billed"], r["calls_billed"], dec.get("late_admissions", 0) or 0, dec.get("in_flight_at_limit", 0) or 0))
order = ["finish", "finish-time", "admit-input", "reserve-typical", "reserve-max", "cut", "typical-cut"]
print(f"| Policy | Spent, each run | Over the ${limit:g} budget, worst | From calls that left after the limit was reached, worst | Calls done, mean |")
print("|---|---|---|---|---|")
for p in order:
    v = rows.get(p)
    if not v: continue
    spent = ", ".join(f"{b:.2f}" for b, *_ in v)
    worst = max(b for b, *_ in v) - limit
    late = max(l for _, _, l, _ in v)
    print(f"| `{p}` | {spent} | {worst:+.2f} ({100*worst/limit:+.0f}%) | {late:.2f} | {st.mean(c for _, c, _, _ in v):.0f} |")
