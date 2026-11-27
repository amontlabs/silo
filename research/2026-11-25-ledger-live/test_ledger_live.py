"""Tests of what this folder added to #388's ledger server and of the boundary's client, each
against a real server process on localhost.

python3 test_ledger_live.py
"""
import os, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "2026-11-25-team-ledger", "ledger"))
import client as ways  # noqa: E402
from test_ledger import Server, call  # noqa: E402


def server(budget, **kw):
    extra = []
    for k, v in kw.items():
        extra += [f"--{k.replace('_', '-')}"] + ([] if v is True else [str(v)])
    s = Server(budget)
    s.start_args = extra
    orig = s.start

    def start():
        import subprocess
        s.starts += 1
        s.p = subprocess.Popen([sys.executable, os.path.join(HERE, "..", "2026-11-25-team-ledger", "ledger", "server.py"),
                                "--db", os.path.join(s.dir, "l.db"), "--port", str(s.port), "--budget", f"team={budget}"] + extra,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        s.p.stdout.readline()
    s.start = start
    del orig
    s.start()
    return s


def check(name, ok, detail=""):
    print(f"{'PASS' if ok else 'FAIL'} {name}{': ' + detail if detail else ''}")
    return ok


def test_central_cut():
    """The ledger counts a running call's output by time and, once the total passes the budget,
    tells the Mac that has calls under way to cut them."""
    s = server(1.0, tps=50000, cut=True, policy="finish_tps")
    try:
        h = ways.Http("127.0.0.1", s.port)
        way = ways.Central(h, "team", "m0")
        cuts = []
        way.on_cut = lambda: cuts.append(time.time())
        time.sleep(0.3)
        t0 = time.time()
        assert way.admit("c1", 0.1, 0.0)
        while not cuts and time.time() - t0 < 5:
            time.sleep(0.05)
        # Output counted at 50,000 tokens/s and $20 per million is $1 a second: the total passes $1
        # 0.9 s after the call starts, and the ledger looks every half second.
        return check("central: a cut reaches the Mac once the ledger counts past the budget", False, "no cut") if not cuts else \
            check("central: a cut reaches the Mac once the ledger counts past the budget", True, f"after {cuts[0] - t0:.2f} s")
    finally:
        s.stop()


def test_whole_and_want():
    """Work out of sight is admitted only if its whole bound fits; under leases a bound bigger than a
    slice is asked for."""
    s = server(10.0, chunk=2.0)
    try:
        r1 = call(s.port, "POST", "/admit_whole", {"budget": "team", "id": "b1", "bound": 12.0})
        r2 = call(s.port, "POST", "/admit_whole", {"budget": "team", "id": "b2", "bound": 4.0})
        r3 = call(s.port, "POST", "/admit", {"budget": "team", "call": "c", "input_cost": 0.1, "reserve": 6.5})
        ok1 = check("central: a bound larger than the budget is refused, one that fits is held", not r1["ok"] and r2["ok"] and not r3["ok"],
                    f"12 → {r1['ok']}, 4 → {r2['ok']}, then a call holding 6.6 → {r3['ok']}")
    finally:
        s.stop()
    s = server(10.0, chunk=2.0)
    try:
        h = ways.Http("127.0.0.1", s.port)
        way = ways.Lease(h, "team", "m0", "reserve_typical", 500)
        ok = way.admit_whole("b", 4.0)
        ok2 = check("leases: a $4 bound with $2 slices is asked for and admitted", ok and way.book.limit >= 4.0, f"slice ${way.book.limit:.2f}")
    finally:
        s.stop()
    return ok1 and ok2


def test_recall():
    """A Mac holding the whole budget and no call gives it back when another asks."""
    s = server(6.0, chunk=2.0, recall_wait=1.0)
    try:
        a = ways.Lease(ways.Http("127.0.0.1", s.port), "team", "a", "reserve_typical", 500, keep=0.0)
        b = ways.Lease(ways.Http("127.0.0.1", s.port), "team", "b", "reserve_typical", 500)
        time.sleep(0.3)
        a._ask(want=6.0)
        held_a = a.book.limit
        t0 = time.time()
        b._ask()
        took = time.time() - t0
        return check("leases: a recall moves a slice from a Mac that does not use it", held_a == 6.0 and b.book.limit >= 2.0 and a.book.limit <= 4.0,
                     f"a held ${held_a:.2f}, then ${a.book.limit:.2f}; b got ${b.book.limit:.2f} in {1000 * took:.0f} ms")
    finally:
        s.stop()


def test_expiry():
    """With a ttl, a slice whose Mac went silent shrinks to what it held, and the Mac itself stops
    spending it on its own clock before that."""
    s = server(6.0, chunk=2.0, ttl=2.0, no_recall=True)
    try:
        a = ways.Lease(ways.Http("127.0.0.1", s.port), "team", "a", "reserve_typical", 500, margin=0.5)
        a.live = False  # no renewals: a Mac asleep
        b = ways.Lease(ways.Http("127.0.0.1", s.port), "team", "b", "reserve_typical", 500)
        b.live = False
        a._ask(want=6.0)
        lapsed_early = a._lapsed(time.time())
        time.sleep(1.6)
        lapsed = a._lapsed(time.time())
        r_before = call(s.port, "GET", "/state?budget=team")["free"]
        time.sleep(0.6)
        b._ask()
        return check("leases: a slice ends on the Mac before it ends at the ledger, then goes to another Mac",
                     not lapsed_early and lapsed and r_before == 0 and b.book.limit >= 2.0,
                     f"free while a's lease runs ${r_before:.2f}; b got ${b.book.limit:.2f} after it ended")
    finally:
        s.stop()


if __name__ == "__main__":
    rs = [test_whole_and_want(), test_recall(), test_expiry(), test_central_cut()]
    sys.exit(0 if all(rs) else 1)
