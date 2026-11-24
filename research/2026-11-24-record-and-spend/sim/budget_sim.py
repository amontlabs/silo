#!/usr/bin/env python3
"""Replay a synthetic overnight trace of model calls under several budget policies.

Research only, 24 November 2026. Not Silo code. Run: python3 budget_sim.py
Prices are Anthropic list prices read on 24 November 2026
(https://platform.claude.com/docs/en/about-claude/pricing).

A call is one model request. Its true cost is known only when it ends. A policy decides,
when a call wants to leave, whether it leaves, and (for the cut policy) whether a running
call is cut. Agent programs stop the conversation that gets a denial (the error shape Silo
should send makes Claude Code and Codex stop, not retry).
"""
import heapq
import math
import random
import statistics
import sys

# $ per million tokens: input, 5m cache write, 1h cache write, cache read, output
PRICES = {
    "opus-5-5": (4.0, 5.0, 8.0, 0.20, 20.0),
    "fable-5-2": (8.0, 10.0, 16.0, 0.20, 40.0),
    "fable-5-2-fast": (16.0, 20.0, 32.0, 0.40, 80.0),  # fast price x cache multipliers
}

THINK_SHARE = 0.4      # share of output that is thinking, streamed as empty text (display omitted)
TOK_PER_S = 60.0       # output speed
POST_ABORT_TOKENS = 180  # tokens billed after a cut, before the provider stops (tianpan.co, 2026)


class Call:
    __slots__ = ("conv", "t", "read", "write5", "write1h", "fresh", "out", "max_tokens",
                 "est_err", "start", "end", "cost", "cut", "idx")

    def __init__(self, conv, t, read, write5, write1h, fresh, out, max_tokens, est_err):
        self.conv, self.t = conv, t
        self.read, self.write5, self.write1h, self.fresh = read, write5, write1h, fresh
        self.out, self.max_tokens, self.est_err = out, max_tokens, est_err
        self.cut = False

    def input_cost(self, p):
        return (self.fresh * p[0] + self.write5 * p[1] + self.write1h * p[2] + self.read * p[3]) / 1e6

    def cost_at(self, p, out_tokens):
        return self.input_cost(p) + out_tokens * p[4] / 1e6

    def duration(self):
        return 3.0 + self.out / TOK_PER_S


def make_trace(seed, n_parallel, max_tokens, hours=8.0):
    """Return a list of conversations; each is a list of call templates spaced by tool time."""
    rng = random.Random(seed)
    convs = []

    def out_tokens():
        # lognormal, median about 1,500 tokens, long tail; capped at max_tokens
        return int(min(max_tokens, max(50, rng.lognormvariate(math.log(1500), 1.1))))

    def conversation(start, n_calls, ctx0, gap_mean, ttl1h=False, big_first=False):
        calls, t, ctx = [], start, ctx0
        last_end = None
        for i in range(n_calls):
            delta = int(rng.lognormvariate(math.log(4000), 0.9))
            if i == 0:
                prefix = ctx + (250_000 if big_first else 0)
                w1 = prefix if ttl1h else 0
                w5 = 0 if ttl1h else prefix
                c = dict(t=t, read=0, write5=w5, write1h=w1, fresh=delta, out=out_tokens())
                ctx = prefix + delta
            else:
                expired = last_end is not None and (t - last_end) > (3600 if ttl1h else 300)
                if expired:
                    c = dict(t=t, read=0, write5=0 if ttl1h else ctx, write1h=ctx if ttl1h else 0,
                             fresh=delta, out=out_tokens())
                else:
                    c = dict(t=t, read=ctx, write5=0 if ttl1h else delta,
                             write1h=delta if ttl1h else 0, fresh=0, out=out_tokens())
                ctx += delta
            ctx += c["out"]
            if ctx > 400_000:          # the program compacts
                ctx = 60_000
            c["est_err"] = rng.uniform(0.9, 1.15)
            calls.append(c)
            dur = 3.0 + c["out"] / TOK_PER_S
            last_end = t + dur
            t = last_end + rng.expovariate(1.0 / gap_mean)
        return calls

    # main: one long conversation all night, 1-hour cache on its prefix
    convs.append(("main", conversation(0.0, 2000, 40_000, 60.0, ttl1h=True)))
    # bursts of parallel subagents
    t = rng.uniform(600, 1800)
    b = 0
    while t < hours * 3600:
        for k in range(n_parallel):
            convs.append((f"sub{b}.{k}", conversation(t + rng.uniform(0, 5), rng.randint(5, 30),
                                                       25_000, 12.0)))
        b += 1
        t += rng.expovariate(1.0 / 3600)
    # one call with a 250k fresh input written to the 1-hour cache, at 01:30
    convs.append(("bigread", conversation(5400.0, 3, 30_000, 20.0, ttl1h=True, big_first=True)))
    # runaway loop from 03:00: same failing test, 180k context, every 25 s
    convs.append(("runaway", conversation(3 * 3600.0, 4000, 180_000, 25.0)))
    out = []
    for name, cl in convs:
        cl = [c for c in cl if c["t"] < hours * 3600]
        objs = [Call(name, c["t"], c["read"], c["write5"], c["write1h"], c["fresh"],
                     c["out"], max_tokens, c["est_err"]) for c in cl]
        for i, o in enumerate(objs):
            o.idx = i
        out.append((name, objs))
    return out


def run(trace, policy, budget, model, rate_per_hour=None):
    """Replay. Returns metrics. policy in: finish, admit_input, reserve_max, reserve_input, cut."""
    p = PRICES[model]
    settled = 0.0
    inflight = {}          # id(call) -> call
    reserved = {}          # id(call) -> reservation
    stopped = set()        # conversations that got a denial
    hist = []              # (end_time, cost) for the rate window
    events = []            # (time, kind, seq, call)
    seq = 0
    for name, calls in trace:
        if calls:
            heapq.heappush(events, (calls[0].t, 0, seq, (name, 0)))
            seq += 1
    conv_calls = dict(trace)
    ticker = [False]
    m = dict(reached_at=None, spend=0.0, calls=0, denied=0, false_stops=0, wasted=0.0, first_denial_left=None,
             runaway_stopped_at=None, cut_calls=0)

    def counted_now(now):
        """What the boundary has counted of running calls: input at start, visible output so far."""
        tot = 0.0
        for c in inflight.values():
            elapsed = max(0.0, now - c.start - 3.0)
            vis = min(c.out, elapsed * TOK_PER_S) * (1 - THINK_SHARE)
            tot += c.cost_at(p, vis)
        return tot

    def reservation(c, kind):
        n_in = c.read + c.write5 + c.write1h + c.fresh
        est = n_in * c.est_err   # client-side token estimate, -10% to +15%
        if kind == "reserve_max":
            # every input token priced at the highest rate the request can incur, plus max_tokens
            return est * max(p[0], p[1], p[2]) / 1e6 + c.max_tokens * p[4] / 1e6
        # reserve_input: input priced as the previous turn's cache state predicts, plus p95 output
        prefix_read = c.read * c.est_err
        rest = est - prefix_read
        return prefix_read * p[3] / 1e6 + max(0.0, rest) * max(p[1], p[2], p[0]) / 1e6 \
            + min(c.max_tokens, 12_000) * p[4] / 1e6

    def rate_spent(now):
        return sum(cost for (t, cost) in hist if t > now - 3600)

    while events:
        now, kind, _, payload = heapq.heappop(events)
        if kind == 1:                      # a call ends
            c = payload
            if id(c) not in inflight:
                continue
            del inflight[id(c)]
            reserved.pop(id(c), None)
            if c.cut:
                continue
            cost = c.cost_at(p, c.out)
            settled += cost
            hist.append((now, cost))
            m["calls"] += 1
            if m.get("reached_at") is None and settled >= budget:
                m["reached_at"] = now
            # next call of the same conversation
            name = c.conv
            cl = conv_calls[name]
            idx = c.idx + 1
            if idx < len(cl) and name not in stopped:
                nxt = cl[idx]
                nxt.t = max(nxt.t, now + 1.0)
                heapq.heappush(events, (nxt.t, 0, seq, (name, idx)))
                seq += 1
            continue
        if kind == 2:                      # cut check for the cut policy
            ticker[0] = False
            if not inflight:
                continue
            if settled + counted_now(now) >= budget:
                for c in list(inflight.values()):
                    elapsed = max(0.0, now - c.start - 3.0)
                    billed_out = min(c.out, elapsed * TOK_PER_S + POST_ABORT_TOKENS)
                    cost = c.cost_at(p, billed_out)
                    settled += cost
                    m["wasted"] += cost
                    m["cut_calls"] += 1
                    c.cut = True
                    stopped.add(c.conv)    # the retry it sends is denied; the program stops
                    m["denied"] += 1
                    del inflight[id(c)]
            else:
                ticker[0] = True
                heapq.heappush(events, (now + 1.0, 2, seq, None))
                seq += 1
            continue
        # kind 0: a call wants to leave
        name, idx = payload
        if name in stopped:
            continue
        c = conv_calls[name][idx]
        if kind == 0 and c.t > now + 1e-9:
            continue
        if policy in ("finish", "cut"):
            ok = settled + counted_now(now) < budget
        elif policy == "admit_input":
            # the plan, plus: the request's own input, priced at the highest rate it can incur,
            # must fit in what remains
            n_in = (c.read + c.write5 + c.write1h + c.fresh) * c.est_err
            worst_in = (c.read * c.est_err * p[3] + (n_in - c.read * c.est_err)
                        * max(p[0], p[1], p[2])) / 1e6
            ok = settled + counted_now(now) + worst_in < budget
        else:
            r = reservation(c, policy)
            ok = settled + sum(reserved.values()) + r <= budget
        if ok and rate_per_hour is not None:
            ok = rate_spent(now) < rate_per_hour
        if not ok:
            m["denied"] += 1
            stopped.add(name)
            if name == "runaway" and m["runaway_stopped_at"] is None:
                m["runaway_stopped_at"] = now
            # a false stop: the call would have fit had the boundary known every true cost
            truth_left = budget - settled - sum(x.cost_at(p, x.out) for x in inflight.values())
            if c.cost_at(p, c.out) <= truth_left:
                m["false_stops"] += 1
                if m["first_denial_left"] is None:
                    m["first_denial_left"] = budget - settled
            continue
        c.start = now
        inflight[id(c)] = c
        if policy in ("reserve_max", "reserve_input"):
            reserved[id(c)] = reservation(c, policy)
        heapq.heappush(events, (now + c.duration(), 1, seq, c))
        seq += 1
        if policy == "cut" and not ticker[0]:
            ticker[0] = True
            heapq.heappush(events, (now + 1.0, 2, seq, None))
            seq += 1
    m["spend"] = settled
    m["overshoot"] = max(0.0, settled - budget)
    return m


def worst_case_table():
    print("Worst case under 'running calls finish', one call that leaves just under the limit:")
    print(f"{'model':16} {'input':>9} {'max_out':>8} {'one call $':>11} {'x4 $':>8} {'x16 $':>8}")
    for model in PRICES:
        p = PRICES[model]
        for n_in, mx in ((200_000, 64_000), (200_000, 128_000), (1_000_000, 128_000)):
            one = n_in * p[2] / 1e6 + mx * p[4] / 1e6   # all input written to the 1-hour cache
            print(f"{model:16} {n_in:>9,} {mx:>8,} {one:>11.2f} {4*one:>8.2f} {16*one:>8.2f}")
    print()


def main():
    seeds = range(int(sys.argv[1]) if len(sys.argv) > 1 else 20)
    worst_case_table()
    policies = ["finish", "admit_input", "reserve_max", "reserve_input", "cut"]
    for model, budget in (("opus-5-5", 50.0), ("fable-5-2-fast", 50.0)):
        for n_par, mx in ((4, 64_000), (8, 64_000), (16, 128_000)):
            print(f"== {model}, budget ${budget:.0f}, {n_par} parallel subagents, max_tokens {mx:,}"
                  f" ({len(seeds)} nights)")
            print(f"{'policy':14} {'spend':>7} {'over med':>9} {'over max':>9} {'false stops':>11}"
                  f" {'left at 1st false stop':>23} {'cut $ wasted':>12} {'calls':>6}")
            for pol in policies:
                rs = [run(make_trace(s, n_par, mx), pol, budget, model) for s in seeds]
                over = [r["overshoot"] for r in rs]
                left = [r["first_denial_left"] for r in rs if r["first_denial_left"] is not None]
                print(f"{pol:14} {statistics.mean(r['spend'] for r in rs):7.2f}"
                      f" {statistics.median(over):9.2f} {max(over):9.2f}"
                      f" {statistics.mean(r['false_stops'] for r in rs):11.1f}"
                      f" {(statistics.median(left) if left else 0):23.2f}"
                      f" {statistics.mean(r['wasted'] for r in rs):12.2f}"
                      f" {statistics.mean(r['calls'] for r in rs):6.0f}")
            print()
    # the night without any budget
    print("== The same nights with no budget (opus-5-5, 8 parallel, max_tokens 64,000)")
    rs = [run(make_trace(s, 8, 64_000), "finish", 1e9, "opus-5-5") for s in seeds]
    print(f"spend over 8 hours: median {statistics.median(r['spend'] for r in rs):.2f},"
          f" min {min(r['spend'] for r in rs):.2f}, max {max(r['spend'] for r in rs):.2f}")
    rs_nr = [run([c for c in make_trace(s, 8, 64_000) if c[0] != "runaway"], "finish", 1e9,
                 "opus-5-5") for s in seeds]
    print(f"of which the runaway loop from 03:00: median"
          f" {statistics.median(a['spend'] - b['spend'] for a, b in zip(rs, rs_nr)):.2f}")
    print()
    # a rate on top of the total: when is the runaway loop stopped?
    print("== Runaway loop from 03:00, opus-5-5, 8 parallel: total $150, with and without a rate")
    for rate in (None, 15.0):
        rs = [run(make_trace(s, 8, 64_000), "finish", 150.0, "opus-5-5", rate) for s in seeds]
        at = [r["runaway_stopped_at"] for r in rs if r["runaway_stopped_at"]]
        reached = [r.get("reached_at") for r in rs if r.get("reached_at")]
        label = "total only" if rate is None else f"+ ${rate:.0f}/hour"
        when = (f"{statistics.median(at)/3600:.2f} h" if at else "not stopped")
        rt = f"{statistics.median(reached)/3600:.2f} h" if reached else "not reached"
        print(f"{label:12} runaway stopped at median {when}; total reached at median {rt};"
              f" night spend {statistics.mean(r['spend'] for r in rs):.2f};"
              f" denied conversations {statistics.mean(r['denied'] for r in rs):.1f}")


if __name__ == "__main__":
    main()
