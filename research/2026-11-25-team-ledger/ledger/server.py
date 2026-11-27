"""A team's ledger: one small server that several Macs' boundaries ask before spending.

Two ways in, both over HTTP and JSON:

  central   POST /admit {budget, call, input_cost, reserve}  -> {ok, counted, limit}
            POST /settle {budget, call, cost}
            Every model call is one round trip. The ledger counts a running call's output by time
            itself (book.py), so nothing is sent while the call runs.
  leases    POST /lease {budget, mac, spent}                  -> {granted, recall}
            POST /give {budget, mac, amount, spent}
            A Mac asks for a slice, spends it on its own, and asks again when low. `recall` in
            any answer tells the Mac that others are short: it gives back the room it does not
            hold for calls under way.
            GET /state?budget=...

What the ledger holds is dollars per Mac and per call id. No prompt, model name or file name
reaches it. Every change is committed to SQLite (WAL, synchronous=FULL, and F_FULLFSYNC on a Mac) before the answer goes
out, so a grant a Mac was told about survives a crash of the ledger.

Run: python3 server.py --db ledger.db --port 8730 --budget team=15 [--policy reserve_typical]
"""
import argparse, json, os, random, sqlite3, sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

sys.path.insert(0, os.path.dirname(__file__))
from book import Book  # noqa: E402

# For the crash test: the share of grants after which the ledger dies between its commit and its
# answer, the one moment a Mac cannot know whether it got a slice.
CRASH_AFTER_COMMIT = float(os.environ.get("LEDGER_CRASH_AFTER_COMMIT", "0"))
P_OUT = 20e-6  # dollars per output token, for counting a running call by time
TPS = 50.0     # the model's speed the ledger counts by

SCHEMA = """
CREATE TABLE IF NOT EXISTS budgets (name TEXT PRIMARY KEY, total REAL NOT NULL);
CREATE TABLE IF NOT EXISTS calls (budget TEXT, id TEXT, started REAL, input REAL, reserve REAL, cost REAL,
  PRIMARY KEY (budget, id));
CREATE TABLE IF NOT EXISTS leases (budget TEXT, mac TEXT, granted REAL DEFAULT 0, given REAL DEFAULT 0,
  reported REAL DEFAULT 0, asked REAL DEFAULT 0, last_req TEXT, last_grant REAL, PRIMARY KEY (budget, mac));
"""


class Ledger:
    def __init__(self, path, policy, chunk=2.0):  # chunk: the smallest slice, in dollars
        self.db = sqlite3.connect(path, check_same_thread=False, isolation_level=None)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        # On a Mac, FULL is a plain fsync(2), which leaves the commit in the drive's cache; these
        # make each commit and each checkpoint F_FULLFSYNC (research/2026-11-25-durability-on-a-mac).
        self.db.execute("PRAGMA fullfsync=ON")
        self.db.execute("PRAGMA checkpoint_fullfsync=ON")
        self.db.executescript(SCHEMA)
        self.mu = threading.Lock()
        self.policy, self.chunk = policy, chunk
        self.books = {}
        self.short = {}  # budget -> time a Mac was last refused for lack of free room
        for name, total in self.db.execute("SELECT name, total FROM budgets"):
            self._load(name, total)

    def _load(self, name, total):
        b = Book(total, self.policy, TPS, P_OUT)
        for cid, started, inp, res, cost in self.db.execute(
                "SELECT id, started, input, reserve, cost FROM calls WHERE budget=?", (name,)):
            if cost is None:
                b.start(cid, started, inp, res)
            else:
                b.settled += cost
        self.books[name] = b

    def ensure(self, name, total):
        with self.mu:
            self.db.execute("INSERT OR IGNORE INTO budgets VALUES (?, ?)", (name, total))
            if name not in self.books:
                self._load(name, total)

    # -- central --

    def admit(self, name, cid, input_cost, reserve):
        with self.mu:
            b, now = self.books[name], time.time()
            ok = b.admits(now, input_cost, reserve)
            if ok:
                self.db.execute("INSERT INTO calls VALUES (?, ?, ?, ?, ?, NULL)", (name, cid, now, input_cost, reserve))
                b.start(cid, now, input_cost, reserve)
            return {"ok": ok, "counted": round(b.counted(now), 6), "limit": b.limit}

    def settle(self, name, cid, cost):
        with self.mu:
            cur = self.db.execute("UPDATE calls SET cost=? WHERE budget=? AND id=? AND cost IS NULL", (cost, name, cid))
            if cur.rowcount:
                self.books[name].settle(cid, cost)
            return {"ok": bool(cur.rowcount)}

    # -- leases --

    def _row(self, name, mac):
        self.db.execute("INSERT OR IGNORE INTO leases (budget, mac) VALUES (?, ?)", (name, mac))
        return self.db.execute("SELECT granted, given, reported FROM leases WHERE budget=? AND mac=?", (name, mac)).fetchone()

    def _free(self, name):
        used = self.db.execute("SELECT COALESCE(SUM(MAX(granted - given, reported)), 0) FROM leases WHERE budget=?", (name,)).fetchone()[0]
        return max(0.0, self.books[name].limit - used)

    def lease(self, name, mac, spent, req=None):
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
                        return {"granted": last[1], "recall": self._recall(name, now), "again": True}
                self.db.execute("UPDATE leases SET reported=MAX(reported, ?), asked=? WHERE budget=? AND mac=?", (spent, now, name, mac))
                free = self._free(name)
                active = self.db.execute("SELECT COUNT(*) FROM leases WHERE budget=? AND asked > ?", (name, now - 60)).fetchone()[0]
                g = min(free, max(self.chunk, free / (2 * max(1, active))))
                self.db.execute("UPDATE leases SET granted = granted + ?, last_req = ?, last_grant = ? WHERE budget=? AND mac=?", (g, req, g, name, mac))
                self.db.execute("COMMIT")
                if CRASH_AFTER_COMMIT and g > 0 and random.random() < CRASH_AFTER_COMMIT:
                    os._exit(9)
            except BaseException:
                self.db.execute("ROLLBACK")
                raise
            if free < self.chunk:
                self.short[name] = now
            return {"granted": g, "recall": self._recall(name, now)}

    def give(self, name, mac, amount, spent):
        with self.mu:
            self.db.execute("BEGIN IMMEDIATE")
            try:
                self._row(name, mac)
                self.db.execute("UPDATE leases SET given = given + ?, reported = MAX(reported, ?) WHERE budget=? AND mac=?", (amount, spent, name, mac))
                self.db.execute("COMMIT")
            except BaseException:
                self.db.execute("ROLLBACK")
                raise
            return {"ok": True, "recall": self._recall(name, time.time())}

    def _recall(self, name, now):
        return now - self.short.get(name, 0) < 10

    def state(self, name):
        with self.mu:
            rows = self.db.execute("SELECT mac, granted, given, reported FROM leases WHERE budget=? ORDER BY mac", (name,)).fetchall()
            b = self.books[name]
            return {"limit": b.limit, "settled": round(b.settled, 6), "in_flight": len(b.flight), "free": round(self._free(name), 6),
                    "leases": [{"mac": m, "granted": g, "given": gv, "reported": r} for m, g, gv, r in rows]}


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
            if u.path == "/state":
                return self.reply(ledger.state(parse_qs(u.query)["budget"][0]))
            self.reply({"error": "not here"}, 404)

        def do_POST(self):
            n = int(self.headers.get("content-length") or 0)
            o = json.loads(self.rfile.read(n) or b"{}")
            try:
                if self.path == "/admit":
                    return self.reply(ledger.admit(o["budget"], o["call"], o["input_cost"], o.get("reserve", 0.0)))
                if self.path == "/settle":
                    return self.reply(ledger.settle(o["budget"], o["call"], o["cost"]))
                if self.path == "/lease":
                    return self.reply(ledger.lease(o["budget"], o["mac"], o.get("spent", 0.0), o.get("req")))
                if self.path == "/give":
                    return self.reply(ledger.give(o["budget"], o["mac"], o["amount"], o.get("spent", 0.0)))
            except KeyError as e:
                return self.reply({"error": f"unknown {e}"}, 400)
            self.reply({"error": "not here"}, 404)
    return H


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="ledger.db")
    ap.add_argument("--port", type=int, default=8730)
    ap.add_argument("--budget", action="append", default=[], help="name=dollars")
    ap.add_argument("--policy", default="reserve_typical")
    ap.add_argument("--chunk", type=float, default=2.0, help="the smallest slice, in dollars")
    a = ap.parse_args()
    ledger = Ledger(a.db, a.policy, a.chunk)
    for spec in a.budget:
        name, total = spec.split("=")
        ledger.ensure(name, float(total))
    srv = ThreadingHTTPServer(("127.0.0.1", a.port), handler(ledger))
    print(f"ledger on 127.0.0.1:{a.port}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
