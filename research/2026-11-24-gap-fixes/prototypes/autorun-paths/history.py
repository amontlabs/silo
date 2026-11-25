#!/usr/bin/env python3
"""How often do ordinary commits change files that run on their own? A hold's false-alarm rate.

    python3 -I history.py <repo> [commits=1000] [--data other-list.json]

Walks the first-parent history of HEAD (in a squash-merge repository, one commit per merged pull
request, which is about one push) and runs autorun.py's analysis on each commit against its first
parent. Prints the share of commits with something listed and with something a rule would hold,
by entry, with a few commit subjects for each.
"""
import collections, os, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import autorun

args = sys.argv[1:]
dpath = os.path.join(os.path.dirname(os.path.abspath(__file__)), "autorun-paths.json")
if "--data" in args:
    i = args.index("--data"); dpath = args[i + 1]; del args[i:i + 2]
repo = args[0]
n = int(args[1]) if len(args) > 1 else 1000
data = autorun.load_data(dpath)
log = autorun.git(repo, "log", "--first-parent", f"-{n + 1}", "--format=%H%x09%P%x09%s").splitlines()
rows = [l.split("\t", 2) for l in log]
rows = [(c, p.split()[0], s) for c, p, s in rows if p][:n]
sf = os.path.join(repo, "shallow")  # the cut of a shallow clone: those commits have no parent here
shallow = set(open(sf).read().split()) if os.path.exists(sf) else set()
listed = held = held_runs = 0  # held_runs: holds for files that run on the Mac
by_entry, by_entry_hold, examples, errors = collections.Counter(), collections.Counter(), collections.defaultdict(list), 0
for c, p, subj in rows:
    if c in shallow:
        continue
    try:
        fs = autorun.analyse(repo, p, c, data)
    except RuntimeError as e:
        errors += 1; continue
    if fs:
        listed += 1
    hs = [f for f in fs if f["hold"]]
    if hs:
        held += 1
    if any(f["entry"] != "github-actions" for f in hs):
        held_runs += 1
    for e in {f["entry"] for f in fs}:
        by_entry[e] += 1
    for e in {f["entry"] for f in hs}:
        by_entry_hold[e] += 1
        if len(examples[e]) < 3:
            paths = sorted({f["path"] for f in hs if f["entry"] == e})[:2]
            examples[e].append(f"{c[:9]} {subj[:70]} [{', '.join(paths)}]")
total = len(rows) - errors
print(f"{repo}: {total} first-parent commits, {listed} with something listed ({100*listed/total:.1f}%), "
      f"{held} a rule would hold ({100*held/total:.1f}%), {held_runs} on the Mac alone, without GitHub workflows ({100*held_runs/total:.1f}%)")
for e, k in by_entry.most_common():
    print(f"  {e:22} listed {k:4}  hold {by_entry_hold[e]:4}")
    for x in examples[e]:
        print(f"      {x}")
