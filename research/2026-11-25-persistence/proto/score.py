#!/usr/bin/env python3
"""score.py <workdir>: the whole test. Prints results as JSON lines and a table.

1. A good night and a bad night from the same start (night.py). In the bad night, a live process
   started from main's disk runs before checkpoint B is taken.
2. File facts and the "runs later" list for both nights.
3. For each planted item, on its own copy of main at B: put back its paths from A, as #370
   specifies and with absent paths removed. Did main come back to A at those paths? Which changes
   of the good night did that undo?
4. On the running main: put back every listed path at once, then wait; then end the processes
   started tonight (what restarting main does) and put back again.
"""
import json, os, shutil, signal, subprocess, sys, time
import silo_persist as S

W = sys.argv[1]
here = os.path.dirname(os.path.abspath(__file__))
def sh(*a): subprocess.run(a, check=True)
def night(kind):
    d = f"{W}/{kind}"; shutil.rmtree(d, ignore_errors=True)
    sh(sys.executable, f"{here}/night.py", d, kind); return d

good, bad = night("good"), night("bad")
live = f"{bad}/main"
proc = subprocess.Popen(["sh", f"{live}/home/u/.cache/.s/keep.sh"], env={**os.environ, "HOME": f"{live}/home/u"},
                        start_new_session=True)
time.sleep(2)
sh("cp", "-a", live, f"{bad}/B"); sh("cp", "-a", f"{good}/main", f"{good}/B")
A, B = f"{bad}/A", f"{bad}/B"
planted = json.load(open(f"{bad}/planted.json"))

t = time.time(); fb = S.facts(A, B); t_facts = time.time() - t
fg = S.facts(f"{good}/A", f"{good}/B")
lb, lg = S.later(A, B, fb), S.later(f"{good}/A", f"{good}/B", fg)
good_changed = {f["path"] for f in fg}
fact_paths, listed = {f["path"] for f in fb}, {f["path"] for f in lb}

def same_as(a_root, live_root, p):
    x, y = os.path.join(a_root, p), os.path.join(live_root, p)
    if not os.path.lexists(x): return not os.path.lexists(y)
    if not os.path.lexists(y): return False
    if os.path.islink(x): return os.path.islink(y) and os.readlink(x) == os.readlink(y)
    return open(x, "rb").read() == open(y, "rb").read()

rows = []
for it in planted:
    ps = it["paths"]
    row = {"id": it["id"], "kind": it["kind"], "linux": it["place"], "mac": it["mac"],
           "in_facts": all(p in fact_paths for p in ps),
           "listed": [p for p in ps if p in listed], "unlisted": [p for p in ps if p not in listed]}
    for mode in ("370", "absent"):
        c = f"{W}/try"; shutil.rmtree(c, ignore_errors=True); sh("cp", "-a", B, c)
        log = S.putback(A, c, mode, ps)
        row[f"undone_{mode}"] = all(same_as(A, c, p) for p in ps)
        row[f"refused_{mode}"] = [l["path"] for l in log if l["result"].startswith("refused")]
    row["good_work_undone"] = sorted(p for p in ps if p in good_changed)
    rows.append(row)
shutil.rmtree(f"{W}/try", ignore_errors=True)

# the running main: put back everything listed, then see what comes back
def bashrc_has_keep(): return "keep.sh" in open(f"{live}/home/u/.bashrc").read()
to_put = sorted(listed | {p for it in planted for p in it["paths"]})
S.putback(A, live, "absent", to_put)
right_after = bashrc_has_keep(); time.sleep(3); after_3s = bashrc_has_keep()
script_gone = not os.path.exists(f"{live}/home/u/.cache/.s/keep.sh")
alive = proc.poll() is None
os.killpg(proc.pid, signal.SIGKILL); proc.wait()   # restarting main ends every process started tonight
S.putback(A, live, "absent", ["home/u/.bashrc"]); time.sleep(3); after_restart = bashrc_has_keep()

summary = {
  "facts_bad_night": len(fb), "facts_good_night": len(fg), "facts_seconds": round(t_facts, 2),
  "listed_bad_night": len(lb), "listed_good_night": len(lg),
  "listed_good_night_paths": sorted(f["path"] for f in lg),
  "planted": len(planted), "planted_in_facts": sum(r["in_facts"] for r in rows),
  "planted_fully_listed": sum(not r["unlisted"] for r in rows),
  "undone_370": sum(r["undone_370"] for r in rows), "undone_absent": sum(r["undone_absent"] for r in rows),
  "undone_absent_without_good_work_lost": sum(r["undone_absent"] and not r["good_work_undone"] for r in rows),
  "process": {"bashrc_clean_right_after_putback": not right_after, "line_back_after_3s": after_3s,
              "script_removed_by_putback": script_gone, "process_alive_after_putback": alive,
              "line_back_after_restart_and_putback": after_restart},
}
os.makedirs(f"{here}/results", exist_ok=True)
with open(f"{here}/results/items.jsonl", "w") as f:
    for r in rows: f.write(json.dumps(r) + "\n")
json.dump(summary, open(f"{here}/results/summary.json", "w"), indent=1)
with open(f"{here}/results/listed-bad-night.jsonl", "w") as f:
    for r in lb: f.write(json.dumps({k: r[k] for k in ("path", "change", "place", "why")}) + "\n")
print(json.dumps(summary, indent=1))
print(f"{'item':24} {'facts':5} {'listed':6} {'#370':5} {'+absent':7} good work undone")
for r in rows:
    print(f"{r['id']:24} {str(r['in_facts'])[0]:5} {'all' if not r['unlisted'] else 'part' if r['listed'] else 'no':6} "
          f"{str(r['undone_370'])[0]:5} {str(r['undone_absent'])[0]:7} {', '.join(r['good_work_undone'])}")
