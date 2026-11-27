"""A precaution budget in dollars on a computer whose model traffic passes unread, replayed on a
recorded run: before each model exchange, Silo counts what it had at that moment (reconcile.py with
`until`) and denies the exchange when the count has reached the limit. Two ways to count:

  reported  the program's report only, as #407 recommends
  checked   the report where the boundary's sizes agree with it, and the conversation estimate
            (reconcile.py's estimate 2) for every exchange with no event yet, or a flagged one

Prints, for each, where it would have denied and what the stand-in had billed by then.
usage: budget.py <out-dir> <limit-usd>
"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import reconcile as R

d, limit = sys.argv[1], float(sys.argv[2])
ex = sorted((e for e in R.load(f"{d}/exchanges.jsonl") if e["up_bytes"] >= R.MIN_UP), key=lambda e: e["t_up"])
served = R.load(f"{d}/trace.jsonl")
cum, total = [], 0.0
for s in served:
    u = s["response"]["usage"]; c = u.get("cache_creation") or {}
    total += R.price(s["response"]["model"], u["input_tokens"], u["output_tokens"], u["cache_read_input_tokens"],
                     c.get("ephemeral_5m_input_tokens", 0), c.get("ephemeral_1h_input_tokens", 0))
    cum.append(total)
res = {"run": os.path.basename(d.rstrip("/")), "limit": limit, "billed_whole_run": round(total, 4)}
for way in ("reported", "checked"):
    stop = None
    for k, e in enumerate(ex):
        r = R.reconcile(d, until=e["t_up"])
        counted = r["usd_reported"] if way == "reported" else r["usd_reported_matched_unflagged"] + r["usd_estimate2_unreported"]
        if counted >= limit:
            stop = k; break
    if stop is None:
        res[way] = f"never denied; billed {total:.4f}"
    else:
        res[way] = f"denied call {stop + 1} of {len(ex)}; billed {cum[stop - 1] if stop else 0:.4f}"
print(json.dumps(res))
