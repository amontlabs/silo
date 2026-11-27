#!/usr/bin/env python3
"""The tables of the README, from results/*.jsonl. Usage: tables.py ../results/*.jsonl"""
import json, sys

rows = []
for f in sys.argv[1:]:
    for line in open(f):
        if line.strip():
            rows.append(json.loads(line))
rows.sort(key=lambda r: r["name"])


def fmt(x, nd=1):
    if x is None:
        return "none"
    return f"{x:,.{nd}f}" if isinstance(x, float) else f"{x:,}"


print("| Run | Reach (d) | Least reach after night 10 (d) | Text kept (d) | Recordings kept (d) | "
      "Last morning: checkpoints / lines / text / recordings / bin (GiB) | Most kept (GiB) | "
      "Least free (GiB) | Hours under 20 GiB free | Hours paused (with agents at work) | Hours full | "
      "Removed: checkpoints, recordings GiB, text GiB, bin GiB |")
print("|" + "---|" * 12)
for r in rows:
    L, X = r["last"], r["removed"]
    print(f"| `{r['name']}` | {fmt(r['reach_d'])} | {fmt(r['min_reach_d_after_10'])} | {fmt(r['text_kept_d'])} | "
          f"{fmt(r['rec_kept_d'])} | {L['ckpt']:.0f} / {L['lines']:.2f} / {L['text']:.2f} / {L['rec']:.1f} / "
          f"{L['bin']:.0f} | {r['max_keep']:.0f} | {r['min_free']:.0f} | {r['alert_h']:.0f} | "
          f"{r['paused_h']:.0f} ({r['agent_paused_h']:.0f}) | {r['full_h']:.0f} | "
          f"{X['ckpt'] + X['ckpt24']:,}, {X['rec']:.1f}, {X['text']:.2f}, {X['bin']:.0f} |")
