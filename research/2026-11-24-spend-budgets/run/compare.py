"""Compares what the boundary counted with what the stand-in providers billed, for one run.

usage: compare.py <out dir> [--json]

The truth is the stand-in's ledger: every call it billed, priced with the same price table the
boundary uses (attempts at their own model's price, a dropped call at the output it generated).
"""
import json, sys, collections, statistics as st

def load(path):
    return [json.loads(l) for l in open(path) if l.strip()]

def price_rows(path):
    return json.load(open(path))["rows"]

def find(rows, provider, model, tier="standard", day="2026-11-24"):
    best = None
    for r in rows:
        if r["provider"] != provider or r.get("tier", "standard") != tier or r.get("from", "2000-01-01") > day: continue
        m = r["model"]
        if not (m == model or (m.endswith("*") and model.startswith(m[:-1]))): continue
        if best is None or r.get("from", "") > best.get("from", ""): best = r
    return best

def cost(r, c):
    tin = c.get("input", 0) + c.get("cache_read", 0) + c.get("cache_write_short", 0) + c.get("cache_write_long", 0)
    long = r.get("long_above") is not None and tin > r["long_above"]
    im = r.get("long_input_mult", 1) if long else 1; om = r.get("long_output_mult", 1) if long else 1
    return 1e-6 * (im * (c.get("input", 0) * r["input"] + c.get("cache_read", 0) * r.get("cache_read", 0) + c.get("cache_write_short", 0) * r.get("cache_write_short", 0) + c.get("cache_write_long", 0) * r.get("cache_write_long", 0)) + om * c.get("output", 0) * r["output"])

def truth_cost(rows, rec, day):
    tier = "fast" if rec.get("speed") == "fast" else "standard"
    if rec.get("iterations"):
        parts = rec["iterations"]
    else:
        parts = [dict(rec)]
    if not rec["completed"]:
        parts = [dict(parts[-1], output=rec["billed_output"])]
    total = 0
    for p in parts:
        r = find(rows, rec["provider"], p.get("model", rec["model"]), tier, day)
        if r is None: return None
        total += cost(r, p)
    return total

def main():
    out = sys.argv[1]
    cfg = json.load(open(f"{out}/config.json"))
    rows = price_rows(cfg["prices"])
    day = cfg.get("today", "2026-11-24")
    led_all = load(f"{out}/ledger.jsonl")
    led = [l for l in led_all if "plan" not in l]
    plan = [l for l in led_all if "plan" in l]
    rec = load(f"{out}/record.jsonl")
    calls = [r for r in rec if r.get("kind") == "call"]
    refused = [r for r in rec if r.get("kind") == "refused"]
    true_total = sum(truth_cost(rows, l, day) or 0 for l in led)
    seen_total = sum(c["cost"] for c in calls)
    res = {"calls_billed": len(led), "calls_counted": len(calls), "dropped_billed": sum(1 for l in led if not l["completed"]),
           "billed": round(true_total, 4), "counted": round(seen_total, 4), "counted_minus_billed": round(seen_total - true_total, 4),
           "estimated_calls": sum(1 for c in calls if c["estimated"]), "refused": collections.Counter(r["why"] for r in refused)}
    # Per computer and person, with the limits.
    lim = {c["name"]: c.get("limits", {}) for c in cfg["computers"]}
    by = collections.defaultdict(float)
    for c in calls: by[c["computer"]] += c["cost"]
    res["by_computer"] = {k: round(v, 4) for k, v in by.items()}
    res["limits"] = {k: v for k, v in lim.items() if v}
    for p in cfg.get("people", []):
        if p.get("limits"): res["limits"]["person:" + p["name"]] = p["limits"]
    # Overshoot of each total, by the stand-in's own bill.
    tc = collections.defaultdict(float)
    comp_of_provider_order = {}
    over = {}
    for name, l in lim.items():
        if l.get("total") is not None:
            over[name] = round(by.get(name, 0) - l["total"], 4)
    for p in cfg.get("people", []):
        t = (p.get("limits") or {}).get("total")
        if t is not None:
            spent = sum(v for k, v in by.items() if any(c["name"] == k and c.get("person") == p["name"] for c in cfg["computers"]))
            over["person:" + p["name"]] = round(spent - t, 4)
    res["over_limit_by"] = over
    # The estimate of input before a call leaves, against the provider's count.
    for src in ("cold", "conversation"):
        errs = []
        for c in calls:
            if c["est_from"] != src or not c["final"]: continue
            actual = c["input"] + c["cache_read"] + c["cache_write_short"] + c["cache_write_long"]
            if actual > 0: errs.append(c["est_tokens"] / actual - 1)
        if errs:
            errs.sort()
            res[f"estimate_{src}"] = {"n": len(errs), "median": round(st.median(errs), 3), "min": round(errs[0], 3), "max": round(errs[-1], 3),
                                      "under": sum(1 for e in errs if e < 0)}
    # Calls settled on an estimate: what the boundary put down against what was billed.
    est = [c for c in calls if c["estimated"]]
    if est:
        res["estimated_counted"] = round(sum(c["cost"] for c in est), 4)
        res["estimated_billed"] = round(sum(truth_cost(rows, l, day) or 0 for l in led if not l["completed"] or l.get("usage_sent") is False), 4)
    # Calls settled on the provider's own count: the boundary's sum against the bill.
    fin = [c for c in calls if not c["estimated"]]
    res["final_counted"] = round(sum(c["cost"] for c in fin), 6)
    res["final_billed"] = round(sum(truth_cost(rows, l, day) or 0 for l in led if l["completed"] and l.get("usage_sent") is not False), 6)
    if plan:
        last = plan[-1]
        moved = max((c.get("plan_5h_moved", 0) for c in calls), default=0)
        res["plan_5h"] = {"reading_end": round(last["u5h"], 4), "moved_by_this_computer_true": round(last["self_5h"], 4), "moved_by_others_true": round(last["other_5h"], 4), "boundary_put_on_computer": round(moved, 4)}
    alerts = [r for r in rec if r.get("kind") in ("alert", "reached")]
    res["alerts"] = [(r["kind"], r["ledger"]) for r in alerts]
    if "--json" in sys.argv: print(json.dumps(res))
    else:
        for k, v in res.items(): print(f"{k:22s} {v}")

main()
