"""Where `Shapes` fires on text that holds no secret: every text file of this repository at a commit,
plus the code frozen at `legacy-2026-11-16`. Prints counts by rule and the files, never a match.

    python3 falsepos.py <commit> ... > results/false-positives.md

A hit here is either a false positive or a real credential in the repository. Each rule's hits are
looked at by a person with the value masked; a real one is reported, never copied.
"""
import collections, subprocess, sys

from redactors import Shapes


def files(rev):
    out = subprocess.run(["git", "ls-tree", "-r", "-z", "--name-only", rev], capture_output=True, check=True).stdout
    for p in out.split(b"\0"):
        if p:
            yield p.decode()


def main():
    shapes, ungated = Shapes(), Shapes(gate=False)
    per_rule, where = collections.Counter(), collections.defaultdict(set)
    loose = collections.Counter()
    size = nfiles = 0
    for rev in sys.argv[1:]:
        for p in files(rev):
            blob = subprocess.run(["git", "show", f"{rev}:{p}"], capture_output=True).stdout
            if b"\0" in blob[:4096] or len(blob) > 4 << 20:
                continue
            text = blob.decode("utf-8", "replace")
            size += len(blob)
            nfiles += 1
            for rule, s, e in ungated.hits(text):
                loose[rule] += 1
            for rule, s, e in shapes.hits(text):
                per_rule[rule] += 1
                where[rule].add(f"{rev}:{p}")
    print(f"# Where the shape rules fire on clean text\n")
    print(f"{nfiles} text files, {size / 1e6:.1f} MB, at {', '.join(sys.argv[1:])}.\n")
    print("| Rule | Hits without the gate | Hits | Per MB | Files |")
    print("|---|---|---|---|---|")
    for rule, n in loose.most_common():
        k = per_rule[rule]
        print(f"| {rule} | {n} | {k} | {k / (size / 1e6):.2f} | {len(where[rule])} |")
    print()
    for rule in per_rule:
        print(f"- {rule}: " + ", ".join(sorted(where[rule])[:6]) + (" …" if len(where[rule]) > 6 else ""))


if __name__ == "__main__":
    main()
