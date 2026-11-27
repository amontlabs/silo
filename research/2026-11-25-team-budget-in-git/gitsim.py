"""Leases kept in a Git branch, simulated as Git can actually do them, against #388's leases.

#388's `lease` way with a 2 s round trip stood for "a Git push as the ledger", but it kept two
things Git cannot do: the ledger calls Macs back (a recall), and writes cost nothing but time. In a
Git branch nobody can call a Mac, and every change is a push, landed one at a time.

`gitlease` here:

  - Every change is a push. The branch takes one at a time; each takes `push` seconds (1.7 s, the
    median measured against GitHub by hostedlease.py). A push that waits behind others is refused
    once per push that lands before it and is tried again; those count as writes.
  - No recall. Every `ttl` seconds a Mac with agents running checks in: one push that reports what
    it spent and gives back what it holds beyond its calls under way and one slice.
  - A Mac whose call needs more room than it has gives its leftover back with its ask. When nothing
    is free, the call waits until the next check-in of a Mac that holds room, and asks again. It is
    refused only when no other Mac holds more than $1.
  - `lapse_frees=True`: a slice also expires `ttl` seconds after its last push; the ledger then
    counts as free whatever the Mac had not reported, and the Mac cuts its calls at its expiry.
    `lapse_frees=False`: a slice stays the Mac's until the Mac gives it back.
  - `fixed=True`: every grant is one $2 slice; otherwise half of what is free per active Mac, as
    #388 does.

`sleep=(mac, t)`: from t, that Mac's agents stop where they are and it sends nothing more.
`down=[(from, to)]`: the branch takes no push in that span.

python3 gitsim.py            every table, 20 seeds, into out/gitsim.md
"""
import json, os, statistics, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "2026-11-25-team-ledger", "sim"))
import sim  # noqa: E402
from sim import Future, Lease, Way, TYPICAL_OUT  # noqa: E402


class Sleepy:
    """Mixin: from `sleep[1]`, Mac `sleep[0]` is asleep. Its admissions never answer (its agents
    stop where they are), and nothing it would send leaves."""

    def asleep(self, mac):
        return self.sleep is not None and mac == self.sleep[0] and self.loop.t >= self.sleep[1]


class SleepyLease(Sleepy, Lease):
    def __init__(self, *a, sleep=None, **kw):
        self.sleep = sleep
        super().__init__(*a, **kw)

    def admit(self, mac, cid, inp):
        if self.asleep(mac):
            return Future(self.loop)
        return super().admit(mac, cid, inp)

    def give_back(self, j, half):
        if self.asleep(j):
            return  # a recall reaches no one
        super().give_back(j, half)

    def done(self, mac):
        if self.asleep(mac):
            return
        super().done(mac)


class GitLease(Sleepy, Way):
    def __init__(self, *a, push=1.7, chunk=2.0, low=1.0, ttl=300.0, margin=None, share=2, sleep=None, down=None, lapse_frees=True, fixed=False):
        super().__init__(*a)
        n = len(self.team)
        self.push, self.chunk, self.low, self.ttl, self.share = push, chunk, low, ttl, share
        self.margin = margin if margin is not None else 3 * push + 5.0
        self.sleep = sleep
        self.lapse_frees, self.fixed = lapse_frees, fixed
        self.down = sim.Down(down)
        self.books = [self.book(0.0) for _ in range(n)]
        self.granted = [0.0] * n
        self.returned = [0.0] * n
        self.reported = [0.0] * n
        self.expires = [0.0] * n      # the ledger's view: when each Mac's room stops being its own
        self.local_exp = [0.0] * n    # the Mac's view
        self.asked_at = [-1e9] * n
        self.pending = [None] * n
        self.empty = [False] * n
        self.retry_at = [None] * n
        self.over_since = [None] * n
        self.renewing = [False] * n
        self.blocked = [False] * n    # the Mac has a call waiting for room
        self.owed = [0.0] * n         # room a lapsed Mac gave up and has not yet told the ledger of
        self.busy_until = 0.0         # the branch takes one push at a time
        self.landed = []              # times pushes landed, to count refusals
        self.pushes = 0
        self.lapsed = 0

    # -- the branch --

    def write(self, mac, apply):
        """A push from `mac`: queued behind the pushes before it; `apply` runs when it lands and
        returns what the Mac learns. Returns a Future of that."""
        f = Future(self.loop)
        sent = self.loop.t

        def land():
            up = self.down.up_at(self.loop.t)
            if up > self.loop.t:
                self.loop.at(up, land)
                return
            start = max(self.loop.t, self.busy_until)
            self.busy_until = start + self.push
            # Refused once per push that landed between this one's read and its own landing.
            self.pushes += 1 + sum(1 for t in self.landed if sent < t <= start)

            def done():
                self.landed.append(self.loop.t)
                self.landed[:] = [t for t in self.landed if t > self.loop.t - 600]
                f.resolve(apply())
            self.loop.at(start + self.push, done)
        land()
        return f

    def held_by(self, j, now):
        """Room the ledger counts as Mac j's: its grants less what it gave back, or what it said it
        spent; once its lease has expired, only what it said it spent."""
        if now >= self.expires[j] and self.lapse_frees:
            return self.reported[j]
        return max(self.granted[j] - self.returned[j], self.reported[j])

    def free(self, now):
        return max(0.0, self.budget - sum(self.held_by(j, now) for j in range(len(self.team))))

    def expire(self, j, now):
        """The ledger's side of an expiry: what was not reported is no longer Mac j's."""
        if self.lapse_frees and now >= self.expires[j] and self.granted[j] - self.returned[j] > self.reported[j]:
            self.returned[j] = self.granted[j] - self.reported[j]
            self.lapsed += 1

    # -- a Mac --

    def room(self, mac):
        b = self.books[mac]
        return b.limit - b.held(self.loop.t)

    def ask(self, mac, want=0.0):
        if self.pending[mac]:
            return self.pending[mac]
        if self.asleep(mac):
            return Future(self.loop)
        f = Future(self.loop)
        self.pending[mac] = f
        b0 = self.books[mac]
        spent = b0.settled
        # Room too small for the call that waits goes back with the ask, so it can be pooled.
        mine = max(0.0, self.room(mac)) if self.blocked[mac] else 0.0
        b0.limit -= mine
        owed, self.owed[mac] = self.owed[mac], 0.0

        def apply():
            now = self.loop.t
            for j in range(len(self.team)):
                self.expire(j, now)
            self.returned[mac] += mine
            if not (self.lapse_frees and now >= self.expires[mac]):
                self.returned[mac] += owed  # a lapse the ledger has not freed on its own
            self.reported[mac] = max(self.reported[mac], spent)
            self.asked_at[mac] = now
            active = sum(1 for t in self.asked_at if now - t < 60)
            free = self.free(now)
            # Nothing is given when what is free does not cover the call that waits: a sliver
            # would only bring the Mac back for another push.
            if self.fixed:  # one slice at a time, whatever is free: less to strand, less to lose
                g = 0.0 if free + 1e-9 < want else min(free, max(self.chunk, want))
            else:
                g = 0.0 if free + 1e-9 < want else min(free, max(self.chunk, free / (self.share * max(1, active)), want))
            self.granted[mac] += g
            self.expires[mac] = now + self.ttl
            others = [j for j in range(len(self.team)) if j != mac and now < self.expires[j]
                      and self.granted[j] - self.returned[j] - self.reported[j] > self.low]
            nxt = min((self.expires[j] for j in others), default=None)
            return g, now + self.ttl, nxt

        def back(r):
            g, exp, nxt = r
            b = self.books[mac]
            b.limit += g
            self.local_exp[mac] = exp
            self.empty[mac] = g <= 1e-9
            self.retry_at[mac] = nxt if g <= 1e-9 else None
            self.pending[mac] = None
            self.schedule_renew(mac)
            f.resolve(g)
        self.write(mac, apply).then(back)
        return f

    def schedule_renew(self, mac):
        exp = self.local_exp[mac]
        at = exp - self.margin
        self.loop.at(max(self.loop.t, at), lambda: self.renew(mac, exp))
        self.loop.at(exp, lambda: self.lapse(mac, exp))

    def renew(self, mac, exp):
        if self.local_exp[mac] != exp or self.asleep(mac) or self.renewing[mac]:
            return
        if not self.has_work(mac):
            return
        b = self.books[mac]
        # Calls under way, plus a slice unless a call already waits for room it cannot get.
        keep = (b.limit - b.settled) - self.room(mac) + (0.0 if self.blocked[mac] else self.chunk)
        back = max(0.0, (b.limit - b.settled) - keep)
        b.limit -= back
        spent = b.settled
        self.renewing[mac] = True

        def apply():
            now = self.loop.t
            if now >= self.expires[mac]:
                if self.lapse_frees:
                    self.expire(mac, now)
                else:  # the ledger kept the room counted: take what this renewal gives back
                    self.returned[mac] += back
                    self.reported[mac] = max(self.reported[mac], spent)
                return None
            self.returned[mac] += back
            self.reported[mac] = max(self.reported[mac], spent)
            self.expires[mac] = now + self.ttl
            return now + self.ttl

        def landed(newexp):
            self.renewing[mac] = False
            if newexp is None:
                return  # too late: the lapse at the old expiry already took the room
            self.local_exp[mac] = newexp
            self.schedule_renew(mac)
        self.write(mac, apply).then(landed)

    def lapse(self, mac, exp):
        """At the Mac's own expiry, unrenewed: cut what runs and give up the room."""
        if self.local_exp[mac] != exp or self.asleep(mac):
            return
        if not self.lapse_frees:
            return  # the ledger never takes the room back on its own, so it stays safe to use
        b = self.books[mac]
        if b.limit > b.settled:
            self.crew.cut_mac(mac)
            self.owed[mac] += b.limit - b.settled
            b.limit = b.settled
        self.local_exp[mac] = 0.0

    def has_work(self, mac):
        return self.crew.live[mac] > 0

    def admit(self, mac, cid, inp):
        f, t0, b = Future(self.loop), self.loop.t, self.books[mac]
        if self.asleep(mac):
            return f
        res = b.reserve_for(inp, TYPICAL_OUT)

        def attempt(_=None):
            if self.asleep(mac):
                return
            live = self.loop.t < self.local_exp[mac] or (not self.lapse_frees and self.local_exp[mac] > 0)
            if live and b.admits(self.loop.t, inp, res):
                self.blocked[mac] = False
                b.start(cid, self.loop.t, inp, res)
                self.waits.append(self.loop.t - t0)
                if self.room(mac) < self.low and not self.empty[mac]:
                    self.ask(mac)
                f.resolve(True)
            elif self.empty[mac] and not self.pending[mac]:
                self.blocked[mac] = True
                if self.retry_at[mac] is not None:
                    at = self.retry_at[mac] + 0.01
                    self.retry_at[mac] = None
                    self.empty[mac] = False
                    self.loop.at(max(self.loop.t, at), lambda: self.ask(mac).then(attempt))
                else:
                    f.resolve(False)
            else:
                need = 0.0
                if self.loop.t < self.local_exp[mac]:
                    self.blocked[mac] = True
                if self.policy not in ("finish", "finish_tps", "admit_input"):
                    need = res  # its own leftover room goes back with the ask and is pooled first
                self.ask(mac, need).then(attempt)
        attempt()
        return f

    def settled(self, mac, cid, cost):
        self.books[mac].settle(cid, cost)

    def admit_whole(self, mac, wid, bound):
        f, b = Future(self.loop), self.books[mac]

        def attempt(_=None):
            if b.admits_whole(self.loop.t, bound):
                b.start_whole(wid, bound)
                f.resolve(True)
            elif self.empty[mac] and not self.pending[mac]:
                f.resolve(False)
            else:
                self.ask(mac, want=bound - self.room(mac)).then(attempt)
        attempt()
        return f

    def settled_whole(self, mac, wid, cost):
        self.books[mac].settle_whole(wid, cost)

    def watch(self):
        now = self.loop.t
        for m in self.running_macs():
            if self.asleep(m):
                continue
            b = self.books[m]
            if b.counted(now) < b.limit:
                self.over_since[m] = None
                continue
            if self.over_since[m] is None:
                self.over_since[m] = now
                self.empty[m] = False
                self.ask(m)
            if (self.empty[m] and not self.pending[m]) or now - self.over_since[m] > 1 + 2 * self.push:
                self.crew.cut_mac(m)

    def done(self, mac):
        if self.asleep(mac):
            return
        b = self.books[mac]
        back = max(0.0, b.limit - b.settled)
        b.limit -= back
        spent = b.settled

        def apply():
            self.returned[mac] += back
            self.reported[mac] = max(self.reported[mac], spent)
        if back > 0:
            self.write(mac, apply)

    @property
    def messages(self):
        return self.pushes

    @messages.setter
    def messages(self, v):
        pass


sim.WAYS["gitlease"] = GitLease
sim.WAYS["lease_sleepy"] = SleepyLease

TEAM5 = ["heavy", "heavy", "medium", "light", "idle"]
TEAM12 = ["heavy"] * 4 + ["medium"] * 4 + ["light"] * 3 + ["idle"]
SEEDS = range(1, 21)


def runs(way, members, budget, policy, tps, **kw):
    return [sim.run_one(way, members, budget, policy, s, tps, 6 * 3600, **dict(kw)) for s in SEEDS]


HEAD = ("| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, s | Longest wait, s | Calls that waited over 10 s, mean | Writes per call |\n"
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|")


def row(label, rs):
    sp = [r["spent"] / r["budget"] for r in rs]
    return "| %s | %.2f | %+.1f%% | %+.1f%% | %.0f | %.1f | %.1f | %.1f | %.2f |" % (
        label, statistics.mean(sp), max(r["over_pct"] for r in rs), min(r["over_pct"] for r in rs),
        statistics.mean(r["calls"] for r in rs), max(r["wait_p99_ms"] for r in rs) / 1000,
        max(r["wait_max_s"] for r in rs), statistics.mean(r["waited_10s"] for r in rs),
        statistics.mean(r["messages_per_call"] for r in rs))


def main():
    out = []

    def table(title, lines, note=""):
        out.append(f"### {title}\n\n{note}{HEAD}\n" + "\n".join(lines) + "\n")
        print(out[-1], flush=True)

    R388 = ("#388's leases, 2 s round trip, with a recall", "lease", {"rtt": 2.0})
    FREES = ("Git: expiry 5 min frees what was not reported", "gitlease", {"ttl": 300.0})
    KEEPS = ("Git: check-in every 5 min, a slice stays the Mac's until it gives it back", "gitlease", {"ttl": 300.0, "lapse_frees": False})
    KEEPS2 = ("Git: as above, $2 slices only", "gitlease", {"ttl": 300.0, "lapse_frees": False, "fixed": True})
    KEEPS_SLOW = ("Git: as above, pushes 5 s", "gitlease", {"ttl": 300.0, "lapse_frees": False, "fixed": True, "push": 5.0})
    WAYS = [R388, FREES, KEEPS, KEEPS2, KEEPS_SLOW]

    for members, budget, name in ((TEAM5, 15.0, "Team of 5, $15"), (TEAM12, 36.0, "Team of 12, $36")):
        lines = [row(l, runs(w, members, budget, "reserve_typical", 50, cut_calls=True, **kw)) for l, w, kw in WAYS]
        table(f"{name}, `typical-cut`, 50 tokens/s", lines)
    lines = [row(l, runs(w, TEAM5, 15.0, "finish_tps", 50, **kw)) for l, w, kw in WAYS]
    table("Team of 5, $15, `finish_tps` without a cut, 50 tokens/s", lines)

    SLEEP = {"sleep": (0, 300.0)}
    lines = [row("#388's leases, 30 ms, a heavy Mac asleep from 300 s (its recall reaches no one)", runs("lease_sleepy", TEAM5, 15.0, "reserve_typical", 50, cut_calls=True, rtt=0.03, **SLEEP))]
    lines += [row(l + ", a heavy Mac asleep from 300 s", runs(w, TEAM5, 15.0, "reserve_typical", 50, cut_calls=True, **kw, **SLEEP)) for l, w, kw in WAYS[1:4]]
    table("A heavy Mac goes to sleep holding its slice, team of 5, $15, `typical-cut`, 50 tokens/s", lines,
          "Its agents stop where they are and it sends nothing more. Under budget here is what the four awake Macs could not spend.\n\n")

    lines = []
    for label, down in (("1 minute from 300 s", [(300.0, 360.0)]), ("10 minutes from 600 s", [(600.0, 1200.0)])):
        lines.append(row(f"#388's leases, 30 ms, ledger down {label}", runs("lease", TEAM5, 15.0, "reserve_typical", 50, cut_calls=True, rtt=0.03, down=down)))
        lines += [row(f"{l}, GitHub down {label}", runs(w, TEAM5, 15.0, "reserve_typical", 50, cut_calls=True, down=down, **kw)) for l, w, kw in WAYS[1:4]]
    table("GitHub cannot be reached, team of 5, $15, `typical-cut`, 50 tokens/s", lines)

    os.makedirs(os.path.join(HERE, "out"), exist_ok=True)
    open(os.path.join(HERE, "out", "gitsim.md"), "w").write("\n".join(out))


if __name__ == "__main__":
    main()
