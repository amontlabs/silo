"""Scores a rule set on labelled cases. Used by measure.py.

A case is {id, provider, label, req}. label is hold:<outcome>, allow, either or deploy.
Recall counts hold cases; precision counts allow cases; either and deploy are reported, not scored.
"""
import os, sys, collections

HERE = os.path.dirname(os.path.abspath(__file__))
OLD = os.path.normpath(os.path.join(HERE, "..", "..", "2026-11-24-destructive-calls", "proto", "detector"))
sys.path.insert(0, OLD)
from detector import Detector  # noqa: E402  the #353 engine, unchanged
sys.path.insert(0, os.path.join(HERE, "detector"))
import engine  # noqa: E402  the same engine with two body conditions added

OLD_RULES = os.path.join(OLD, "rules.json")
BLIND_RULES = os.path.join(HERE, "detector", "rules-blind.json")  # frozen before the captures
NEW_RULES = os.path.join(HERE, "detector", "rules.json")  # blind + the rules tuned after them


def load(path):
    return (Detector if path == OLD_RULES else engine.Detector).load(path)


def score(det, cases):
    rows = []
    agg = collections.defaultdict(lambda: collections.Counter())
    for c in cases:
        r = det.check(dict(c["req"]))
        got = r["decision"]
        lab = c["label"].split(":")[0]
        a = agg[c["provider"]]
        if lab == "hold":
            a["hold_n"] += 1
            a["tp" if got == "hold" else "fn"] += 1
        elif lab == "allow":
            a["allow_n"] += 1
            a["fp" if got == "hold" else "tn"] += 1
        else:
            a[lab + "_n"] += 1
            a[lab + "_held"] += got == "hold"
        if r["verdict"] == "unknown":
            a["unknown"] += 1
        rows.append({"id": c["id"], "provider": c["provider"], "label": c["label"], "got": got,
                     "verdict": r["verdict"], "says": r["says"][:2], "rules": [m["rule"] for m in r["matches"]][:3],
                     "why": c.get("why", "")})
    tot = collections.Counter()
    for v in agg.values():
        tot.update(v)
    agg["total"] = tot
    return {k: dict(v) for k, v in agg.items()}, rows


def table(agg, title):
    lines = ["%s" % title,
             "| API | hold: n | held | missed | recall | allow: n | held wrongly | either: held/n | deploy: held/n |",
             "|---|---|---|---|---|---|---|---|---|"]
    for k, v in agg.items():
        hn, an = v.get("hold_n", 0), v.get("allow_n", 0)
        lines.append("| %s | %d | %d | %d | %s | %d | %d | %d/%d | %d/%d |" % (
            k, hn, v.get("tp", 0), v.get("fn", 0), ("%.0f%%" % (100.0 * v.get("tp", 0) / hn)) if hn else "-",
            an, v.get("fp", 0), v.get("either_held", 0), v.get("either_n", 0), v.get("deploy_held", 0), v.get("deploy_n", 0)))
    return "\n".join(lines)
