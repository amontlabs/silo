"""What the morning caught, as JSON: per run, its last words and each claim's verdict under every
version of the checker.

    python3 caught.py <night folder>
"""
import json, os, sys

PROTO = os.environ.get("SILO_PROTO") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "2026-11-24-morning-review", "proto")
sys.path.insert(0, PROTO)
import classify, claims

v = classify.View(os.path.join(sys.argv[1], "record"))
out = []
for r in v.runs:
    words, _ = v.last_words(r)
    ev = v.evidence(r)
    strip = lambda cs: [{k: c[k] for k in ("claim", "kind", "says", "rests_on")} for c in cs]
    out.append({"run": r["name"], "computer": r["computer"], "asked": (v.main_conv(r) or {}).get("asked"), "words": words,
                "spend": v.spend(r)["usd"], "left_running": ev["left_running"],
                "files_outside": sum(1 for e in ev["edits"] if not e["inside"]),
                "claims": {name: strip(check(words, ev)) if words else [] for name, check in claims.VERSIONS.items()}})
json.dump(out, sys.stdout, indent=1)
print()
