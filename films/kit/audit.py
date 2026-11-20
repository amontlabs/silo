"""audit.py <tag> [case-substring…] [-- extra queries applied to every case]: held end of every scene, light, into audit/<tag>/<case>/."""
import sys, subprocess, pathlib, re, concurrent.futures as cf
HERE = pathlib.Path(__file__).parent.resolve()
args = sys.argv[1:]; tag = args.pop(0)
extra = args[args.index("--") + 1:] if "--" in args else []
names = args[:args.index("--")] if "--" in args else args
def run(c):
    n = len(re.findall(r"caption:", (c / "final.js").read_text()))
    q = [f"scene={i}" for i in range(n)] + extra
    r = subprocess.run([str(HERE / "render.sh"), str(c / "final.js"), str(HERE / "audit" / tag / c.name), (c / "ask.txt").read_text(), *q], capture_output=True, text=True)
    return f"== {c.name} ({n} scenes)\n{r.stdout}{r.stderr}"
cases = [c for c in sorted((HERE / "cases2").iterdir()) if (c / "final.js").exists() and (not names or any(s in c.name for s in names))]
with cf.ThreadPoolExecutor(3) as ex:
    for out in ex.map(run, cases): print(out)
