"""Reconciles a program's own spend report with what the boundary saw of the same traffic, for a
computer whose model traffic passes unread. This is the part of Silo that decides what the app
shows and what a precaution budget counts.

Inputs, in <out-dir>:
- exchanges.jsonl: facts from passthrough.py (sizes and times of encrypted exchanges);
- reports.jsonl:   what the program said, from listener.py;
- trace.jsonl:     what the stand-in provider served and billed. Used only to score the result,
                   never by the reconciliation itself: Silo has no such file.

What it does, per computer:
1. Keeps the exchanges that can be model calls: at least MIN_UP bytes up. A Messages request
   carries the system prompt and tools, so it is never small; TLS handshakes and probes are.
2. Matches each `api_request` event to one exchange: the event's start (its time less its
   duration) within MATCH_S of the exchange's first byte up, and the event arrived at Silo after
   the exchange ended. Closest first.
3. Checks each match against the sizes. A request carries its whole prompt, cached or not, so its
   bytes bound the input tokens it can hold: an event whose input, cache read and cache write
   together are fewer than up_bytes / MAX_BYTES_PER_TOKEN tokens says less than the connection
   carried. The same for output against bytes down, more loosely, since hidden reasoning sends
   few bytes and many tokens (it can only make the report larger than the bytes, never smaller).
   And a cache read cannot be longer than the longest earlier request of the computer in the last
   hour, since a cache holds only what was sent.
4. Counts what is left: exchanges with no event (the report is silent or late for them) and
   events with no exchange (told, never seen: forged, or traffic that did not cross the boundary).
5. Gives four figures: spend as reported (Silo's own price table on the reported tokens), a floor
   for the exchanges with no event (their bytes at MAX_BYTES_PER_TOKEN, all at the cheapest rate
   the computer's model has), and an estimate for them (the dollars per byte up of this computer's
   matched exchanges), and a second estimate from the conversation's growth (see est2 below).

usage: reconcile.py <out-dir> [--json]
"""
import json, sys

MIN_UP = 2000                 # bytes up below which an exchange is not a model call
MATCH_S = 1.0                 # seconds between an event's start and an exchange's first byte up
MAX_BYTES_PER_TOKEN = 8.0     # bytes of request per prompt token, at most (see README: rests on the stand-in)
MAX_DOWN_PER_OUT_TOKEN = 40.0 # bytes of stream per output token, at most
UP_BYTES_PER_PROMPT_TOKEN = 4.0   # defaults for estimate 2 on a computer with no honest event
DOWN_BYTES_PER_OUT_TOKEN = 8.0
CACHE_MARGIN = 0.005         # share by which a cache read may exceed the longest earlier request
SILENT_AFTER = 3              # exchanges with no event after the last matched one: the report is silent
PRICES = {                    # $ per million tokens, from #374's prices.json
    "claude-opus-5-5": {"input": 4, "output": 20, "cache_read": 0.2, "cw5": 5, "cw1h": 8},
    "claude-opus-5": {"input": 4, "output": 20, "cache_read": 0.2, "cw5": 5, "cw1h": 8},
}
FLOOR_MODEL = "claude-opus-5-5"


def load(path):
    try:
        return [json.loads(l) for l in open(path) if l.strip()]
    except FileNotFoundError:
        return []


def price(model, inp, out, cr, cw5, cw1h):
    p = PRICES.get(model) or PRICES[FLOOR_MODEL]
    return (inp * p["input"] + out * p["output"] + cr * p["cache_read"] + cw5 * p["cw5"] + cw1h * p["cw1h"]) / 1e6


def num(a, k):
    return int(float(a.get(k) or 0))


def reconcile(d, until=None):
    """until: reconcile only what Silo had at that time (exchanges begun, exports arrived)."""
    ex = [e for e in load(f"{d}/exchanges.jsonl") if e["up_bytes"] >= MIN_UP and (until is None or e["t_up"] < until)]
    rep = [r for r in load(f"{d}/reports.jsonl") if until is None or r.get("got", 0) < until]
    events = [r for r in rep if r["kind"] == "log" and r["attrs"].get("event.name") == "api_request"]
    errors = [r for r in rep if r["kind"] == "log" and r["attrs"].get("event.name") == "api_error"]
    unreadable = [r for r in rep if r["kind"] == "unreadable"]
    ex.sort(key=lambda e: e["t_up"])
    for e in ex:
        e["event"] = None
    out = {"computers": sorted({e["computer"] for e in ex} | {r["computer"] for r in events})}

    told = []
    for r in sorted(events, key=lambda r: r["t"]):
        a = r["attrs"]
        start = r["t"] - num(a, "duration_ms") / 1000
        cands = [e for e in ex if e["event"] is None and e["computer"] == r["computer"]
                 and abs(e["t_up"] - start) <= MATCH_S and r["got"] >= e["t_end"]]
        e = min(cands, key=lambda e: abs(e["t_up"] - start)) if cands else None
        inp, out_t, cr = num(a, "input_tokens"), num(a, "output_tokens"), num(a, "cache_read_tokens")
        cw5, cw1h = num(a, "cache_creation_5m_tokens"), num(a, "cache_creation_1h_tokens")
        if cw5 + cw1h < num(a, "cache_creation_tokens"):
            cw5 = num(a, "cache_creation_tokens") - cw1h
        t = {"model": a.get("model"), "in": inp, "out": out_t, "cr": cr, "cw5": cw5, "cw1h": cw1h,
             "usd": price(a.get("model"), inp, out_t, cr, cw5, cw1h), "usd_told": float(a.get("cost_usd") or 0),
             "exchange": None, "flags": []}
        if e:
            e["event"] = t
            t["exchange"] = (e["conn"], e["exchange"])
            prompt = inp + cr + cw5 + cw1h
            t["up_per_token"] = e["up_bytes"] / max(prompt, 1)
            t["down_per_out"] = e["down_bytes"] / max(out_t, 1)
            if t["up_per_token"] > MAX_BYTES_PER_TOKEN:
                t["flags"].append("input-below-bytes")
            if t["down_per_out"] > MAX_DOWN_PER_OUT_TOKEN:
                t["flags"].append("output-below-bytes")
            # A cache read is a prefix the provider saw before, within the cache's hour: it cannot be
            # longer than the longest earlier request of this computer. The exchange's own bytes per
            # reported token convert that request's bytes to tokens, so the check needs no tokenizer.
            earlier = [x["up_bytes"] for x in ex if x["computer"] == e["computer"]
                       and x["t_up"] < e["t_up"] and x["t_up"] > e["t_up"] - 3600]
            if cr and (not earlier or cr > max(earlier) / t["up_per_token"] * (1 + CACHE_MARGIN)):
                t["flags"].append("cache-read-never-sent")
        else:
            t["flags"].append("no-exchange")
        told.append(t)

    matched = [t for t in told if t["exchange"]]
    unseen = [t for t in told if not t["exchange"]]
    silent_ex = [e for e in ex if e["event"] is None]
    honest = [t for t in matched if not t["flags"]]
    up_matched = sum(e["up_bytes"] for e in ex if e["event"] and not e["event"]["flags"])
    usd_per_up = (sum(t["usd"] for t in honest) / up_matched) if up_matched else None
    up_silent = sum(e["up_bytes"] for e in silent_ex)
    floor = up_silent / MAX_BYTES_PER_TOKEN * PRICES[FLOOR_MODEL]["cache_read"] / 1e6

    # the report's state, as the app would show it at the end of the record
    last_matched = max((i for i, e in enumerate(ex) if e["event"]), default=-1)
    after = len(ex) - 1 - last_matched
    if not ex:
        state = "no model traffic"
    elif not events:
        state = "silent: no report at all"
    elif after >= SILENT_AFTER:
        state = f"silent since exchange {last_matched + 2} of {len(ex)}"
    elif any(t["flags"] for t in told):
        state = "contradicted by what the boundary saw"
    elif silent_ex:
        state = "partial"
    else:
        state = "whole"

    # Estimate 2, by the conversation: a request repeats the one before it and adds to it, so its
    # bytes up beyond the previous request's are new prompt (a cache write, at the 1-hour rate
    # Claude Code uses on a plan) and the rest a cache read; the output is its bytes down. Bytes per
    # token, up and down, are this computer's medians over its unflagged events, or the defaults
    # below when it has none (the defaults rest on the stand-in; see the README).
    ratios = sorted(t["up_per_token"] for t in honest) or [UP_BYTES_PER_PROMPT_TOKEN]
    bpt = ratios[len(ratios) // 2]
    downs = sorted(t["down_per_out"] for t in honest) or [DOWN_BYTES_PER_OUT_TOKEN]
    dpt = downs[len(downs) // 2]
    est2 = 0.0
    p = PRICES[FLOOR_MODEL]
    for i, e in enumerate(ex):
        if e["event"] is not None and not e["event"]["flags"]:
            continue
        prev = ex[i - 1]["up_bytes"] if i else 0
        new = e["up_bytes"] - prev if 0 < prev <= e["up_bytes"] else e["up_bytes"]
        old = e["up_bytes"] - new
        est2 += (new / bpt * p["cw1h"] + old / bpt * p["cache_read"] + e["down_bytes"] / dpt * p["output"]) / 1e6

    served = load(f"{d}/trace.jsonl")
    billed = 0.0
    for s in served:
        u = s["response"]["usage"]; c = u.get("cache_creation") or {}
        billed += price(s["response"]["model"], u["input_tokens"], u["output_tokens"], u["cache_read_input_tokens"],
                        c.get("ephemeral_5m_input_tokens", 0), c.get("ephemeral_1h_input_tokens", 0))
    out.update({
        "state": state,
        "model_exchanges": len(ex), "events": len(events), "api_errors": len(errors), "unreadable_exports": len(unreadable),
        "matched": len(matched), "flagged": sum(1 for t in matched if t["flags"]),
        "flags": sorted({f for t in told for f in t["flags"]}),
        "exchanges_without_event": len(silent_ex), "events_without_exchange": len(unseen),
        "usd_reported": round(sum(t["usd"] for t in told), 6),
        "usd_reported_matched_unflagged": round(sum(t["usd"] for t in honest), 6),
        "usd_told_by_program": round(sum(t["usd_told"] for t in told), 6),
        "usd_floor_unreported": round(floor, 6),
        "usd_estimate_unreported": round(usd_per_up * up_silent, 6) if usd_per_up else None,
        "usd_estimate2_unreported": round(est2, 6),   # exchanges with no event or a flagged one
        "up_bytes_per_prompt_token": [round(t["up_per_token"], 2) for t in matched],
        "down_bytes_per_output_token": [round(t["down_per_out"], 1) for t in matched],
        "calls_served": len(served), "usd_billed": round(billed, 6),
        "usd_billed_unreported": round(billed - sum(t["usd"] for t in honest), 6),   # scoring only
    })
    return out


if __name__ == "__main__":
    r = reconcile(sys.argv[1])
    if "--json" in sys.argv:
        print(json.dumps(r))
    else:
        for k, v in r.items():
            print(f"{k}: {v}")
