"""Test 39's outcome: what the guest read of its own files against what the reader read from
outside. Prints "key|sentence" on the first line, then one JSON object of facts.

  39-compare.py GUEST_SHA READER_JSONL [COMPRESSED_LIST]
      GUEST_SHA: `shasum -a 256` lines, as the guest printed them (sha, two spaces, path)
      READER_JSONL: apfscontent.py ls lines (path, size, compressed, sha256 or gap)
      COMPRESSED_LIST: optional, the paths the guest saw flagged compressed (one per line)
"""
import json
import sys

guest = {}
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    line = line.rstrip("\n")
    if len(line) > 66 and line[64:66] == "  ":
        guest[line[66:]] = line[:64]
reader = {}
for line in open(sys.argv[2]):
    try:
        r = json.loads(line)
    except ValueError:
        continue
    if "path" in r:
        reader[r["path"]] = r
flagged = set()
if len(sys.argv) > 3:
    flagged = {p.rstrip("\n") for p in open(sys.argv[3], encoding="utf-8", errors="replace") if p.strip()}

by_alg = {}
same = wrong = gap = missing = 0
wrong_ex, gap_ex, missing_ex = [], [], []
unread_types = set()
for path, sha in sorted(guest.items()):
    r = reader.get(path)
    if r is None:
        missing += 1
        missing_ex.append(path)
        continue
    alg = r.get("compressed") or "none"
    c = by_alg.setdefault(alg, {"files": 0, "same": 0, "wrong": 0, "gap": 0})
    c["files"] += 1
    if "gap" in r:
        gap += 1
        c["gap"] += 1
        gap_ex.append("%s (%s): %s" % (path, alg, r["gap"]))
        if alg != "none":
            unread_types.add(alg)
    elif r.get("sha256") == sha:
        same += 1
        c["same"] += 1
    else:
        wrong += 1
        c["wrong"] += 1
        wrong_ex.append("%s (%s)" % (path, alg))
compressed = sum(v["files"] for k, v in by_alg.items() if k != "none")
not_seen_compressed = sorted(p for p in flagged if p in reader and not reader[p].get("compressed"))
facts = {"files": len(guest), "same": same, "wrong": wrong, "gap": gap, "missing": missing,
         "by_algorithm": by_alg, "wrong_examples": wrong_ex[:5], "gap_examples": gap_ex[:5],
         "missing_examples": missing_ex[:5], "guest_flagged_compressed": len(flagged),
         "flagged_but_read_as_plain": not_seen_compressed[:5]}
summary = "%d files: %d read the same as the guest, %d differ, %d gaps, %d not found; compressed: %s" % (
    len(guest), same, wrong, gap, missing,
    ", ".join("%s %d" % (k, v["files"]) for k, v in sorted(by_alg.items()) if k != "none") or "none")
if not guest or len(reader) == 0:
    key = "inconclusive"
    summary = "nothing to compare (guest list %d files, reader %d)" % (len(guest), len(reader))
elif wrong or not_seen_compressed:
    key = "reads-wrong"
elif missing > len(guest) // 10:
    key = "inconclusive"
    summary += "; the reader did not find %d of the guest's files" % missing
elif compressed == 0:
    key = "inconclusive"
    summary += "; no compressed file to read"
elif unread_types:
    key = "type-unread"
    summary += "; no decoder for: %s" % ", ".join(sorted(unread_types))
elif gap:
    key = "reads-wrong"
    summary += "; gaps on files that are not compressed"
else:
    key = "reads-all"
print(key + "|" + summary)
print(json.dumps(facts))
