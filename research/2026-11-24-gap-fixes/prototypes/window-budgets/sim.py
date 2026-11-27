"""Simulator: a budget counted as a share of a subscription's weekly window.

One account, two clients that spend from the same windows:
- the computer (what Silo's boundary sees): a scheduled job loop day and night (the #110395 shape)
  plus agent sessions in working hours;
- the laptop (invisible to Silo): interactive sessions in working hours, at a chosen intensity.

The provider counts each call in its own hidden weights (which differ from API prices, and give
headless calls a 1.86x weight, the #111890 measurement), reports the account's utilization after
each response, rounded to a quantum, with an accounting lag. Silo sees only the computer's calls
(token counts, its own API-price cost) and the reported utilization on the computer's responses.

Estimators of the computer's share of the week:
  E1 span deltas : sum of utilization increases between consecutive own responses less than 2 min apart.
  E2 envelope    : utilization per priced dollar, k, is the 20th percentile of (increase / own cost),
                   weighted by own cost, over spans of own calls in the last 48 h; last week's k
                   (with a drift) until this week has spans; share = k * own cost this week.
                   Other devices only ever add to an increase, so the low ratios are the clean ones.
  E3 per kind    : increase = sum over token kinds (input, output, cache read, 5m and 1h writes)
                   of a weight times the own priced cost of that kind, fitted non-negative over the
                   spans of the last 48 h with positive residuals down-weighted (other devices only
                   add); share = the fitted weights applied to own costs this week.
  E4 provider    : the provider reports each thread's share itself (Codex's thread_usage/query_v2);
                   exact up to its lag. Shown as the reference a rule could rest on.
Two budgets are checked against the truth:
  share budget   : "this computer may use 30% of the week", enforced on each estimate.
  reserve rule   : "this computer's calls stop once the week is 70% used", on the reported figure.

Run: python3 sim.py            (prints the tables in the doc; about a minute)
     python3 sim.py --quick    (fewer seeds)
"""
import math, random, sys
import numpy as np

DAY, WEEK = 86400.0, 7 * 86400.0
GAP = 600.0   # own calls closer than this belong to one span of activity
# Silo's prices, dollars per million tokens (Claude Opus 5.5 and Sonnet 5.5, from the claude-api reference)
PRICE = {"opus": {"in": 4.0, "out": 20.0, "cr": 0.20, "w5": 5.0, "w1": 8.0},
         "sonnet": {"in": 2.0, "out": 10.0, "cr": 0.10, "w5": 2.5, "w1": 4.0}}
# The provider's hidden weights per token kind, relative to Silo's price (made up, to be wrong on purpose)
HIDDEN = {"in": 1.0, "out": 1.0, "cr": 2.0, "w5": 1.0, "w1": 0.8}


KINDS = ["in", "out", "cr", "w5", "w1"]


def priced_kinds(u, model):
    p = PRICE[model]
    return [p[k] * u[k] / 1e6 for k in KINDS]


def nnls(A, b, iters=100):
    """Non-negative least squares by coordinate descent (small problems only)."""
    x = np.zeros(A.shape[1]); AtA = A.T @ A; Atb = A.T @ b
    for _ in range(iters):
        for j in range(len(x)):
            if AtA[j, j] > 0:
                x[j] = max(0.0, x[j] + (Atb[j] - AtA[j] @ x) / AtA[j, j])
    return x


def priced(u, model):
    p = PRICE[model]
    return sum(p[k] * u[k] for k in p) / 1e6


def hidden_cost(u, model, headless):
    p = PRICE[model]
    return sum(p[k] * HIDDEN[k] * u[k] for k in p) / 1e6 * (1.86 if headless else 1.0)


class Conversation:
    """Token counts of one agent conversation, turn by turn, with a 1h or 5m cache."""

    def __init__(self, rng, start_ctx, ttl):
        self.rng, self.ctx, self.ttl, self.last = rng, start_ctx, ttl, None

    def turn(self, t):
        r = self.rng
        new = r.lognormvariate(math.log(1500), 1.0)          # tool result and the model's last reply
        out = r.lognormvariate(math.log(250), 0.8)
        miss = self.last is None or t - self.last > self.ttl
        u = {"in": 5, "out": out, "cr": 0.0, "w5": 0.0, "w1": 0.0}
        kind = "w1" if self.ttl == 3600 else "w5"
        if miss:
            u[kind] = self.ctx + new
        else:
            u["cr"], u[kind] = self.ctx, new
        self.ctx += new + out
        if self.ctx > 160_000:                                # compaction
            self.ctx = 25_000
        self.last = t
        return u


def gen_client(rng, who, intensity, t_end):
    """Calls of one client: list of (t, who, usage, model, headless)."""
    calls = []
    if who == "computer":
        # scheduled job loop: a run every ~150 s, 3 turns, 40k context (#110395: 580 runs a day)
        t = rng.uniform(0, 150)
        while t < t_end:
            c = Conversation(rng, 40_000, 3600)
            tt = t
            for _ in range(3):
                calls.append((tt, who, c.turn(tt), "sonnet", True)); tt += rng.lognormvariate(math.log(8), 0.6)
            t += rng.lognormvariate(math.log(150), 0.3)
        sessions_per_day, model = 3, "opus"
    else:
        sessions_per_day, model = 4 * intensity, "opus"
        if intensity == 0:
            return calls
    # agent sessions in working hours (09:00 to 19:00), Monday to Friday
    for d in range(int(t_end // DAY) + 1):
        if d % 7 in (5, 6):
            continue
        n = np.random.default_rng(rng.randrange(1 << 30)).poisson(sessions_per_day)
        for _ in range(n):
            t = d * DAY + rng.uniform(9, 18) * 3600
            c = Conversation(rng, 30_000, 3600)
            for _ in range(int(rng.lognormvariate(math.log(40), 0.6))):
                if t >= t_end:
                    break
                calls.append((t, who, c.turn(t), model, who == "computer"))
                t += rng.lognormvariate(math.log(12), 0.9)
                if rng.random() < 0.03:
                    t += rng.uniform(300, 1500)               # a pause: you read, think, come back
    return calls


def run(seed, intensity, quantum, lag, budget=0.30, reserve=0.70, cut_at=None, q2=0.2, close_q=6, lookback=2 * DAY):
    rng = random.Random(seed)
    comp = gen_client(rng, "computer", 1, WEEK)
    lap = []
    if intensity:
        probe = gen_client(random.Random(seed + 999), "laptop", 1.0, WEEK)
        per_unit = sum(hidden_cost(u, m, h) for t, w, u, m, h in probe)
        target = intensity * sum(hidden_cost(u, m, h) for t, w, u, m, h in comp)
        lap = gen_client(rng, "laptop", target / per_unit, WEEK)
    calls = comp + lap
    calls.sort(key=lambda c: c[0])
    comp_week = sum(hidden_cost(u, m, h) for t, w, u, m, h in calls if w == "computer")
    total_week = sum(hidden_cost(u, m, h) for t, w, u, m, h in calls)
    cap = max(comp_week / 0.6, total_week / 0.97)            # the week's capacity, in hidden dollars

    # provider: an accounting queue; utilization at time t counts calls accounted by t
    acct_t = np.array([c[0] + (rng.uniform(0, lag) if lag else 0.0) for c in calls])
    costs = np.array([hidden_cost(c[2], c[3], c[4]) for c in calls])
    if cut_at is not None:                                    # the plan's week is cut by 17% mid-week
        cap_at = lambda t: cap * (0.83 if t >= cut_at else 1.0)
    else:
        cap_at = lambda t: cap
    order = np.argsort(acct_t); acct_sorted = acct_t[order]; cum_sorted = np.cumsum(costs[order])

    def reported(t):
        i = np.searchsorted(acct_sorted, t, side="right")
        v = (cum_sorted[i - 1] if i else 0.0) / cap_at(t)
        return math.floor(v / quantum) * quantum if quantum else v

    own = [(c[0], c[2], c[3], c[4], costs[i]) for i, c in enumerate(calls) if c[1] == "computer"]
    true_share = np.cumsum([o[4] for o in own]) / np.array([cap_at(o[0]) for o in own])
    t_resp = [o[0] + 2.0 for o in own]                        # the response ends 2 s after it starts
    u_obs = [reported(t) for t in t_resp]
    price = [priced(o[1], o[2]) for o in own]
    cum_price = np.cumsum(price)
    cum_kind = np.cumsum(np.array([priced_kinds(o[1], o[2]) for o in own]), axis=0)
    own_t = np.array([o[0] for o in own])
    # last week's k: this week's true ratio, off by a drift between -20% and +25%
    # (#104967 saw a point of the week fall from $4.45 to $2.31 in a month)
    k_prior = float(true_share[-1] / cum_price[-1]) * rng.uniform(0.8, 1.25)

    est = {"E1": [], "E2": [], "E3": [], "E4": []}
    e1 = 0.0
    spans = []          # (t_end, d_u, d_price, duration)
    span_start = 0
    k2 = k3 = None
    close_at = max(close_q * quantum, 0.003)
    for i in range(len(own)):
        gap = t_resp[i] - t_resp[i - 1] if i else 1e9
        if gap <= GAP:
            e1 += max(0.0, u_obs[i] - u_obs[i - 1])
        # a span is a run of own calls with no gap over GAP; it closes once the reported figure
        # has moved by 6 quanta (so the rounding is a sixth of it at most) or after 6 hours
        if gap > GAP:
            span_start = i
        elif u_obs[i] - u_obs[span_start] >= close_at or t_resp[i] - t_resp[span_start] > 6 * 3600:
            d_u = u_obs[i] - u_obs[span_start]; d_p = cum_price[i] - cum_price[span_start]
            if d_p > 0 and d_u >= 0:
                spans.append((t_resp[i], d_u, d_p, cum_kind[i] - cum_kind[span_start]))
            span_start = i
        if i % 200 == 0:
            recent = [x for x in spans if x[0] > t_resp[i] - lookback]
            if len(recent) >= 4:
                # the 20th percentile weighted by own spend: the ratio below which 20% of the
                # computer's own spend was measured (a count of spans would let the many short,
                # busy daytime spans outvote the few long, clean night ones)
                ratios = np.array([x[1] / x[2] for x in recent]); wts = np.array([x[2] for x in recent])
                o = np.argsort(ratios); cw = np.cumsum(wts[o]) / wts.sum()
                k2 = float(ratios[o][np.searchsorted(cw, q2)])
                # one weight per token kind, fitted so that spans with other devices in them
                # (positive residuals) count for less: a lower envelope per kind
                X = np.array([x[3] for x in recent]); y = np.array([x[1] for x in recent])
                w = np.ones(len(y))
                for _ in range(8):
                    sol = nnls(X * w[:, None], y * w)
                    res = y - X @ sol
                    sc = np.median(np.abs(res)) + 1e-12
                    w = np.where(res > 0, 1.0 / (1.0 + (res / sc) ** 2), 1.0)
                if sol.sum() > 0:
                    k3 = sol
        est["E1"].append(e1)
        # before this week's spans give a k, an estimate uses last week's, which drifts
        est["E2"].append((k2 if k2 is not None else k_prior) * cum_price[i])
        est["E3"].append(float(cum_kind[i] @ k3) if k3 is not None else k_prior * cum_price[i])
        # E4: the provider's own per-thread figure, 5 minutes stale, at the same quantum
        j = int(np.searchsorted(own_t, own[i][0] - 300, side="right"))
        v = true_share[j - 1] if j else 0.0
        est["E4"].append(math.floor(v / quantum) * quantum if quantum else v)

    result = {"share_true_end": float(true_share[-1]), "account_end": reported(WEEK)}
    for name, e in est.items():
        e = np.array(e)
        err = e - true_share
        result[name + "_err_mid"] = float(err[len(err) // 2])
        result[name + "_err_end"] = float(err[-1])
        idx = np.argmax(e >= budget) if (e >= budget).any() else None
        # enforced level: the true share when the estimate says 30%, plus the calls the
        # computer sends before the next response tells Silo (none here: denial is immediate)
        result[name + "_enforced"] = float(true_share[idx]) if idx is not None else float(true_share[-1])
        result[name + "_fired"] = idx is not None
    # reserve rule: deny the computer's next call once a reported week utilization >= 70%
    k = next((i for i, u in enumerate(u_obs) if u >= reserve), None)
    if k is not None:
        # what the account really reaches by the time the rule fired, and once everything is accounted
        t_fire = t_resp[k]
        truth_at_fire = float(np.sum(costs[np.array([c[0] for c in calls]) <= t_fire]) / cap_at(t_fire))
        result["reserve_true_at_fire"] = truth_at_fire
    else:
        result["reserve_true_at_fire"] = None
    result["n_own"] = len(own); result["n_all"] = len(calls)
    return result


def table(rows, cols):
    w = [max(len(c), 10) for c in cols]
    print("  ".join(c.ljust(x) for c, x in zip(cols, w)))
    for r in rows:
        print("  ".join(str(v).ljust(x) for v, x in zip(r, w)))


def pct(x):
    return "-" if x is None else f"{100 * x:+.1f}" if x < 0 or x > 0 else "0.0"


if __name__ == "__main__":
    seeds = range(3) if "--quick" in sys.argv else range(6)
    print("A budget of 30% of the week on the computer. 'enforced' is the computer's true share of the")
    print("week at the moment the estimate reached 30%: median (min..max) over the seeds.\n")
    for quantum, lag in [(0.01, 0.0), (0.01, 120.0), (0.0001, 0.0), (0.0001, 120.0)]:
        print(f"== reported to {'1 point' if quantum == 0.01 else '0.01 point'}, accounting lag up to {int(lag)} s")
        rows = []
        for inten in [0.0, 0.5, 1.0, 2.0]:
            rs = [run(s, inten, quantum, lag) for s in seeds]
            row = [f"laptop x{inten}"]
            for e in ["E1", "E2", "E3", "E4"]:
                enf = [r[e + "_enforced"] for r in rs]
                row.append(f"{100*np.median(enf):.1f} ({100*min(enf):.1f}..{100*max(enf):.1f})")
            rows.append(row)
        table(rows, ["interleave", "E1 enforced", "E2 enforced", "E3 enforced", "E4 enforced"])
        print()
    print("== reserve rule: the computer stops once the reported week reaches 70%")
    rows = []
    for quantum, lag in [(0.01, 0.0), (0.01, 120.0), (0.01, 900.0)]:
        for inten in [0.5, 2.0]:
            rs = [run(s, inten, quantum, lag, reserve=0.70) for s in seeds]
            v = [r["reserve_true_at_fire"] for r in rs if r["reserve_true_at_fire"] is not None]
            rows.append([f"q={quantum} lag={int(lag)}s", f"laptop x{inten}", f"{len(v)}/{len(rs)}",
                         f"{100*np.median(v):.2f}" if v else "-", f"{100*max(v):.2f}" if v else "-"])
    table(rows, ["report", "interleave", "fired", "true at fire (median)", "true at fire (max)"])
    print()
    print("== the plan's week cut by 17% on day 3 (reported to 1 point, no lag)")
    rows = []
    for inten in [0.5, 1.0]:
        rs = [run(s, inten, 0.01, 0.0, cut_at=3 * DAY) for s in seeds]
        row = [f"laptop x{inten}"]
        for e in ["E1", "E2", "E3"]:
            enf = [r[e + "_enforced"] for r in rs]
            row.append(f"{100*np.median(enf):.1f} ({100*min(enf):.1f}..{100*max(enf):.1f})")
        rows.append(row)
    table(rows, ["interleave", "E1 enforced", "E2 enforced", "E3 enforced"])
