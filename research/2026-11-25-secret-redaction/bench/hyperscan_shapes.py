"""The shape rules in one pass of Hyperscan, as a native engine would run them.

    python3 bench/hyperscan_shapes.py   (needs `pip install hyperscan`)

Hyperscan reports where a rule matches; the Python rule then runs only around those places, to
take the group and apply the gate. This measures the scan, which is the cost on clean text.
"""
import json, os, re, sys, time

import hyperscan

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
from redactors import RULES  # noqa: E402
from run import code  # noqa: E402


def native(p):
    flags = 0
    if p.startswith("(?i)"):
        p, flags = p[4:], flags | hyperscan.HS_FLAG_CASELESS
    p = re.sub(r"\(\?![^)]*\)", "", p).replace("*?", "*").replace("+?", "+").replace("}?", "}")
    p = p.replace("(?=[^A-Za-z0-9+/=\\s\\\\])", "")
    return p.encode(), flags


def main():
    text = code()
    pats = [native(p) for _, p in RULES]
    db = hyperscan.Database()
    db.compile(expressions=[p for p, _ in pats], ids=list(range(len(pats))), flags=[f for _, f in pats])
    hits = []
    best = 1e9
    for _ in range(5):
        hits.clear()
        t = time.perf_counter()
        db.scan(text, match_event_handler=lambda i, s, e, f, ctx: hits.append((i, e)))
        best = min(best, time.perf_counter() - t)
    print(json.dumps({"method": "shapes", "engine": "hyperscan, one pass", "bytes": len(text), "rules_hit": len(hits), "mb_per_s": round(len(text) / best / 1e6, 1)}))


if __name__ == "__main__":
    main()
