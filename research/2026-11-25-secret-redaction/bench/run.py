"""Measures the cost of each redactor on code an agent reads, in MB/s.

    python3 bench/run.py > results/cost.jsonl

The corpus is the text files of `legacy-2026-11-16` (code, much like the files agents read and the
tool results that carry them). The known values are random, made for the run in a temporary folder
that is deleted after; none is written to the repository. Two of every ten values are planted in
the corpus, in a few of their forms, so a scan has something to find.
"""
import json, os, random, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=HERE, capture_output=True, text=True, check=True).stdout.strip()
sys.path.insert(0, os.path.dirname(HERE))
from corpus import FORMATS  # noqa: E402
from redactors import Shapes, forms  # noqa: E402


def code(limit=8 << 20):
    files = subprocess.run(["git", "ls-tree", "-r", "--name-only", "legacy-2026-11-16"], capture_output=True, text=True, check=True, cwd=ROOT).stdout.split()
    out, size = [], 0
    for p in sorted(files):
        if not p.endswith((".rs", ".ts", ".tsx", ".py", ".md", ".json", ".sh", ".swift", ".yml")):
            continue
        b = subprocess.run(["git", "show", f"legacy-2026-11-16:{p}"], capture_output=True, cwd=ROOT).stdout
        out.append(b)
        size += len(b)
        if size >= limit:
            break
    return b"".join(out)


def main():
    rng = random.Random(25112026)
    base = code()
    with tempfile.TemporaryDirectory() as tmp:
        exe = os.path.join(tmp, "bench")
        subprocess.run(["cc", "-O2", "-o", exe, os.path.join(HERE, "bench.c")], check=True)
        for nsec in (0, 10, 50, 200, 1000):
            values = [rng.choice(FORMATS).make(rng) for _ in range(nsec)]
            text = bytearray(base)
            for v in values[: nsec // 5]:
                at = rng.randrange(len(text))
                text[at:at] = (" " + rng.choice(sorted(forms(v))) + " ").encode()
            cpath = os.path.join(tmp, "corpus")
            open(cpath, "wb").write(bytes(text))
            esc = lambda s: s.replace("\n", "\x01")
            for name, pats in (("pairs", values), ("pairs", [f for v in values for f in forms(v)]), ("anchors", [f for v in values for f in forms(v)])):
                ppath = os.path.join(tmp, "patterns")
                open(ppath, "w").write("".join(esc(p) + "\n" for p in pats))
                r = json.loads(subprocess.run([exe, cpath, ppath, name], capture_output=True, text=True, check=True).stdout)
                r["secrets"] = nsec
                r["forms"] = pats is not values
                print(json.dumps(r), flush=True)
        # The shape rules, in Python's regular expressions: an upper bound on their cost.
        text = base.decode("utf-8", "replace")
        for decode in (False, True):
            s = Shapes(decode=decode)
            t = time.perf_counter()
            s.spans(text)
            t = time.perf_counter() - t
            print(json.dumps({"method": "shapes" + ("+decode" if decode else ""), "engine": "python re", "bytes": len(base), "mb_per_s": round(len(base) / t / 1e6, 1)}))


if __name__ == "__main__":
    main()
