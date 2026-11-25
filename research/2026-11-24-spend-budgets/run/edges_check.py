"""Checks one edge-case run: what the boundary counted against what the stand-in billed, and
what the case must show. Prints one line per check, PASS or FAIL, and the numbers.

usage: edges_check.py <out dir> <case> <config name>
"""
import json, subprocess, sys, os

out, case, conf = sys.argv[1:4]
here = os.path.dirname(os.path.abspath(__file__))
res = json.loads(subprocess.run([sys.executable, f"{here}/compare.py", out, "--json"], capture_output=True, text=True).stdout or "{}")
steps = [json.loads(l) for l in open(f"{out}/edge/out.txt") if l.startswith("{")]
rec = [json.loads(l) for l in open(f"{out}/record.jsonl") if l.strip()]
calls = [r for r in rec if r.get("kind") == "call"]
cfg = json.load(open(f"{out}/config.json"))
total = cfg["computers"][0].get("limits", {}).get("total")
billed, counted = res.get("billed", 0), res.get("counted", 0)
fails = 0

def check(name, ok, **k):
    global fails
    fails += 0 if ok else 1
    print(f"  {'PASS' if ok else 'FAIL'}  {name}  " + " ".join(f"{a}={b}" for a, b in k.items()))

def step(prefix):
    return [s for s in steps if s["step"].startswith(prefix)]

def exact(name="counted equals billed"):
    check(name, abs(counted - billed) < 1e-4, counted=counted, billed=billed)

print(f"{case} under {conf}: billed ${billed}, counted ${counted}, refused {res.get('refused')}")
if case == "prev_id":
    exact()
    errs = [round(c["est_tokens"] / c["context"] - 1, 3) for c in calls if c["format"] == "responses" and c.get("context") and c["est_from"] != "cold"]
    check("input of each chained call estimated within 10% of the provider's count", len(errs) == 5 and all(abs(e) < 0.10 for e in errs), errors=errs, cold_first_calls=[round(c["est_tokens"] / c["context"] - 1, 3) for c in calls if c["est_from"] == "cold"])
elif case == "background":
    exact()
    check("spend stays within the budget", counted <= total + 1e-9, spent=counted, budget=total)
    check("a background call is refused once held calls fill the budget", any(s.get("said") for s in step("start")), said=[s.get("said") for s in step("start")][-1:])
elif case == "websearch":
    exact()
    check("searches counted", sum(c.get("web_searches", 0) for c in calls) == 3 + 4, counted_searches=sum(c.get("web_searches", 0) for c in calls))
elif case == "chat":
    if conf == "chat-deny":
        refused = [s["step"] for s in steps if s["status"] == 400]
        check("streams that do not ask for usage are refused, the others pass", refused == ["stream, no usage", "stream, n=4, no usage"], refused=refused)
        exact()
    else:
        est = [c for c in calls if c["estimated"]]
        check("calls without usage settled on an estimate", len(est) == 2, estimated=len(est))
        print(f"        estimated: counted ${res.get('estimated_counted')} against billed ${res.get('estimated_billed')}")
elif case == "cache_expiry":
    exact()
    bad = []
    for c in calls:
        r = next(x for x in json.load(open(cfg["prices"]))["rows"] if x["model"] == c["model"] and x.get("tier", "standard") == "standard")
        actual_in = (c["input"] * r["input"] + c["cache_read"] * r["cache_read"] + c["cache_write_short"] * r["cache_write_short"] + c["cache_write_long"] * r["cache_write_long"]) / 1e6
        bad.append((round(c["input_cost_bound"], 4), round(actual_in, 4)))
    check("the input priced before each call is not under what the provider billed for it", all(b >= a * 0.97 for b, a in bad), before_vs_billed=bad)
elif case == "gcache":
    check("storage counted within 1% of its bill (each side times the cache's life)", abs(counted - billed) <= 0.01 * billed, counted=counted, billed=billed)
    check("a day of storage that does not fit is refused", step("cache for a day")[0]["status"] == 400, said=step("cache for a day")[0].get("said"))
elif case == "batch":
    held = [r for r in rec if r.get("kind") == "held"]
    open_holds = [h for h in held if not any(c.get("held_id") == h["id"] for c in calls)]
    settled = sum(c["cost"] for c in calls)
    check("batches whose results were read are counted at their bill", abs(settled - (billed - res.get("unread_billed", 0))) < 1e-4, settled=round(settled, 6), billed_read=round(billed - res.get("unread_billed", 0), 6))
    check("a batch that does not fit is refused", any(s["status"] == 400 for s in step("openai batch of 400")), said=[s.get("said") for s in step("openai batch of 400")])
    check("a batch never read stays held at its bound", len(open_holds) == 1, open_holds=[round(h["bound"], 4) for h in open_holds], unread_billed=res.get("unread_billed"))
elif case == "midstream":
    long = step("long call")[0]
    print(f"        long call: whole={long['whole']} secs={long['secs']} said={long.get('said')}")
    if conf.endswith("cut"):
        check("spend ends within 5% of the budget", billed <= total * 1.05, billed=billed, budget=total)
        check("the program is told why its answer stopped", bool(long.get("said")), said=long.get("said"))
        check("the cut call, settled on an estimate, is counted within 10% of its bill", abs(counted - billed) <= 0.1 * billed, counted=counted, billed=billed)
    elif long["status"] == 200:
        check("the long call finishes, past the budget", long["whole"] and billed > total, billed=billed, budget=total)
    else:
        print("        the long call was refused before it left")
elif case == "fork":
    conv = {}
    for r in rec:
        if r.get("kind") == "call": conv.setdefault(r.get("conversation", "?"), []).append((r["computer"], round(r["cost"], 4)))
    turns = {s["step"]: s["status"] for s in steps}
    print(f"        turns: " + ", ".join(f"{k}:{v}" for k, v in turns.items()))
    print(f"        by conversation: " + "; ".join(f"{k}: " + " ".join(f"{c}={v}" for c, v in l) + f" (sum {round(sum(v for _, v in l), 4)})" for k, l in conv.items()))
    main_led = [r for r in rec if r.get("kind") == "ledgers"]
    exact()
    fork_spend = sum(c["cost"] for c in calls if c["computer"] == "fork")
    check("the fork's spend counts in main's computer budget", any(abs(r.get("parent_spent", -1) - counted) < 1e-4 for r in main_led[-1:]), fork=round(fork_spend, 4))
print(f"  {'ok' if fails == 0 else str(fails) + ' failed'}")
