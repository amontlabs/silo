"""Replays the providers' 14 price changes of 2024 to 2026 against two rules of the channel:
- a cut beyond the floor needs the root key: how often would the owner have signed?
- a frozen table (no fetch since before the change): does it count the change high (a budget
  stops early) or low (a budget lets more through)?
An alias move needs no new price: #374's boundary prices the model the reply names.

usage: replay.py measure/history.json [floor] > results/replay.txt"""
import json, sys
ev = json.load(open(sys.argv[1]))["events"]
floor = float(sys.argv[2]) if len(sys.argv) > 2 else 0.5
root, frozen = [], {"high (safe)": [], "right (dated row)": [], "low (unsafe)": [], "no new price needed": [], "unknown": []}
for e in ev:
    d, f = e["direction"], e["factor"]
    if d.startswith("down") and 1 / f < floor: root.append(e)
    if d == "alias": k = "no new price needed"
    elif e["dated"] is True: k = "right (dated row)"
    elif d.startswith("down"): k = "high (safe)"
    elif d == "up": k = "low (unsafe)"
    else: k = "unknown"
    frozen[k].append(e)
years = 2.2  # September 2024 to November 2026
print(f"floor {floor}: {len(root)} of {len(ev)} changes needed the root key, about {len(root) / years:.1f} a year")
for e in root: print(f"   {e['day']} {e['what']} ({e['factor']}x{'' if e['direction'] == 'down' else ', direction a guess'})")
print("a table frozen before each change counts it:")
for k, v in frozen.items():
    print(f"   {k}: {len(v)}" + "".join(f"\n      {e['day']} {e['what']}" for e in v if k in ('low (unsafe)', 'unknown')))
