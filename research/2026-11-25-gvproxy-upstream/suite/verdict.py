"""Compare each mode's results with expected.txt. Prints a table and exits 1 if any case differs."""
import sys, os
exp, out, modes = sys.argv[1], sys.argv[2], sys.argv[3:]
lines = [l for l in open(exp) if l.strip()]
cols = lines[0].lstrip("#").split()[1:]
rows = [l.split() for l in lines if not l.startswith("#")]
got = {}
for m in modes:
    got[m] = {}
    for l in open(os.path.join(out, m + ".txt")):
        p = l.split(None, 1)
        if len(p) == 2: got[m][p[0]] = p[1].strip()
bad = 0
print("| case | " + " | ".join(modes) + " |"); print("|---" * (len(modes) + 1) + "|")
for r in rows:
    cells = []
    for m in modes:
        e = r[1 + cols.index(m)]; g = got[m].get(r[0], "(missing)")
        ok = e == "*" or e == g
        bad += not ok
        cells.append(g if ok else f"**{g}** (expected {e})")
    print(f"| {r[0]} | " + " | ".join(cells) + " |")
print(f"\n{bad} case(s) differ from expected.txt")
sys.exit(1 if bad else 0)
