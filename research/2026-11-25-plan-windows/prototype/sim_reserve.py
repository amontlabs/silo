"""A reserve on a plan's window ("stop this account's Silo computers once the week is 70% used")
with several Silo computers on several Macs, and devices outside Silo on the same account.

Each Silo computer runs agents that keep `conc` model calls under way. A call lasts 20 to 120 s
and adds its weight to the window when it ends; the provider's figure counts it after an
accounting delay of up to `delay` seconds and is sent, rounded down to `step`, on the reply of
each call. Devices outside Silo spend in the same window at a share `outside` of Silo's rate.

A boundary admits a call when the newest figure it knows is under the level. Three ways of
knowing it:
  own     the figure on the last reply to that computer (each computer alone)
  shared  the newest figure any Silo computer of the account saw (Silo's computers tell each
          other, through the app on one Mac or #388's lease branch across Macs)
  hold    shared, and each call under way also holds its typical weight, as #374's
          reserve_typical does for dollars
  recent  hold, and each Silo call that ended less than `delay` ago also counts, at the weight
          Silo read from its reply, since the figure may not hold it yet. Silo reads dollars,
          not points of the window: a point's price is #355's estimate k. `recent` takes k
          exact, `recent/2` half the truth (its own calls look half as heavy) and `recent*2`
          twice the truth
Printed, for a level of 70%: the true window when Silo's last call ended, counting only Silo's
own spend after the true window passed the level (devices outside Silo are not Silo's to stop),
and the true window when Silo stopped admitting calls (under 70: stopped early).
usage: sim_reserve.py
"""
import heapq, random, statistics

LEVEL = 0.70


def run(n_comp, conc, outside, delay, step, mode, seed):
    rng = random.Random(seed)
    cap = 1.0
    true = 0.55                      # the week starts at 55%
    counted = []                     # (time it shows in the figure, weight)
    typical = 0.0009                 # one call's weight: 0.09% of the week (about $2 of Opus)
    known = [0.55] * n_comp
    shared = [0.55]
    t, ev = 0.0, []
    under_way = [0] * n_comp
    held = [0.0]
    recent = []                      # (end time, weight) of Silo's own calls
    crossed = [None]
    silo_after = [0.0]
    last_admit = [0.55]

    def figure(now):
        v = 0.55 + sum(w for (ts, w) in counted if ts <= now)
        return int(v / step) * step

    def try_start(c, now):
        k = known[c] if mode == "own" else shared[0]
        if mode == "hold" or mode.startswith("recent"):
            k = k + held[0] + typical
        if mode.startswith("recent"):
            kf = {"recent": 1.0, "recent/2": 0.5, "recent*2": 2.0}[mode]
            k = k + kf * sum(w for (te, w) in recent if te > now - delay)
        if k >= LEVEL:
            return False
        w = typical * rng.lognormvariate(0, 0.6)
        dur = rng.uniform(20, 120)
        heapq.heappush(ev, (now + dur, "end", c, w))
        under_way[c] += 1
        held[0] += typical
        last_admit[0] = true
        return True

    for c in range(n_comp):
        for _ in range(conc):
            try_start(c, 0.0)
    # outside devices: a Poisson stream of calls, at `outside` times Silo's starting rate
    silo_rate = n_comp * conc / 70.0
    if outside > 0:
        heapq.heappush(ev, (rng.expovariate(silo_rate * outside), "out", -1, 0))
    stopped_at = None
    while ev:
        now, kind, c, w = heapq.heappop(ev)
        if kind == "out":
            if stopped_at is not None and now > stopped_at + 600:
                continue
            ow = typical * rng.lognormvariate(0, 0.6)
            true += ow
            if crossed[0] is None and true >= LEVEL:
                crossed[0] = now
            counted.append((now + rng.uniform(0, delay), ow))
            heapq.heappush(ev, (now + rng.expovariate(silo_rate * outside), "out", -1, 0))
            continue
        true += w
        if crossed[0] is not None:
            silo_after[0] += w
        elif true >= LEVEL:
            crossed[0] = now
            silo_after[0] += true - LEVEL
        recent.append((now, w))
        counted.append((now + rng.uniform(0, delay), w))
        under_way[c] -= 1
        held[0] -= typical
        f = figure(now)
        known[c] = f
        shared[0] = max(shared[0], f)
        if not try_start(c, now):
            if sum(under_way) == 0 and stopped_at is None:
                stopped_at = now
        # restart other idle computers' slots only through their own replies, as real agents do
    return LEVEL + silo_after[0] if crossed[0] is not None else None, last_admit[0]


def table():
    print("Level 70%. Each cell: median (min to max) over 20 seeds of the true week when Silo's last call")
    print("ended, Silo's spend only; then, after /, the median true week at Silo's last admitted call.\n")
    modes = ["own", "shared", "hold", "recent", "recent/2", "recent*2"]
    print(f"{'computers':>9} {'calls':>5} {'outside':>7} {'delay':>5} | " + " | ".join(f"{m:>26}" for m in modes))
    for n, conc in [(1, 4), (3, 4), (6, 8)]:
        for outside in [0.0, 1.0]:
            for delay in [0, 120, 900]:
                row = []
                for mode in modes:
                    r = [run(n, conc, outside, delay, 0.0001, mode, s) for s in range(20)]
                    over = [x[0] * 100 for x in r if x[0] is not None]
                    adm = statistics.median(x[1] * 100 for x in r)
                    if over:
                        row.append(f"{statistics.median(over):4.1f} ({min(over):4.1f}-{max(over):4.1f}) / {adm:4.1f}")
                    else:
                        row.append(f"never reached / {adm:4.1f}")
                print(f"{n:>9} {conc:>5} {outside:>7} {delay:>4}s | " + " | ".join(f"{x:>26}" for x in row))


if __name__ == "__main__":
    table()
