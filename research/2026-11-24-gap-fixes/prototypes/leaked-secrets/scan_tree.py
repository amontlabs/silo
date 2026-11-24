"""Scan a tree of files or one file (- for stdin) and report hits per rule and grade, with the time
per MB. Values are never printed: a hit shows its rule, file, a masked context and a fingerprint.

Run: python3 scan_tree.py <path> [--samples N] [--mode keyword|all|off]
"""
import collections, os, sys, time
from detector import scan, scan_fast, mask

MAX_FILE = 5 * 1024 * 1024


def files(root):
    if root == "-" or os.path.isfile(root):
        yield root
        return
    for d, dirs, fs in os.walk(root):
        dirs[:] = [x for x in dirs if x != ".git"]
        for f in fs:
            p = os.path.join(d, f)
            if os.path.islink(p):
                continue
            yield p


def read(p):
    if p == "-":
        return sys.stdin.buffer.read().decode("utf-8", "replace")
    if os.path.getsize(p) > MAX_FILE and not os.path.samefile(p, sys.argv[1]):
        return None
    b = open(p, "rb").read()
    if b"\0" in b[:4096]:
        return None
    return b.decode("utf-8", "replace")


def main():
    root = sys.argv[1]
    nsamp = int(sys.argv[sys.argv.index("--samples") + 1]) if "--samples" in sys.argv else 3
    mode = sys.argv[sys.argv.index("--mode") + 1] if "--mode" in sys.argv else "keyword"
    texts = []
    for p in files(root):
        t = read(p)
        if t:
            texts.append((p, t))
    total = sum(len(t.encode()) for _, t in texts)
    counts = collections.Counter(); uniq = collections.defaultdict(set); samples = collections.defaultdict(list)
    t0 = time.perf_counter()
    for p, t in texts:
        for h in scan(t, entropy_mode=mode):
            k = (h["rule"], h["grade"])
            counts[k] += 1; uniq[k].add(h["fp"])
            if len(samples[k]) < nsamp:
                v = t[h["start"]:h["end"]]
                ctx = (t[max(0, h["start"] - 50):h["start"]] + "[" + mask(v) + "]" + t[h["end"]:h["end"] + 30]).replace("\n", "\\n")
                samples[k].append((os.path.relpath(p, root) if p != "-" and root != p else p, ctx, h["fp"]))
    dt = time.perf_counter() - t0
    t1 = time.perf_counter()
    nfast = sum(len(scan_fast(t)) for _, t in texts)
    dfast = time.perf_counter() - t1
    print(f"scanned {len(texts)} files, {total/1e6:.1f} MB")
    print(f"full scan (mode {mode}): {dt:.2f} s, {dt/(total/1e6)*1000:.1f} ms per MB")
    print(f"exact rules with prefilter: {dfast:.2f} s, {dfast/(total/1e6)*1000:.1f} ms per MB, {nfast} hits")
    for k in sorted(counts):
        print(f"  {k[0]:28} {k[1]:24} {counts[k]:7} hits {len(uniq[k]):6} distinct")
    print("samples (value masked):")
    for k in sorted(samples):
        if k[1].startswith("ignored") or k[1] == "failed-check":
            continue
        for p, ctx, fp in samples[k]:
            print(f"  {k[0]} | {p} | {ctx[:160]} | fp {fp}")


if __name__ == "__main__":
    main()
