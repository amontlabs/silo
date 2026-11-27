"""One budget's admission rule, ported from #374's `boundary/src/budget.zig` (total limits only),
with #385's additions: work out of sight (a batch, a background response, a cache) admitted only
if its whole bound fits, and held at that bound until its result passes.

A call is admitted against what is counted, counted while it runs, and settled on the provider's
count. Pure: the caller passes the time in. Used by the simulator for every Mac's boundary and by
the ledger server, so what is measured is the rule that runs.

`extra` is spend this book does not see itself but must count: the other Macs' spend, as a peer
last heard it.
"""

MAX_TOKENS = 128_000  # what Claude Code asks for on every call (#374)

POLICIES = ("finish", "finish_tps", "admit_input", "reserve_typical", "reserve_max")


class Book:
    def __init__(self, limit, policy="finish_tps", tps=None, out_price=0.0):
        assert policy in POLICIES, policy
        self.limit = limit
        self.policy = policy
        # `finish` counts no output while a call runs; the others count it by time, as #374's
        # `output_tps` did for every policy but `finish` and `reserve_max`.
        self.tps = None if policy in ("finish", "reserve_max") else tps
        self.out_price = out_price  # dollars per output token
        self.settled = 0.0
        self.flight = {}  # call id -> (start, input cost, reserve)
        self.whole = {}   # id of work out of sight -> its bound, held until its result passes

    def _counted_one(self, f, now):
        start, inp, _ = f
        c = inp
        if self.tps:
            c += min(max(0.0, now - start) * self.tps, MAX_TOKENS) * self.out_price
        return c

    def counted(self, now):
        return self.settled + sum(self.whole.values()) + sum(self._counted_one(f, now) for f in self.flight.values())

    def held(self, now):
        return self.settled + sum(self.whole.values()) + sum(max(self._counted_one(f, now), f[2]) for f in self.flight.values())

    def reserve_for(self, input_cost, typical_out_tokens):
        if self.policy == "reserve_typical":
            return input_cost + typical_out_tokens * self.out_price
        if self.policy == "reserve_max":
            return input_cost + MAX_TOKENS * self.out_price
        return 0.0

    def admits(self, now, input_cost, reserve, extra=0.0, limit=None):
        limit = self.limit if limit is None else limit
        if self.policy in ("finish", "finish_tps"):
            return self.counted(now) + extra < limit
        if self.policy == "admit_input":
            return self.counted(now) + extra + input_cost <= limit
        return self.held(now) + extra + reserve <= limit

    def start(self, cid, now, input_cost, reserve):
        self.flight[cid] = (now, input_cost, reserve)

    def settle(self, cid, cost):
        self.flight.pop(cid, None)
        self.settled += cost

    # -- work out of sight (#385): whatever the policy, its whole bound must fit, since nothing
    # can stop it once it has left --

    def admits_whole(self, now, bound, extra=0.0, limit=None):
        limit = self.limit if limit is None else limit
        return self.held(now) + extra + bound <= limit

    def start_whole(self, wid, bound):
        self.whole[wid] = bound

    def settle_whole(self, wid, cost):
        self.whole.pop(wid, None)
        self.settled += cost
