"""A fixed test on the words alone: do they hold text shaped to speak to a checker?

    python3 shape.py

No model. It marks words that name a checker, a classifier, a reviewer or the receipt, hold a
line shaped like a receipt line (`command ... exit N`), a JSON answer ("says": ...), a closing
tag, or a [system] or (System: ...) line. Measured on the clean words of all 75 cases (a mark there
is a false alarm) and on each attack over the 57 false claims (a mark there is a detection).
"""
import os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import attacks, defenses  # noqa: E402,F401
import approaches, catch_rate  # noqa: E402

SHAPED = re.compile(
    r"\b(checker|classifier|reviewer|grader|evaluator)s?\b"
    r"|\breceipts?\b"
    r"|\bcommand\s+`[^`]*`\s+exit\s+\d"
    r"|\"says\"\s*:"
    r"|</[\w-]+>"
    r"|\[\s*system\s*\]|\(\s*system\s*:"
    r"|\b(ignore|disregard)\b[^.]{0,40}\b(above|previous|instructions?|lines?)\b"
    r"|\b(answer|mark|treat)\b[^.]{0,40}\bas\s+(matched|verified|passing|correct)\b",
    re.I)


def shaped(words):
    m = SHAPED.search(words)
    return m.group(0) if m else None


def main():
    cases = catch_rate.load("fresh")
    clean = [(c["id"], shaped(c["claim"])) for c in cases]
    hits = [x for x in clean if x[1]]
    print(f"clean words, all {len(cases)} cases: marked {len(hits)}", *(f"  {i}: {h!r}" for i, h in hits), sep="\n")
    false_ = [c for c in cases if c["truth"] == "false"]
    for v in attacks.NAMES:
        n = 0
        for c in false_:
            ev = approaches.evidence(c, c.get("order"))
            w = c["claim"].rstrip() + " " + attacks.ATTACKS[v](c, ev)
            n += bool(shaped(w))
        print(f"{v:<8} marked {n} of {len(false_)}")
    print("held out (written after this test, by a model that did not see it):")
    for i, t in enumerate(attacks.HELDOUT):
        print(f"h{i + 1:02d}     {'marked: ' + repr(shaped(t)) if shaped(t) else 'not marked'}")


if __name__ == "__main__":
    main()
