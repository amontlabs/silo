#!/usr/bin/env python3
"""Prints the README's tables from ../results/part*.jsonl."""
import glob, json, os
here = os.path.dirname(os.path.abspath(__file__))
rows = {}
for f in sorted(glob.glob(os.path.join(here, "../results/part*.jsonl"))):
    for l in open(f):
        r = json.loads(l); rows[r["name"]] = r

def line(r):
    g = r["gap_h_last"]
    fmt = lambda v: "gone" if v is None else f"{v}"
    return (f"| {r['max_held_gib']} | {r['min_free_gib']} | {r['reach_last_morning_d']} | "
            f"{r['min_reach_d_after_night_10']} | {r['nights_whole_at_morning']}/{r['mornings']} | "
            f"{fmt(g['7'])} / {fmt(g['14'])} / {fmt(g['28'])} | {r['removed_younger_than_reach']} | "
            f"{r['max_removed_in_an_hour']} | {r['alert_floor_minutes']} |")

HEAD = ("| Policy | Most held (GiB) | Least free (GiB) | Reach, last morning (days) | "
        "Least reach after night 10 | Nights whole at 08:00 | Gap at 7 / 14 / 28 days back (h) | "
        "Removed under 30 days by the limit | Most removed in an hour | Minutes under 20 GiB free |\n"
        "|---|---|---|---|---|---|---|---|---|---|")
for host in ("mac-512", "mac-1tb"):
    for work in ("light", "medium", "heavy", "macos-no-trim"):
        print(f"\n#### {work}, {host}\n\n{HEAD}")
        for name, r in rows.items():
            parts = name.split("/")
            if parts[0] == work and parts[1] == host and len(parts) == 3:
                print(f"| `{parts[2]}` " + line(r))
print(f"\n#### Variants\n\n{HEAD}")
for name, r in rows.items():
    if len(name.split("/")) > 3:
        print(f"| `{name}` " + line(r))
