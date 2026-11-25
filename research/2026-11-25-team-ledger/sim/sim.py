"""A team's budget held across several Macs: a discrete-event simulation.

Each Mac runs agents like #374's overshoot run (Claude Code, a main conversation of 40 steps that
starts 8 subagents of 14 steps at step 2, 2,000 to 12,000 tokens of hidden reasoning a call) and has
its own boundary, which counts with #374's rule (`ledger/book.py`). The ways to share one budget:

  exact    one book sees every call at once, no delay: one boundary, or computers on one host
  central  a ledger admits every call; each admission is a round trip
  lease    a ledger gives each Mac a slice; the Mac admits locally and asks for more when low
  gossip   no ledger; each Mac tells the others what it has counted every few seconds
  static   each Mac has a fixed share and nobody talks

Run: python3 sim.py --help
"""
import argparse, heapq, json, os, random, statistics, sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "ledger"))
from book import Book, POLICIES  # noqa: E402

# Opus 5.5, dollars per token, from #374's prices.json.
P_IN, P_OUT, P_CR, P_CW = 4e-6, 20e-6, 0.2e-6, 5e-6
THINK = (2000, 12000)
TYPICAL_OUT = THINK[0] + 0.9 * (THINK[1] - THINK[0]) + 120  # the 90th percentile of output


# ---- a small event loop ----

class Loop:
    def __init__(self):
        self.t, self.q, self.n = 0.0, [], 0

    def at(self, t, fn):
        self.n += 1
        heapq.heappush(self.q, (t, self.n, fn))

    def after(self, dt, fn):
        self.at(self.t + dt, fn)

    def run(self, until):
        while self.q and self.q[0][0] <= until:
            t, _, fn = heapq.heappop(self.q)
            self.t = t
            fn()

    def spawn(self, gen):
        self.after(0, lambda: self._step(gen, None))

    def _step(self, gen, val):
        try:
            y = gen.send(val)
        except StopIteration:
            return
        if isinstance(y, Future):
            y.then(lambda v: self._step(gen, v))
        else:
            self.after(y, lambda: self._step(gen, None))


class Future:
    def __init__(self, loop):
        self.loop, self.done, self.v, self.cbs = loop, False, None, []

    def resolve(self, v=None):
        if self.done:
            return
        self.done, self.v = True, v
        for cb in self.cbs:
            self.loop.after(0, lambda cb=cb: cb(v))
        self.cbs = []

    def then(self, cb):
        if self.done:
            self.loop.after(0, lambda: cb(self.v))
        else:
            self.cbs.append(cb)


# ---- the team's spend, as the provider bills it ----

class Bill:
    def __init__(self):
        self.calls = []  # (mac, start, end, cost)

    def spent_by(self, t):
        """What the provider has billed by time t, each call spread evenly over its length (#374's
        'truly reached')."""
        s = 0.0
        for _, a, b, c in self.calls:
            if t >= b:
                s += c
            elif t > a:
                s += c * (t - a) / (b - a)
        return s

    def total(self):
        return sum(c[3] for c in self.calls)


# ---- ways to share a budget ----

class Way:
    """admit() returns a Future of True or False; started() and settled() tell the way about a call."""
    waits: list

    def __init__(self, loop, team, budget, policy, tps):
        self.loop, self.team, self.budget, self.policy, self.tps = loop, team, budget, policy, tps
        self.waits = []  # seconds each admitted call waited for its answer
        self.messages = 0

    def book(self, limit):
        return Book(limit, self.policy, self.tps, P_OUT)

    def done(self, mac):
        pass


class Exact(Way):
    def __init__(self, *a):
        super().__init__(*a)
        self.b = self.book(self.budget)

    def admit(self, mac, cid, inp):
        f = Future(self.loop)
        res = self.b.reserve_for(inp, TYPICAL_OUT)
        ok = self.b.admits(self.loop.t, inp, res)
        if ok:
            self.b.start(cid, self.loop.t, inp, res)
            self.waits.append(0.0)
        f.resolve(ok)
        return f

    def settled(self, mac, cid, cost):
        self.b.settle(cid, cost)


class Static(Way):
    def __init__(self, *a, shares=None):
        super().__init__(*a)
        n = len(self.team)
        shares = shares or [1 / n] * n
        self.books = [self.book(self.budget * s) for s in shares]

    def admit(self, mac, cid, inp):
        b = self.books[mac]
        f = Future(self.loop)
        res = b.reserve_for(inp, TYPICAL_OUT)
        ok = b.admits(self.loop.t, inp, res)
        if ok:
            b.start(cid, self.loop.t, inp, res)
            self.waits.append(0.0)
        f.resolve(ok)
        return f

    def settled(self, mac, cid, cost):
        self.books[mac].settle(cid, cost)


class Down:
    """When the ledger cannot be reached: a list of (from, to) intervals."""

    def __init__(self, spans):
        self.spans = spans or []

    def up_at(self, t):
        for a, b in self.spans:
            if a <= t < b:
                return b
        return t


class Central(Way):
    """Every admission is a round trip to the ledger. The ledger counts a running call's output by
    time from the moment it admitted it, so no message is needed while the call runs.
    `rtt` is seconds, one number or one per Mac. `down` makes the ledger unreachable; `on_down`
    is `hold` (the call waits for the ledger) or `deny` (the call is refused)."""

    def __init__(self, *a, rtt=0.03, down=None, on_down="hold"):
        super().__init__(*a)
        self.b = self.book(self.budget)
        self.rtt = rtt if isinstance(rtt, list) else [rtt] * len(self.team)
        self.down, self.on_down = Down(down), on_down

    def admit(self, mac, cid, inp):
        f, t0, half = Future(self.loop), self.loop.t, self.rtt[mac] / 2
        self.messages += 2

        def at_ledger():
            up = self.down.up_at(self.loop.t)
            if up > self.loop.t:
                if self.on_down == "deny":
                    self.loop.after(half, lambda: f.resolve(False))  # the boundary times out
                    return
                self.loop.at(up, at_ledger)
                return
            res = self.b.reserve_for(inp, TYPICAL_OUT)
            ok = self.b.admits(self.loop.t, inp, res)
            if ok:
                self.b.start(cid, self.loop.t, inp, res)

            def back():
                if ok:
                    self.waits.append(self.loop.t - t0)
                f.resolve(ok)
            self.loop.after(half, back)
        self.loop.after(half, at_ledger)
        return f

    def settled(self, mac, cid, cost):
        self.messages += 1

        def at_ledger():
            up = self.down.up_at(self.loop.t)
            if up > self.loop.t:
                self.loop.at(up, at_ledger)
            else:
                self.b.settle(cid, cost)
        self.loop.after(self.rtt[mac] / 2, at_ledger)


class Lease(Way):
    """The ledger hands out slices of the budget. Each Mac admits locally, with #374's rule, against
    the slices it holds, and asks for more when what it has left falls under `low`. A grant is
    half of what is free divided among the Macs that asked recently, at least `chunk` (or all that
    is free), so a lone heavy Mac gets most of the budget and a crowd shares it. A Mac whose agents
    have all ended gives back what it did not spend."""

    def __init__(self, *a, rtt=0.03, chunk=2.0, low=1.0, down=None, recall=True, share=2):
        super().__init__(*a)
        self.recall, self.share = recall, share
        n = len(self.team)
        self.rtt = rtt if isinstance(rtt, list) else [rtt] * n
        self.chunk, self.low = chunk, low
        self.down = Down(down)
        self.books = [self.book(0.0) for _ in range(n)]
        self.granted = [0.0] * n     # what the ledger has given each Mac
        self.reported = [0.0] * n    # what each Mac said it spent, with its last message
        self.returned = [0.0] * n
        self.asked_at = [-1e9] * n
        self.pending = [None] * n    # the Future of an ask under way
        self.empty = [False] * n     # the ledger's last answer was: nothing left

    def free(self):
        used = sum(max(g - r, s) for g, r, s in zip(self.granted, self.returned, self.reported))
        return max(0.0, self.budget - used)

    def ask(self, mac):
        if self.pending[mac]:
            return self.pending[mac]
        f = Future(self.loop)
        self.pending[mac] = f
        recalled = [False]
        half = self.rtt[mac] / 2
        spent_now = self.books[mac].settled
        self.messages += 2

        def at_ledger():
            up = self.down.up_at(self.loop.t)
            if up > self.loop.t:
                self.loop.at(up, at_ledger)
                return
            self.reported[mac] = max(self.reported[mac], spent_now)
            self.asked_at[mac] = self.loop.t
            if self.free() < self.chunk and not recalled[0] and self.recall:
                # Too little is free: take back the room other Macs hold and do not use, then
                # answer. Without this, a slice stays on a Mac that has stopped asking.
                recalled[0] = True
                longest = 0.0
                for j in range(len(self.team)):
                    if j == mac:
                        continue
                    hj = self.rtt[j] / 2
                    longest = max(longest, 2 * hj)
                    self.messages += 2
                    self.loop.after(hj, lambda j=j, hj=hj: self.give_back(j, hj))
                self.loop.after(longest, at_ledger)
                return
            active = sum(1 for t in self.asked_at if self.loop.t - t < 60)
            free = self.free()
            g = min(free, max(self.chunk, free / (self.share * max(1, active))))
            self.granted[mac] += g

            def back():
                self.books[mac].limit += g
                self.empty[mac] = g <= 1e-9
                self.pending[mac] = None
                f.resolve(g)
            self.loop.after(half, back)
        self.loop.after(half, at_ledger)
        return f

    def give_back(self, j, half):
        """At Mac j: hand back the room no call under way holds."""
        r = max(0.0, self.room(j))
        self.books[j].limit -= r
        if r > 0:
            self.empty[j] = False
        spent = self.books[j].settled

        def at_ledger():
            self.returned[j] += r
            self.reported[j] = max(self.reported[j], spent)
        self.loop.after(half, at_ledger)

    def room(self, mac):
        b = self.books[mac]
        return b.limit - b.held(self.loop.t)

    def admit(self, mac, cid, inp):
        f, t0, b = Future(self.loop), self.loop.t, self.books[mac]
        res = b.reserve_for(inp, TYPICAL_OUT)

        def attempt(_=None):
            if b.admits(self.loop.t, inp, res):
                b.start(cid, self.loop.t, inp, res)
                self.waits.append(self.loop.t - t0)
                if self.room(mac) < self.low and not self.empty[mac]:
                    self.ask(mac)  # ahead of need, so the next call does not wait
                f.resolve(True)
            elif self.empty[mac] and not self.pending[mac]:
                f.resolve(False)
            else:
                self.ask(mac).then(attempt)
        attempt()
        return f

    def settled(self, mac, cid, cost):
        self.books[mac].settle(cid, cost)

    def done(self, mac):
        b = self.books[mac]
        back = max(0.0, b.limit - b.settled)
        if back <= 0:
            return
        b.limit -= back
        self.messages += 1

        def at_ledger():
            up = self.down.up_at(self.loop.t)
            if up > self.loop.t:
                self.loop.at(up, at_ledger)
                return
            self.returned[mac] += back
            self.reported[mac] = max(self.reported[mac], b.settled)
        self.loop.after(self.rtt[mac] / 2, at_ledger)


class Gossip(Way):
    """No ledger. Every `every` seconds each Mac sends the others what it has counted, which arrives
    after `delay`. A Mac admits against its own count plus what it last heard from the others.
    `cut` = (from, to, group): during the span, Macs in `group` and the rest do not hear each other.
    With `split`, a Mac that has not heard from a peer for 3 intervals limits itself to its share
    of what was left when it last heard from everyone (an escrow, as the demarcation protocol of
    Barbara and Garcia-Molina does)."""

    def __init__(self, *a, every=1.0, delay=0.03, cut=None, split=False):
        super().__init__(*a)
        n = len(self.team)
        self.every, self.delay, self.cut, self.split = every, delay, cut, split
        self.books = [self.book(self.budget) for _ in range(n)]
        self.view = [[(0.0, 0.0)] * n for _ in range(n)]  # view[i][j] = (count, heard at)
        self.allowance = [None] * n
        self.on = [True] * n
        for i in range(n):
            self.loop.at(random.Random(i).random() * every, lambda i=i: self.tick(i))

    def cut_between(self, i, j, t):
        if not self.cut:
            return False
        a, b, group = self.cut
        return a <= t < b and ((i in group) != (j in group))

    def tick(self, i):
        if not self.on[i]:
            return
        b, t = self.books[i], self.loop.t
        c = b.held(t) if b.policy.startswith("reserve") else b.counted(t)
        for j in range(len(self.team)):
            if j != i and not self.cut_between(i, j, t):
                self.messages += 1
                self.loop.after(self.delay, lambda j=j, c=c: self.hear(j, i, c))
        self.loop.after(self.every, lambda: self.tick(i))

    def hear(self, j, i, c):
        old, _ = self.view[j][i]
        self.view[j][i] = (max(old, c), self.loop.t)

    def others(self, i):
        return sum(c for j, (c, _) in enumerate(self.view[i]) if j != i)

    def stale(self, i):
        return any(j != i and self.loop.t - t > 3 * self.every and self.on[j] for j, (_, t) in enumerate(self.view[i]))

    def admit(self, mac, cid, inp):
        f, b, now = Future(self.loop), self.books[mac], self.loop.t
        res = b.reserve_for(inp, TYPICAL_OUT)
        if self.split and now > 3 * self.every and self.stale(mac):
            if self.allowance[mac] is None:
                left = self.budget - self.others(mac) - b.counted(now)
                self.allowance[mac] = b.counted(now) + max(0.0, left) / len(self.team)
            ok = b.admits(now, inp, res, limit=self.allowance[mac])
        else:
            self.allowance[mac] = None
            ok = b.admits(now, inp, res, extra=self.others(mac))
        if ok:
            b.start(cid, now, inp, res)
            self.waits.append(0.0)
        f.resolve(ok)
        return f

    def settled(self, mac, cid, cost):
        self.books[mac].settle(cid, cost)

    def done(self, mac):
        # A Mac whose agents ended keeps telling the others its count: it may wake again.
        pass


WAYS = {"exact": Exact, "central": Central, "lease": Lease, "gossip": Gossip, "static": Static}


# ---- the agents on each Mac ----

KINDS = {"heavy": 8, "medium": 2, "light": 0}


class Team:
    def __init__(self, loop, way, members, seed, tps, main_steps=40, sub_steps=14, tool_s=(0.2, 1.0), stagger=30.0):
        self.loop, self.way, self.tps = loop, way, tps
        self.bill = Bill()
        self.rng = random.Random(seed)
        self.refused, self.calls, self.cid = 0, 0, 0
        self.first_refusal = None
        self.main_steps, self.sub_steps, self.tool_s = main_steps, sub_steps, tool_s
        self.live = [0] * len(members)
        self.end = None
        for mac, kind in enumerate(members):
            if kind == "idle":
                continue
            self.live[mac] += 1
            start = self.rng.uniform(0, stagger)
            loop.after(start, lambda mac=mac, kind=kind: loop.spawn(self.main(mac, KINDS[kind])))

    def ended(self, mac):
        self.live[mac] -= 1
        if self.live[mac] == 0:
            self.way.done(mac)
        if sum(self.live) == 0:
            self.end = self.loop.t
            self.loop.after(5, self.loop.q.clear)  # let the last messages land, then stop

    def call(self, mac, conv):
        """One model call: True if it left and ended, False if it was refused."""
        rng = self.rng
        new = rng.randint(200, 600)
        if conv["first"]:
            inp = (conv["ctx"] + new) * P_CW
            conv["first"] = False
        else:
            inp = conv["ctx"] * P_CR + new * P_CW
        conv["ctx"] += new + 80
        think, out = rng.randint(*THINK), rng.randint(40, 120)
        self.cid += 1
        cid = (mac, self.cid)
        ok = yield self.way.admit(mac, cid, inp)
        if not ok:
            self.refused += 1
            if self.first_refusal is None:
                self.first_refusal = self.loop.t
            return False
        t0 = self.loop.t
        dur = 0.3 + (think + out) / self.tps
        yield dur
        cost = inp + (think + out) * P_OUT
        self.bill.calls.append((mac, t0, self.loop.t, cost))
        self.calls += 1
        self.way.settled(mac, cid, cost)
        return True

    def conv(self):
        return {"ctx": 15000, "first": True}

    def sub(self, mac, done):
        c = self.conv()
        for _ in range(self.sub_steps + 1):
            ok = yield from self.call(mac, c)
            if not ok:
                break
            yield self.rng.uniform(*self.tool_s)
        self.ended(mac)
        done()

    def main(self, mac, subs):
        c = self.conv()
        for step in range(self.main_steps + 1):
            ok = yield from self.call(mac, c)
            if not ok:
                break
            if step == 2 and subs:
                join = Future(self.loop)
                left = [subs]

                def one_done():
                    left[0] -= 1
                    if left[0] == 0:
                        join.resolve()
                for _ in range(subs):
                    self.live[mac] += 1
                    self.loop.spawn(self.sub(mac, one_done))
                yield join
            yield self.rng.uniform(*self.tool_s)
        self.ended(mac)


def run_one(way, members, budget, policy, seed, tps, until, **kw):
    random.seed(seed)
    loop = Loop()
    w = WAYS[way](loop, members, budget, policy, tps, **kw)
    team = Team(loop, w, members, seed, tps)
    loop.run(until)
    spent = team.bill.total()
    # The moment the provider's bill truly passed the budget, and what was billed for calls that
    # left after it.
    truly = None
    for a in sorted({c[1] for c in team.bill.calls} | {c[2] for c in team.bill.calls}):
        if team.bill.spent_by(a) >= budget:
            lo, hi = 0.0, a
            for _ in range(40):
                mid = (lo + hi) / 2
                lo, hi = (lo, mid) if team.bill.spent_by(mid) >= budget else (mid, hi)
            truly = hi
            break
    late = sum(c[3] for c in team.bill.calls if truly is not None and c[1] > truly)
    waits = sorted(w.waits) or [0.0]
    return {
        "way": way, "policy": policy, "seed": seed, "budget": budget, "spent": round(spent, 4),
        "over": round(spent - budget, 4), "over_pct": round(100 * (spent - budget) / budget, 1),
        "late": round(late, 4), "calls": team.calls, "refused": team.refused,
        "wait_p50_ms": round(1000 * waits[len(waits) // 2], 1),
        "wait_p99_ms": round(1000 * waits[min(len(waits) - 1, int(len(waits) * 0.99))], 1),
        "wait_max_s": round(waits[-1], 2),
        "waited_10s": sum(1 for x in waits if x > 10),
        "messages_per_call": round(w.messages / max(1, team.calls), 2),
        "ended_s": None if team.end is None else round(team.end, 1),
        "first_refusal_s": None if team.first_refusal is None else round(team.first_refusal, 1),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--way", choices=WAYS, default="exact")
    ap.add_argument("--policy", choices=POLICIES, default="finish_tps")
    ap.add_argument("--members", default="heavy", help="comma-separated: heavy, medium, light, idle")
    ap.add_argument("--budget", type=float, default=3.0)
    ap.add_argument("--tps", type=float, default=500, help="the model's speed, tokens a second")
    ap.add_argument("--seeds", default="1,2,3")
    ap.add_argument("--until", type=float, default=3600)
    ap.add_argument("--kw", default="{}", help="JSON options of the way, such as {\"rtt\": 0.1}")
    a = ap.parse_args()
    for s in a.seeds.split(","):
        r = run_one(a.way, a.members.split(","), a.budget, a.policy, int(s), a.tps, a.until, **json.loads(a.kw))
        print(json.dumps(r))


if __name__ == "__main__":
    main()
