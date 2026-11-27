"""A team's ledger: one small server that several Macs' boundaries ask before spending.

Over HTTP and JSON:

  central   POST /admit {budget, call, input_cost, reserve, mac}     -> {ok, counted, limit}
            POST /settle {budget, call, cost}
            POST /admit_whole {budget, id, bound, mac}               -> {ok}
            POST /settle_whole {budget, id, cost}
            Every model call is one round trip. The ledger counts a running call's output by time
            itself (book.py), so nothing is sent while the call runs. Once what it counts passes
            the budget, it tells each Mac with calls under way to cut them (#385's cut), through
            /watch.
  leases    POST /lease {budget, mac, spent, held, want, req, renew} -> {granted, slice, seq, recall, ttl}
            POST /give {budget, mac, amount, spent, held}            -> {slice, seq}
            A Mac asks for a slice, spends it on its own, and asks again when low. `slice` is all
            the room the ledger holds for that Mac; the Mac's own limit is that number. `want` asks
            for at least that much, for work out of sight whose bound is larger than a slice
            (#385). When too little is free, the ledger first recalls: it tells every other Mac,
            through /watch, to give back the room no call of theirs holds, waits for them a short
            moment, then answers. With `--ttl`, a slice lasts that long after the Mac's last
            message: a Mac renews it while its agents run and stops spending it on its own clock
            before it ends, and once it has ended the ledger keeps only what the Mac last said it
            held, so a Mac that sleeps strands nothing.
  both      GET /watch?budget=&mac=&after=N   waits up to 15 s for news for this Mac:
                                              {boot, gen, cut, recall}, counters that only grow
            GET /state?budget=...

What the ledger holds is dollars per Mac and per call id. No prompt, model name or file name
reaches it. Every change is committed to SQLite (WAL, synchronous=FULL, and F_FULLFSYNC on a Mac) before the answer goes
out, so a grant a Mac was told about survives a crash of the ledger. What /watch says is not
stored: after a restart its counters start again under a new `boot`, and the central ledger sees
the budget passed again on its next look.

Run: python3 server.py --db ledger.db --port 8730 --budget team=15 [--policy reserve_typical]
"""
import argparse, json, os, random, sqlite3, sys, threading, time, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

sys.path.insert(0, os.path.dirname(__file__))
from book import Book  # noqa: E402

# For the crash test: the share of grants after which the ledger dies between its commit and its
# answer, the one moment a Mac cannot know whether it got a slice.
CRASH_AFTER_COMMIT = float(os.environ.get("LEDGER_CRASH_AFTER_COMMIT", "0"))
P_OUT = 20e-6  # dollars per output token, for counting a running call by time

SCHEMA = """
CREATE TABLE IF NOT EXISTS budgets (name TEXT PRIMARY KEY, total REAL NOT NULL);
CREATE TABLE IF NOT EXISTS calls (budget TEXT, id TEXT, started REAL, input REAL, reserve REAL, cost REAL, mac TEXT,
  PRIMARY KEY (budget, id));
CREATE TABLE IF NOT EXISTS whole (budget TEXT, id TEXT, bound REAL, cost REAL, mac TEXT, PRIMARY KEY (budget, id));
CREATE TABLE IF NOT EXISTS leases (budget TEXT, mac TEXT, granted REAL DEFAULT 0, given REAL DEFAULT 0,
  reported REAL DEFAULT 0, asked REAL DEFAULT 0, last_req TEXT, last_grant REAL, held REAL DEFAULT 0,
  expires REAL, seq INTEGER DEFAULT 0, PRIMARY KEY (budget, mac));
"""


class Ledger:
    def __init__(self, path, policy, chunk=2.0, tps=50.0, recall=True, recall_wait=1.0, ttl=0.0, cut=False, share=2):
        self.db = sqlite3.connect(path, check_same_thread=False, isolation_level=None)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        # On a Mac, FULL is a plain fsync(2), which leaves the commit in the drive's cache; these
        # make each commit and each checkpoint F_FULLFSYNC (research/2026-11-25-durability-on-a-mac).
        self.db.execute("PRAGMA fullfsync=ON")
        self.db.execute("PRAGMA checkpoint_fullfsync=ON")
        self.db.executescript(SCHEMA)
        self.mu = threading.Lock()
        self.news = threading.Condition(self.mu)  # /watch waits on this
        self.policy, self.chunk, self.tps = policy, chunk, tps
        self.recall, self.recall_wait, self.ttl, self.cut, self.share = recall, recall_wait, ttl, cut, share
        self.boot = uuid.uuid4().hex[:8]
        self.books = {}
        self.call_mac = {}  # (budget, call id) -> mac, for calls under way
        self.short = {}  # budget -> time a Mac was last refused for lack of free room
        self.watch_state = {}  # (budget, mac) -> {"gen", "cut", "recall"}
        self.gives = {}  # (budget, mac) -> gives received, so a recall knows who answered
        for name, total in self.db.execute("SELECT name, total FROM budgets"):
            self._load(name, total)
        if cut:
            threading.Thread(target=self._watch_loop, daemon=True).start()

    def _load(self, name, total):
        b = Book(total, self.policy, self.tps, P_OUT)
        for cid, started, inp, res, cost, mac in self.db.execute(
                "SELECT id, started, input, reserve, cost, mac FROM calls WHERE budget=?", (name,)):
            if cost is None:
                b.start(cid, started, inp, res)
                self.call_mac[(name, cid)] = mac
            else:
                b.settled += cost
        for wid, bound, cost in self.db.execute("SELECT id, bound, cost FROM whole WHERE budget=?", (name,)):
            if cost is None:
                b.start_whole(wid, bound)
            else:
                b.settled += cost
        self.books[name] = b

    def ensure(self, name, total):
        with self.mu:
            self.db.execute("INSERT OR IGNORE INTO budgets VALUES (?, ?)", (name, total))
            if name not in self.books:
                self._load(name, total)

    # -- news for one Mac, read by /watch --

    def _post(self, name, mac, what):
        st = self.watch_state.setdefault((name, mac), {"gen": 0, "cut": 0, "recall": 0})
        st[what] += 1
        st["gen"] += 1
        self.news.notify_all()

    def watch(self, name, mac, after, boot, wait=15.0):
        end = time.time() + wait
        with self.mu:
            while True:
                st = self.watch_state.setdefault((name, mac), {"gen": 0, "cut": 0, "recall": 0})
                if boot != self.boot or st["gen"] > after:
                    return dict(st, boot=self.boot)
                left = end - time.time()
                if left <= 0:
                    return dict(st, boot=self.boot)
                self.news.wait(left)

    # -- central --

    def admit(self, name, cid, input_cost, reserve, mac=None):
        with self.mu:
            b, now = self.books[name], time.time()
            if cid in b.flight:  # the same admission sent again, after an answer that was lost
                return {"ok": True, "counted": round(b.counted(now), 6), "limit": b.limit, "again": True}
            ok = b.admits(now, input_cost, reserve)
            if ok:
                self.db.execute("INSERT OR IGNORE INTO calls VALUES (?, ?, ?, ?, ?, NULL, ?)", (name, cid, now, input_cost, reserve, mac))
                b.start(cid, now, input_cost, reserve)
                self.call_mac[(name, cid)] = mac
            return {"ok": ok, "counted": round(b.counted(now), 6), "limit": b.limit}

    def settle(self, name, cid, cost):
        with self.mu:
            cur = self.db.execute("UPDATE calls SET cost=? WHERE budget=? AND id=? AND cost IS NULL", (cost, name, cid))
            if cur.rowcount:
                self.books[name].settle(cid, cost)
                self.call_mac.pop((name, cid), None)
            return {"ok": bool(cur.rowcount)}

    def admit_whole(self, name, wid, bound, mac=None):
        with self.mu:
            b, now = self.books[name], time.time()
            if wid in b.whole:
                return {"ok": True, "again": True}
            ok = b.admits_whole(now, bound)
            if ok:
                self.db.execute("INSERT OR IGNORE INTO whole VALUES (?, ?, ?, NULL, ?)", (name, wid, bound, mac))
                b.start_whole(wid, bound)
            return {"ok": ok}

    def settle_whole(self, name, wid, cost):
        with self.mu:
            cur = self.db.execute("UPDATE whole SET cost=? WHERE budget=? AND id=? AND cost IS NULL", (cost, name, wid))
            if cur.rowcount:
                self.books[name].settle_whole(wid, cost)
            return {"ok": bool(cur.rowcount)}

    def _watch_loop(self):
        """#385's cut, at the ledger: every half second, once what it counts passes the budget, it
        tells each Mac with calls under way to cut them, as the simulator's `Central.watch` does."""
        while True:
            time.sleep(0.5)
            with self.mu:
                now = time.time()
                for name, b in self.books.items():
                    if b.flight and b.counted(now) >= b.limit:
                        for mac in {self.call_mac.get((name, c)) for c in b.flight} - {None}:
                            self._post(name, mac, "cut")

    # -- leases --

    def _row(self, name, mac):
        self.db.execute("INSERT OR IGNORE INTO leases (budget, mac) VALUES (?, ?)", (name, mac))
        return self.db.execute("SELECT granted, given, reported, held, expires, seq FROM leases WHERE budget=? AND mac=?",
                               (name, mac)).fetchone()

    def _expire(self, name, now):
        """A slice whose Mac has not been heard from within its lease shrinks to what that Mac last
        said it held: settled spend, calls under way at their hold, work out of sight at its bound."""
        if not self.ttl:
            return
        for mac, granted, given, held in self.db.execute(
                "SELECT mac, granted, given, held FROM leases WHERE budget=? AND expires IS NOT NULL AND expires < ?",
                (name, now)).fetchall():
            back = (granted - given) - held
            if back > 1e-9:
                self.db.execute("UPDATE leases SET given = given + ?, seq = seq + 1, expires = NULL WHERE budget=? AND mac=?", (back, name, mac))
            else:
                self.db.execute("UPDATE leases SET expires = NULL WHERE budget=? AND mac=?", (name, mac))

    def _free(self, name):
        used = self.db.execute("SELECT COALESCE(SUM(MAX(granted - given, reported)), 0) FROM leases WHERE budget=?", (name,)).fetchone()[0]
        return max(0.0, self.books[name].limit - used)

    def _answer(self, name, mac, now, extra):
        granted, given, _, _, _, seq = self._row(name, mac)
        return dict(extra, slice=round(granted - given, 9), seq=seq, recall=self._recall(name, now), ttl=self.ttl)

    def lease(self, name, mac, spent, req=None, want=0.0, held=None, renew=False):
        held = spent if held is None else held
        # Too little free: recall first, outside the lock, so the other Macs' gives can land.
        if self.recall and not renew:
            with self.mu:
                again = req is not None and self._row(name, mac) and self.db.execute(
                    "SELECT last_req FROM leases WHERE budget=? AND mac=?", (name, mac)).fetchone()[0] == req
                self._expire(name, time.time())
                need = max(self.chunk, want)
                others = []
                if not again and self._free(name) < need:
                    others = [m for (m, g, gv, h) in self.db.execute(
                        "SELECT mac, granted, given, held FROM leases WHERE budget=? AND mac != ?", (name, mac)).fetchall()
                        if g - gv > 1e-9]
                    before = {m: self.gives.get((name, m), 0) for m in others}
                    for m in others:
                        self._post(name, m, "recall")
                    end = time.time() + self.recall_wait
                    while time.time() < end and any(self.gives.get((name, m), 0) == before[m] for m in others):
                        self.news.wait(max(0.0, end - time.time()))
        with self.mu:
            now = time.time()
            self.db.execute("BEGIN IMMEDIATE")
            try:
                self._row(name, mac)
                if req is not None:
                    # The same ask sent again, after an answer that was lost: the same answer.
                    last = self.db.execute("SELECT last_req, last_grant FROM leases WHERE budget=? AND mac=?", (name, mac)).fetchone()
                    if last[0] == req:
                        self.db.execute("COMMIT")
                        return self._answer(name, mac, now, {"granted": last[1], "again": True})
                self._expire(name, now)
                exp = now + self.ttl if self.ttl else None
                self.db.execute("UPDATE leases SET reported=MAX(reported, ?), held=?, asked=?, expires=? WHERE budget=? AND mac=?",
                                (spent, held, now, exp, name, mac))
                g = 0.0
                if not renew:
                    free = self._free(name)
                    active = self.db.execute("SELECT COUNT(*) FROM leases WHERE budget=? AND asked > ?", (name, now - 60)).fetchone()[0]
                    g = min(free, max(self.chunk, free / (self.share * max(1, active)), want))
                    self.db.execute("UPDATE leases SET granted = granted + ?, last_req = ?, last_grant = ?, seq = seq + 1 WHERE budget=? AND mac=?",
                                    (g, req, g, name, mac))
                    if free < self.chunk:
                        self.short[name] = now
                self.db.execute("COMMIT")
                if CRASH_AFTER_COMMIT and g > 0 and random.random() < CRASH_AFTER_COMMIT:
                    os._exit(9)
            except BaseException:
                self.db.execute("ROLLBACK")
                raise
            return self._answer(name, mac, now, {"granted": g})

    def give(self, name, mac, amount, spent, held=None, req=None):
        with self.mu:
            now = time.time()
            self.db.execute("BEGIN IMMEDIATE")
            try:
                self._row(name, mac)
                last = self.db.execute("SELECT last_req FROM leases WHERE budget=? AND mac=?", (name, mac)).fetchone()[0]
                if req is None or last != req:
                    exp = now + self.ttl if self.ttl else None
                    self.db.execute("UPDATE leases SET given = given + ?, reported = MAX(reported, ?), held = ?, seq = seq + 1, "
                                    "last_req = ?, expires = ? WHERE budget=? AND mac=?",
                                    (amount, spent, spent if held is None else held, req, exp, name, mac))
                self.db.execute("COMMIT")
            except BaseException:
                self.db.execute("ROLLBACK")
                raise
            self.gives[(name, mac)] = self.gives.get((name, mac), 0) + 1
            self.news.notify_all()
            return self._answer(name, mac, now, {"ok": True})

    def _recall(self, name, now):
        return now - self.short.get(name, 0) < 10

    def state(self, name):
        with self.mu:
            now = time.time()
            self._expire(name, now)
            rows = self.db.execute("SELECT mac, granted, given, reported, held FROM leases WHERE budget=? ORDER BY mac", (name,)).fetchall()
            b = self.books[name]
            return {"limit": b.limit, "settled": round(b.settled, 6), "in_flight": len(b.flight), "whole": len(b.whole),
                    "counted": round(b.counted(now), 6), "free": round(self._free(name), 6), "boot": self.boot,
                    "leases": [{"mac": m, "granted": g, "given": gv, "reported": r, "held": h} for m, g, gv, r, h in rows]}


def handler(ledger):
    class H(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def reply(self, o, code=200):
            body = json.dumps(o).encode()
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            u = urlparse(self.path)
            q = {k: v[0] for k, v in parse_qs(u.query).items()}
            if u.path == "/state":
                return self.reply(ledger.state(q["budget"]))
            if u.path == "/watch":
                return self.reply(ledger.watch(q["budget"], q["mac"], int(q.get("after", -1)), q.get("boot"),
                                               float(q.get("wait", 15))))
            self.reply({"error": "not here"}, 404)

        def do_POST(self):
            n = int(self.headers.get("content-length") or 0)
            o = json.loads(self.rfile.read(n) or b"{}")
            try:
                if self.path == "/admit":
                    return self.reply(ledger.admit(o["budget"], o["call"], o["input_cost"], o.get("reserve", 0.0), o.get("mac")))
                if self.path == "/settle":
                    return self.reply(ledger.settle(o["budget"], o["call"], o["cost"]))
                if self.path == "/admit_whole":
                    return self.reply(ledger.admit_whole(o["budget"], o["id"], o["bound"], o.get("mac")))
                if self.path == "/settle_whole":
                    return self.reply(ledger.settle_whole(o["budget"], o["id"], o["cost"]))
                if self.path == "/lease":
                    return self.reply(ledger.lease(o["budget"], o["mac"], o.get("spent", 0.0), o.get("req"), o.get("want", 0.0),
                                                   o.get("held"), o.get("renew", False)))
                if self.path == "/give":
                    return self.reply(ledger.give(o["budget"], o["mac"], o["amount"], o.get("spent", 0.0), o.get("held"), o.get("req")))
            except KeyError as e:
                return self.reply({"error": f"unknown {e}"}, 400)
            self.reply({"error": "not here"}, 404)
    return H


class Server(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 256


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="ledger.db")
    ap.add_argument("--port", type=int, default=8730)
    ap.add_argument("--budget", action="append", default=[], help="name=dollars")
    ap.add_argument("--policy", default="reserve_typical")
    ap.add_argument("--chunk", type=float, default=2.0, help="the smallest slice, in dollars")
    ap.add_argument("--tps", type=float, default=50.0, help="the model's speed the ledger counts a running call by")
    ap.add_argument("--cut", action="store_true", help="#385's cut: tell Macs to cut calls under way once the budget is passed")
    ap.add_argument("--no-recall", action="store_true")
    ap.add_argument("--recall-wait", type=float, default=1.0, help="how long an ask waits for the Macs it recalled")
    ap.add_argument("--ttl", type=float, default=0.0, help="seconds a slice lasts after its Mac's last message; 0: forever")
    a = ap.parse_args()
    ledger = Ledger(a.db, a.policy, a.chunk, a.tps, not a.no_recall, a.recall_wait, a.ttl, a.cut)
    for spec in a.budget:
        name, total = spec.split("=")
        ledger.ensure(name, float(total))
    srv = Server(("127.0.0.1", a.port), handler(ledger))
    print(f"ledger on 127.0.0.1:{a.port}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
