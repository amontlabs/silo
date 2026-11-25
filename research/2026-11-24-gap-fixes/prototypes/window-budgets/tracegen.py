"""Synthetic weeks of model calls, with one fault each, to run the three alerts over.

Each scenario is a week of the computer's calls (built with sim.py's conversation model) plus the
laptop's, which only move the reported week figure. A fault starts at a known time, so every alert
can be called true (after the fault, and about it) or false.

Run: python3 tracegen.py        (prints, per scenario, each kind of alert: count, first time, true/false)
"""
import math, random
import numpy as np
import sim
from detectors import Detectors

DAY, HOUR = sim.DAY, 3600.0


def week(seed, fault=None, laptop=1.0):
    rng = random.Random(seed)
    calls = []          # (t, who, conv, asked, answered, usage, ttl, headless)
    conv_id = [0]

    def session(t0, n_turns, model, start_ctx, ttl, who, step_s=12.0):
        conv_id[0] += 1
        cid = f"{who}-{conv_id[0]}"
        c = sim.Conversation(rng, start_ctx, ttl)
        t = t0
        asked = model
        for k in range(n_turns):
            if t >= 7 * DAY:
                break
            ttl_now = ttl
            if fault == "ttl_cut" and who == "computer" and t >= 2 * DAY:
                ttl_now = 300
            c.ttl = ttl_now
            u = c.turn(t)
            if fault == "breakpoints_stuck" and who == "computer" and t >= 4 * DAY:
                # the program stops moving its cache marker: only the system prefix is read
                whole = u["cr"] + u["w1"] + u["w5"]
                u = dict(u, cr=min(whole, 20_000), w1=0.0, w5=0.0, **{"in": max(0.0, whole - 20_000)})
            if fault == "program_fallback" and who == "computer" and t >= 3 * DAY + 11 * HOUR and k > 3 and asked == "claude-opus-5-5":
                asked = "claude-sonnet-5-5"               # the program's own fallback after overloads
            answered = asked
            if fault == "provider_swap" and who == "computer" and 3 * DAY + 14 * HOUR <= t < 3 * DAY + 16 * HOUR:
                answered = "claude-opus-5"
            calls.append((t, who, cid, asked, answered, u, ttl_now, who == "computer"))
            t += rng.lognormvariate(math.log(step_s), 0.9)
            if rng.random() < 0.03:
                t += rng.uniform(300, 1500)               # you read, think, come back

    # the computer: a nightly job of 12 runs, and sessions in working hours
    for d in range(7):
        for j in range(12):
            session(d * DAY + 1 * HOUR + j * 600, 3, "claude-sonnet-5-5", 40_000, 3600, "computer", 8.0)
        if d % 7 in (5, 6):
            continue
        for _ in range(np.random.default_rng(rng.randrange(1 << 30)).poisson(3)):
            session(d * DAY + rng.uniform(9, 18) * HOUR, int(rng.lognormvariate(math.log(40), 0.6)), "claude-opus-5-5", 30_000, 3600, "computer")
    if fault == "runaway_loop":
        # from day 2 at 02:00, a scheduled job every 150 s, 3 turns of 40k each (#110395)
        t = 2 * DAY + 2 * HOUR
        while t < 7 * DAY:
            session(t, 3, "claude-sonnet-5-5", 40_000, 3600, "computer", 8.0)
            t += rng.lognormvariate(math.log(150), 0.3)
    for d in range(7):
        if d % 7 in (5, 6):
            continue
        for _ in range(np.random.default_rng(rng.randrange(1 << 30)).poisson(4 * laptop)):
            session(d * DAY + rng.uniform(9, 18) * HOUR, int(rng.lognormvariate(math.log(40), 0.6)), "claude-opus-5-5", 30_000, 3600, "laptop")
    calls.sort(key=lambda c: c[0])
    mdl = lambda m: "sonnet" if "sonnet" in m else "opus"
    costs = [sim.hidden_cost(c[5], mdl(c[4]), c[7]) for c in calls]
    normal_week = sum(cst for c, cst in zip(calls, costs))
    cap = normal_week / 0.6 if fault != "runaway_loop" else normal_week / 1.6
    cum = 0.0
    out = []
    TRUTH["outside"] = sum(cst for c, cst in zip(calls, costs) if c[1] != "computer") / cap
    for c, cst in zip(calls, costs):
        cum += cst
        if c[1] != "computer":
            continue
        u = c[5]
        usage = {"input_tokens": int(u["in"]), "output_tokens": int(u["out"]), "cache_read_input_tokens": int(u["cr"]),
                 "cache_creation_input_tokens": int(u["w5"] + u["w1"]),
                 "cache_creation": {"ephemeral_5m_input_tokens": int(u["w5"]), "ephemeral_1h_input_tokens": int(u["w1"])}}
        out.append({"t": c[0], "computer": "main", "conversation": c[2], "main": True, "_own_share": cst / cap,
                    "request": {"model": c[3], "cache_ttls": ["1h" if c[6] == 3600 else "5m"]},
                    "response": {"model": c[4], "usage": usage},
                    "headers": {"anthropic-ratelimit-unified-7d-utilization": f"{math.floor(100 * cum / cap) / 100:.2f}",
                                "anthropic-ratelimit-unified-7d-reset": str(int(7 * DAY))}})
    return out


TRUTH = {}


def outside_seen(trace, idle=600):
    """The week's rise between two of the computer's calls more than `idle` seconds apart: points
    counted while the computer had no call in flight, so spent elsewhere (a fact, up to the
    provider's accounting lag)."""
    rise, own_in_it, prev = 0.0, 0.0, None
    for r in trace:
        u = float(r["headers"]["anthropic-ratelimit-unified-7d-utilization"])
        if prev and r["t"] - prev[0] > idle:
            rise += max(0.0, u - prev[1])
            own_in_it += r["_own_share"]          # the call that ends the gap is the computer's own
        prev = (r["t"], u)
    return rise, own_in_it


FAULTS = {None: None, "provider_swap": 3 * DAY + 14 * HOUR, "program_fallback": 3 * DAY + 11 * HOUR,
          "ttl_cut": 2 * DAY, "breakpoints_stuck": 4 * DAY, "runaway_loop": 2 * DAY + 2 * HOUR}
EXPECT = {"provider_swap": "1a", "program_fallback": "1b", "ttl_cut": "2a", "breakpoints_stuck": "2c", "runaway_loop": ("3a", "3b")}


def fmt(t):
    return f"day {int(t // DAY)} {int(t % DAY // 3600):02d}:{int(t % 3600 // 60):02d}"


if __name__ == "__main__":
    seeds = range(5)
    print("== reconciliation: the week's rise while the computer had nothing in flight")
    for lap in [0.5, 1.0, 2.0]:
        seen, true = [], []
        for s in seeds:
            tr = week(s, None, laptop=lap); true.append(TRUTH["outside"]); seen.append(outside_seen(tr))
        print(f"   laptop x{lap}: other devices used {100 * np.median(true):.1f} points; the week rose {100 * np.median([x[0] for x in seen]):.1f} across the computer's idle gaps,"
              f" of which {100 * np.median([x[1] for x in seen]):.1f} was the computer's own first call after each gap (median of 5 weeks)")
    for fault, t0 in FAULTS.items():
        kinds = {}
        for s in seeds:
            d = Detectors(share_pace_per_day=0.30 / 7)   # a share budget of 30% of the week
            for r in week(s + 100, None):               # a normal week before, shifted back
                d.feed(dict(r, t=r["t"] - 7 * DAY, conversation="prev-" + r["conversation"], headers={**r["headers"], "anthropic-ratelimit-unified-7d-reset": "0"}))
            d.alerts.clear()
            for r in week(s, fault):
                d.feed(r)
            for al in d.alerts:
                k = al["kind"].split()[0]
                good = fault is not None and k in EXPECT[fault] and al["t"] >= t0
                e = kinds.setdefault(al["kind"], {"n": 0, "true": 0, "first": [], "seeds": set()})
                e["n"] += 1; e["true"] += good; e["seeds"].add(s)
                if good and s not in e.get("hit", set()):
                    e.setdefault("hit", set()).add(s); e["first"].append(al["t"] - t0)
        print(f"== {fault or 'normal week'}" + (f" (fault from {fmt(t0)}, expect {EXPECT[fault]})" if fault else ""))
        if not kinds:
            print("   no alert in any of 5 weeks")
        for k, e in sorted(kinds.items()):
            lag = f", detected {np.median(e['first']) / 60:.0f} min after the fault (median)" if e["first"] else ""
            print(f"   {k:<28} {e['n']:>3} alerts in {len(e['seeds'])}/5 weeks, {e['true']} true{lag}")
