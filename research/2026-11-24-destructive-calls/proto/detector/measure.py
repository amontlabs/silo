"""Runs the detector over corpus.jsonl (and the CLI captures in ../capture/captures, labelled by
capture_labels.py) and writes results.json: confusion per family, unknowns, misses, timing.

Run: python3 measure.py
"""
import base64
import json
import os
import statistics
import time

import corpus_build
import capture_labels
from detector import Detector

HERE = os.path.dirname(os.path.abspath(__file__))


def load_req(c):
    r = dict(c["req"])
    r["body"] = base64.b64decode(r.pop("body_b64", ""))
    return r


def run(reps=30):
    cases = corpus_build.main() + capture_labels.cases()
    d = Detector.load(os.path.join(HERE, "rules.json"))
    fam = {}
    rows = []
    times = []
    for c in cases:
        req = load_req(c)
        res = d.check(req)
        ts = []
        for _ in range(reps):
            t = time.perf_counter()
            d.check(req)
            ts.append((time.perf_counter() - t) * 1e3)
        times.append(statistics.median(ts))
        got = res["decision"]
        key = (c["family"], c["kind"])
        f = fam.setdefault(key, {"tp": 0, "fp": 0, "tn": 0, "fn": 0, "unknown": 0, "n": 0})
        f["n"] += 1
        if res["verdict"] == "unknown":
            f["unknown"] += 1
        if c["label"] == "hold":
            f["tp" if got == "hold" else "fn"] += 1
        else:
            f["fp" if got == "hold" else "tn"] += 1
        rows.append({"id": c["id"], "family": c["family"], "kind": c["kind"], "label": c["label"], "got": got,
                     "verdict": res["verdict"], "note": c["note"], "says": res["says"], "unknown": res["unknown"],
                     "ms": round(times[-1], 4)})
    times.sort()
    out = {
        "cases": len(cases),
        "plain": sum(c["kind"] == "plain" for c in cases),
        "evasion": sum(c["kind"] == "evasion" for c in cases),
        "capture": sum(c["kind"] == "capture" for c in cases),
        "families": {"%s/%s" % k: v for k, v in sorted(fam.items())},
        "totals": {k: sum(v[k] for v in fam.values()) for k in ("tp", "fp", "tn", "fn", "unknown", "n")},
        "ms_median": round(statistics.median(times), 4),
        "ms_p99": round(times[min(len(times) - 1, int(len(times) * 0.99))], 4),
        "ms_max": round(times[-1], 4),
        "wrong": [r for r in rows if r["label"] != r["got"]],
        "unknown_cases": [r for r in rows if r["verdict"] == "unknown"],
        "aws_source": d.apis[[a["id"] for a in d.apis].index("aws")]["_aws"].source,
        "rows": rows,
    }
    with open(os.path.join(HERE, "results.json"), "w") as f:
        json.dump(out, f, indent=1)
    return out


if __name__ == "__main__":
    o = run()
    print("cases", o["cases"], "plain", o["plain"], "evasion", o["evasion"], "capture", o["capture"])
    print("%-24s %4s %4s %4s %4s %4s %4s" % ("family/kind", "n", "tp", "fp", "tn", "fn", "unk"))
    for k, v in o["families"].items():
        print("%-24s %4d %4d %4d %4d %4d %4d" % (k, v["n"], v["tp"], v["fp"], v["tn"], v["fn"], v["unknown"]))
    t = o["totals"]
    print("%-24s %4d %4d %4d %4d %4d %4d" % ("total", t["n"], t["tp"], t["fp"], t["tn"], t["fn"], t["unknown"]))
    print("median ms", o["ms_median"], "p99 ms", o["ms_p99"], "max ms", o["ms_max"])
    print("aws:", o["aws_source"])
    for r in o["wrong"]:
        print("WRONG", r["id"], r["kind"], "label", r["label"], "got", r["got"], r["verdict"], "|", r["note"], "|", r["says"][:1], r["unknown"][:1])
