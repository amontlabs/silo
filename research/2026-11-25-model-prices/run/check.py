"""Checks a run of release.sh from the boundary's record and the stand-in's bill.

usage: check.py <out dir> <case>
"""
import json, subprocess, sys, os

out, case = sys.argv[1:3]
E = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../2026-11-24-spend-budgets")
steps = [json.loads(l) for l in open(f"{out}/edge/out.txt") if l.startswith("{")]
rec = [json.loads(l) for l in open(f"{out}/record.jsonl") if l.strip()]
res = json.loads(subprocess.run([sys.executable, f"{E}/run/compare.py", out, "--json"], capture_output=True, text=True).stdout or "{}")
calls = [r for r in rec if r.get("kind") == "call"]
events = [r for r in rec if r.get("kind") == "prices"]
fails = 0


def check(name, ok, **k):
    global fails
    fails += 0 if ok else 1
    print(f"  {'PASS' if ok else 'FAIL'}  {name}  " + " ".join(f"{a}={b}" for a, b in k.items()))


def st(name):
    return next((s for s in steps if s["step"] == name), {})


print(f"{case}: billed ${res.get('billed')}, counted ${res.get('counted')}")
if case == "prices_release":
    check("v1: Opus is admitted", st("v1 opus").get("status") == 200)
    check("v1: a model with no row is refused under a budget", st("v1 new model").get("status") == 400, said=st("v1 new model").get("said"))
    check("a table edited by hand is refused for its signature", any(e["event"] == "bad_signature" for e in events), events=[e["event"] for e in events])
    tam = [c for c in calls if c.get("model") == "claude-opus-5-5"]
    check("the call after the edit is priced at the signed table, not the edited one", len(tam) >= 2 and abs(tam[1]["cost"] - tam[0]["cost"]) / tam[0]["cost"] < 0.5,
          first=round(tam[0]["cost"], 6) if tam else None, after_edit=round(tam[1]["cost"], 6) if len(tam) > 1 else None)
    check("v2 is taken and its new model admitted", st("v2 new model").get("status") == 200 and any(e["event"] == "loaded" and e["version"] == 2026112601 for e in events))
    check("an older signed release is refused", any(e["event"] == "rolled_back" for e in events) and st("v1 replayed, new model").get("status") == 200)
    geo = [c for c in calls if c.get("model") == "claude-opus-5-5"][-1:]
    if geo:
        g = geo[0]; base = 1e-6 * (g["input"] * 4 + g.get("cache_read", 0) * 0.2 + g.get("cache_write_short", 0) * 5 + g.get("cache_write_long", 0) * 8 + g["output"] * 20)
        check("US-only inference counted at 1.1x", abs(g["cost"] / base - 1.1) < 1e-3, ratio=round(g["cost"] / base, 4))  # the record rounds costs to $0.000001
    others = res.get("billed", 0) + (geo[0]["cost"] / 11 if geo else 0)
    check("counted equals the bill, plus the 10% of the US-only call", abs(res.get("counted", 0) - others) < 1e-3, counted=res.get("counted"), expected=round(others, 6))  # compare.py rounds to $0.001
elif case == "unknown_bound":
    bounds = [r for r in rec if r.get("kind") == "priced-as-bound"]
    check("the model with no row is admitted at a bound", len(bounds) >= 1 and st("new model 0").get("status") == 200, bound_input=bounds[0]["input"] if bounds else None, bound_output=bounds[0]["output"] if bounds else None)
    check("what is counted is at least what was billed", res.get("counted", 0) >= res.get("billed", 0), counted=res.get("counted"), billed=res.get("billed"))
    check("the budget of $1 holds: spend stops", any(s.get("status") == 400 for s in steps), said=[s.get("said") for s in steps if s.get("status") == 400][:1])
    check("billed stays within the budget", res.get("billed", 0) <= 1.0, billed=res.get("billed"))
print("OK" if fails == 0 else f"{fails} FAILED")
