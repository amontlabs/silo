"""Scores each redactor on the corpus: for every value in every place, how many of the characters
that carry it are left in what the record would keep.

    python3 measure.py [seed] > results/coverage.md

A sample is held when no character that carries the value is left, partial when some are, and
missed when more than half are. Nothing printed holds a value.
"""
import collections, json, sys

from corpus import FORMATS, PLACES, corpus
from redactors import Exact, Forms, Shapes, merge


def left(sample, spans_per_text):
    """Characters of the value left after redaction, and the total."""
    total = kept = 0
    for t, s, e in sample.spans:
        total += e - s
        cover = spans_per_text[t]
        for i in range(s, e):
            if not any(a <= i < b for a, b in cover):
                kept += 1
    return kept, total


def verdict(kept, total):
    if kept == 0:
        return "held"
    return "missed" if kept * 2 > total else "partial"


def main():
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 25112026
    shapes, shapes_dec = Shapes(), Shapes(decode=True)
    cols = ["#386 exact", "forms", "shapes", "shapes+decode", "forms+shapes"]
    by_place = collections.defaultdict(lambda: collections.Counter())
    by_format = collections.defaultdict(lambda: collections.Counter())
    n = 0
    for f, v, pname, sample in corpus(seed):
        assert sample.spans, (f.name, pname)
        n += 1
        ex, fo = Exact([v]), Forms([v])
        for col in cols:
            per = []
            for text in sample.texts:
                if col == "#386 exact":
                    sp = ex.spans(text)
                elif col == "forms":
                    sp = fo.spans(text)
                elif col == "shapes":
                    sp = shapes.spans(text)
                elif col == "shapes+decode":
                    sp = shapes_dec.spans(text)
                else:
                    sp = merge(fo.spans(text) + shapes_dec.spans(text))
                per.append(sp)
            kept, total = left(sample, per)
            r = verdict(kept, total)
            by_place[pname][(col, r)] += 1
            by_format[f.name][(col, r)] += 1

    def table(title, rows, keys):
        print(f"## {title}\n")
        print("| | " + " | ".join(cols) + " |")
        print("|---|" + "---|" * len(cols))
        for k in keys:
            c = rows[k]
            tot = sum(c[(cols[0], r)] for r in ("held", "partial", "missed"))
            cells = []
            for col in cols:
                h, p, m = c[(col, "held")], c[(col, "partial")], c[(col, "missed")]
                cells.append(f"{h}/{tot}" + (f" ({p} partial)" if p else ""))
            print(f"| {k} | " + " | ".join(cells) + " |")
        print()

    print(f"# Redaction coverage, seed {seed}\n")
    print(f"{n} samples: {len(FORMATS)} formats, 3 values each, {len(PLACES)} places. "
          "A cell is the samples held (no character of the value left); partial ones left some.\n")
    table("By place", by_place, [p for p, _ in PLACES])
    table("By format", by_format, [f.name for f in FORMATS])
    tot = collections.Counter()
    for c in by_place.values():
        tot.update(c)
    print("## All\n")
    for col in cols:
        print(f"- {col}: {tot[(col, 'held')]} held, {tot[(col, 'partial')]} partial, {tot[(col, 'missed')]} missed of {n}")
    json.dump({p: {f"{c}|{r}": k for (c, r), k in v.items()} for p, v in by_place.items()},
              open("results/coverage.json", "w"), indent=1, sort_keys=True)


if __name__ == "__main__":
    main()
