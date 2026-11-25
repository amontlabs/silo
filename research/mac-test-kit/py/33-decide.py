"""Test 33's outcome from the allocated sizes of its disk images: prints "key|sentence".

  33-decide.py CSV WRITTEN_BYTES
      CSV: target,stage,alloc_bytes. Targets: main (the computer's own disk image), raw (a raw
      extra disk), asif (an ASIF extra disk). Stages, in order: before, written, deleted (rm, no
      sync, 5 s), settled (sync, 60 s), remount (the volume unmounted and mounted again in the
      guest; extra disks only), restart (the computer stopped and started again).
"""
import csv
import sys

STAGES = ["before", "written", "deleted", "settled", "remount", "restart"]
NAMES = {"main": "the main raw image", "raw": "a raw extra disk", "asif": "an ASIF extra disk"}

rows = list(csv.DictReader(open(sys.argv[1])))
written = float(sys.argv[2])
a = {}
for r in rows:
    if r.get("alloc_bytes"):
        a.setdefault(r["target"], {})[r["stage"]] = float(r["alloc_bytes"])

freed = {}  # target -> {stage: fraction of what the write added that is gone}
parts = []
grew_ok = True
for t in ("main", "raw", "asif"):
    v = a.get(t)
    if not v or "before" not in v or "written" not in v:
        continue
    grew = v["written"] - v["before"]
    if grew < 0.5 * written:
        grew_ok = False
        parts.append("%s grew only %.2f GB for %.2f GB written" % (t, grew / 1e9, written / 1e9))
        continue
    freed[t] = {s: (v["written"] - v[s]) / grew for s in STAGES[2:] if s in v}
    parts.append("%s: grew %.2f GB, then freed %s" % (t, grew / 1e9, ", ".join(
        "%d%% %s" % (round(100 * freed[t][s]), s) for s in STAGES[2:] if s in freed[t])))
s = "; ".join(parts)
best = lambda t, stages: max([freed.get(t, {}).get(x, 0) for x in stages] or [0])
early = ["deleted", "settled"]
late = ["remount", "restart"]
if "main" not in freed and "raw" not in freed:
    print("inconclusive|" + (s or "no image was measured")); sys.exit()
raw_early = max(best("main", early), best("raw", early))
raw_any = max(best("main", early + late), best("raw", early + late))
asif_any = best("asif", early + late)
if raw_early >= 0.75:
    key = "trim-reaches"
elif raw_any >= 0.75:
    key = "trim-at-mount"
elif asif_any >= 0.75:
    key = "asif-only"
elif max(raw_any, asif_any) < 0.25:
    key = "no-trim"
else:
    key = "partial"
if not grew_ok and key in ("no-trim", "partial"):
    key = "inconclusive"
print(key + "|" + s)
