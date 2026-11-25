"""Scores rule sets, the fallback, and rules with the fallback behind them, on labelled cases.

A case is {id, provider, label, req}; label is hold:<outcome>, allow, either or deploy (#383's words).
Recall counts hold cases, false holds count allow cases; either and deploy are reported, not scored.
"""
import collections, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
R = os.path.normpath(os.path.join(HERE, "..", ".."))
OLD = os.path.join(R, "2026-11-24-destructive-calls", "proto", "detector")
P383 = os.path.join(R, "2026-11-25-destructive-calls-platforms", "proto")
sys.path.insert(0, OLD)
from detector import Detector  # noqa: E402  #353's engine
sys.path.insert(0, os.path.join(HERE, "detector"))
sys.path.insert(0, os.path.join(HERE, "fallback"))
import engine  # noqa: E402  #383's engine plus this folder's additions
import fallback  # noqa: E402

RULES = {
    "353": os.path.join(OLD, "rules.json"),
    "383": os.path.join(P383, "detector", "rules.json"),
    "383blind": os.path.join(P383, "detector", "rules-blind.json"),
    "blind": os.path.join(HERE, "detector", "rules-blind.json"),
    "tuned": os.path.join(HERE, "detector", "rules.json"),
}


class Fallback:
    """The fallback alone, on every request."""
    def check(self, req):
        f = fallback.classify(req)
        return {"decision": f["decision"], "verdict": "fallback", "says": [f["why"]], "matches": [{"rule": "fallback"}]}


class WithFallback:
    """A rule set first; when no rule matched the request, the fallback decides."""
    def __init__(self, det):
        self.det = det

    def check(self, req):
        r = self.det.check(dict(req))
        if r["decision"] == "hold" or r["matches"]:
            return r
        f = fallback.classify(req)
        if f["decision"] == "hold":
            return {"decision": "hold", "verdict": "fallback", "says": [f["why"]], "matches": [{"rule": "fallback:" + (f["outcome"] or "")}]}
        return r


def load(name):
    if name == "fallback":
        return Fallback()
    if name.endswith("+fb"):
        return WithFallback(load(name[:-3]))
    path = RULES[name]
    return (Detector if name == "353" else engine.Detector).load(path)


def score(det, cases):
    rows = []
    agg = collections.defaultdict(collections.Counter)
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
        rows.append({"id": c["id"], "provider": c["provider"], "label": c["label"], "got": got,
                     "verdict": r["verdict"], "says": r["says"][:2], "rules": [m.get("rule") for m in r["matches"]][:3]})
    tot = collections.Counter()
    for v in agg.values():
        tot.update(v)
    agg["total"] = tot
    return {k: dict(v) for k, v in agg.items()}, rows


def table(agg, title):
    lines = [title, "| API | hold: n | held | missed | recall | allow: n | held wrongly | either: held/n | deploy: held/n |",
             "|---|---|---|---|---|---|---|---|---|"]
    for k, v in agg.items():
        hn = v.get("hold_n", 0)
        lines.append("| %s | %d | %d | %d | %s | %d | %d | %d/%d | %d/%d |" % (
            k, hn, v.get("tp", 0), v.get("fn", 0), ("%.0f%%" % (100.0 * v.get("tp", 0) / hn)) if hn else "-",
            v.get("allow_n", 0), v.get("fp", 0), v.get("either_held", 0), v.get("either_n", 0),
            v.get("deploy_held", 0), v.get("deploy_n", 0)))
    return "\n".join(lines)
