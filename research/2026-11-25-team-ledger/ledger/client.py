"""The boundary's side of a team budget: what one Mac's boundary runs to admit its model calls
against a budget other Macs share. Three ways, one interface, threads only (each agent's call
runs on its own thread, as the boundary serves each connection on its own):

  Local    one book in this process: one boundary that sees every call (the baseline)
  Central  asks the ledger before every call (/admit), tells it the cost after (/settle); the
           ledger says when to cut calls under way
  Lease    holds a slice from the ledger and admits locally with #374's rule; asks for more when
           what is left falls under `low`; gives back when recalled, and when its agents end; cuts
           on its own with the simulator's rule; with a ttl, renews its slice while its agents run
           and stops spending it on its own clock before it ends

Interface: admit(cid, input_cost, reserve) -> bool, waiting as long as the ledger asks;
settle(cid, cost); admit_whole(wid, bound) -> bool; settle_whole(wid, cost); check_cut() -> bool,
called every half second; done(). `on_cut` is set by the Mac and cuts every call under way.

Every message to the ledger is counted in `messages` (a request and its answer are two, as in the
simulator); news from /watch, one each.
"""
import http.client, json, os, sys, threading, time, uuid

sys.path.insert(0, os.path.dirname(__file__))
from book import Book  # noqa: E402

P_OUT = 20e-6


class Down(Exception):
    pass


class Http:
    """One request per connection, as small as can be. `timeout` is per try."""

    def __init__(self, host, port, timeout=2.0):
        self.host, self.port, self.timeout = host, port, timeout
        self.messages = 0
        self.failures = 0
        self.mu = threading.Lock()

    def once(self, method, path, body=None, timeout=None):
        c = http.client.HTTPConnection(self.host, self.port, timeout=timeout or self.timeout)
        try:
            c.request(method, path, json.dumps(body) if body is not None else None, {"content-type": "application/json"})
            r = c.getresponse()
            data = r.read()
            if r.status != 200:
                raise Down(f"{r.status} {data[:200]!r}")
            return json.loads(data)
        except (OSError, http.client.HTTPException, json.JSONDecodeError) as e:
            with self.mu:
                self.failures += 1
            raise Down(str(e)) from e
        finally:
            c.close()

    def until(self, method, path, body=None, stop=None, pause=0.25):
        """Send until answered; the same body each time, so a request id makes it safe."""
        while True:
            with self.mu:
                self.messages += 2
            try:
                return self.once(method, path, body)
            except Down:
                if stop is not None and stop():
                    raise
                time.sleep(pause)


class Way:
    def __init__(self):
        self.on_cut = lambda: None
        self.waits = []
        self.events = 0
        self.mu = threading.Lock()

    def check_cut(self):
        return False

    def done(self):
        pass

    def stats(self):
        return {"waits": self.waits, "events": self.events}


class Local(Way):
    """One boundary: one book, no messages."""

    def __init__(self, limit, policy, tps):
        super().__init__()
        self.book = Book(limit, policy, tps, P_OUT)

    def admit(self, cid, inp, res):
        with self.mu:
            ok = self.book.admits(time.time(), inp, res)
            if ok:
                self.book.start(cid, time.time(), inp, res)
                self.waits.append(0.0)
            return ok

    def settle(self, cid, cost):
        with self.mu:
            self.book.settle(cid, cost)

    def admit_whole(self, wid, bound):
        with self.mu:
            ok = self.book.admits_whole(time.time(), bound)
            if ok:
                self.book.start_whole(wid, bound)
            return ok

    def settle_whole(self, wid, cost):
        with self.mu:
            self.book.settle_whole(wid, cost)

    def check_cut(self):
        with self.mu:
            return bool(self.book.flight) and self.book.counted(time.time()) >= self.book.limit

    def stats(self):
        return dict(super().stats(), messages=0, failures=0)


class Watcher(threading.Thread):
    """Holds a /watch open to the ledger and calls `news(kind)` for each cut or recall."""

    def __init__(self, http, budget, mac, news):
        super().__init__(daemon=True)
        self.http, self.budget, self.mac, self.news = http, budget, mac, news
        self.boot, self.seen = None, {"gen": -1, "cut": 0, "recall": 0}
        self.stopped = False
        self.events = 0

    def run(self):
        while not self.stopped:
            q = f"/watch?budget={self.budget}&mac={self.mac}&after={self.seen['gen']}&boot={self.boot or ''}&wait=10"
            try:
                r = self.http.once("GET", q, timeout=12 + self.http.timeout)
            except Down:
                time.sleep(0.25)
                continue
            if r["boot"] != self.boot:  # a new ledger: its counters start again
                fresh = self.boot is None
                self.boot, self.seen = r["boot"], {"gen": r["gen"], "cut": 0, "recall": 0}
                if fresh:
                    self.seen = {"gen": r["gen"], "cut": r["cut"], "recall": r["recall"]}
                    continue
            for kind in ("cut", "recall"):
                if r[kind] > self.seen[kind]:
                    self.events += 1
                    self.news(kind)
            self.seen = {"gen": r["gen"], "cut": r["cut"], "recall": r["recall"]}


class Central(Way):
    """`on_down`: `hold` (a call waits for the ledger) or `deny` (refused after one failed try)."""

    def __init__(self, http, budget, mac, on_down="hold"):
        super().__init__()
        self.http, self.budget, self.mac, self.on_down = http, budget, mac, on_down
        self.queue, self.qcv = [], threading.Condition()
        threading.Thread(target=self._sender, daemon=True).start()
        self.watcher = Watcher(http, budget, mac, self._news)
        self.watcher.start()

    def _news(self, kind):
        if kind == "cut":
            self.on_cut()

    def _sender(self):
        """Settles go out in the background, in order, until each is answered."""
        while True:
            with self.qcv:
                while not self.queue:
                    self.qcv.wait()
                path, body = self.queue[0]
            self.http.until("POST", path, body)
            with self.qcv:
                self.queue.pop(0)

    def admit(self, cid, inp, res):
        t0 = time.time()
        body = {"budget": self.budget, "call": cid, "input_cost": inp, "reserve": res, "mac": self.mac}
        try:
            r = self.http.until("POST", "/admit", body, stop=lambda: self.on_down == "deny")
        except Down:
            return False
        if r["ok"]:
            with self.mu:
                self.waits.append(time.time() - t0)
        return r["ok"]

    def settle(self, cid, cost):
        with self.qcv:
            self.queue.append(("/settle", {"budget": self.budget, "call": cid, "cost": cost}))
            self.qcv.notify()

    def admit_whole(self, wid, bound):
        r = self.http.until("POST", "/admit_whole", {"budget": self.budget, "id": wid, "bound": bound, "mac": self.mac})
        return r["ok"]

    def settle_whole(self, wid, cost):
        with self.qcv:
            self.queue.append(("/settle_whole", {"budget": self.budget, "id": wid, "cost": cost}))
            self.qcv.notify()

    def stats(self):
        return dict(super().stats(), events=self.watcher.events, messages=self.http.messages + self.watcher.events, failures=self.http.failures)


class Lease(Way):
    def __init__(self, http, budget, mac, policy, tps, low=1.0, rtt=0.03, margin=5.0, keep=1.0, rearm=True):
        super().__init__()
        # keep: on a recall, the room this Mac keeps while calls of its own are under way, so they
        # can finish without passing its slice at once. 0 gives back everything not held, as the
        # simulator and #388 did. rearm: an answer that came but left the Mac still over its slice
        # (a grant taken back by a recall) starts the wait again, instead of counting as no answer.
        self.keep, self.rearm = keep, rearm
        self.answered_at = 0.0
        self.http, self.budget, self.mac = http, budget, mac
        self.book = Book(0.0, policy, tps, P_OUT)
        self.low, self.rtt, self.margin = low, rtt, margin
        self.talk = threading.Lock()  # one request to the ledger at a time, so answers stay in order
        self.pending = None           # an Event while an ask is under way
        self.empty = False            # the ledger's last answer was: nothing left
        self.over_since = None
        self.seq = -1
        self.ttl = 0.0
        self.expires = None           # on this Mac's clock: when its slice ends
        self.asks = self.gives = self.renews = 0
        self.live = True
        self.trace = []               # every answer of the ledger, for the run's log
        self.watcher = Watcher(http, budget, mac, self._news)
        self.watcher.start()
        threading.Thread(target=self._renewer, daemon=True).start()

    # -- talking to the ledger --

    def _apply(self, r, sent_at):
        """Under self.mu. `slice` is all the room the ledger holds for this Mac."""
        if r["seq"] >= self.seq:
            self.seq = r["seq"]
            self.book.limit = r["slice"]
        self.ttl = r.get("ttl") or 0.0
        self.expires = sent_at + self.ttl if self.ttl else None

    def _ask(self, want=0.0, renew=False):
        q = time.time()
        with self.talk:
            if time.time() - q > 0.1:
                self.trace.append({"t": round(q, 3), "queued": round(time.time() - q, 3), "ask": want})
            req = uuid.uuid4().hex
            with self.mu:
                now = time.time()
                body = {"budget": self.budget, "mac": self.mac, "spent": self.book.settled, "held": self.book.held(now),
                        "want": want, "req": req, "renew": renew}
            sent = time.time()
            r = self.http.until("POST", "/lease", body)
            with self.mu:
                self._apply(r, sent)
                self.answered_at = time.time()
                self.trace.append({"t": round(sent, 3), "took": round(time.time() - sent, 3), "ask": "renew" if renew else want,
                                   "granted": r.get("granted"), "slice": r["slice"], "held": round(body["held"], 4)})
                if renew:
                    self.renews += 1
                else:
                    self.asks += 1
                    self.empty = r["granted"] <= 1e-9
            return r

    def ask(self, want=0.0):
        """Start an ask unless one is under way; returns the Event that is set when it is answered."""
        with self.mu:
            if self.pending is not None:
                return self.pending
            ev = self.pending = threading.Event()

        def go():
            try:
                self._ask(want)
            finally:
                with self.mu:
                    self.pending = None
                ev.set()
        threading.Thread(target=go, daemon=True).start()
        return ev

    def _give(self, amount):
        q = time.time()
        with self.talk:
            if time.time() - q > 0.1:
                self.trace.append({"t": round(q, 3), "queued": round(time.time() - q, 3), "give": round(amount, 4)})
            with self.mu:
                body = {"budget": self.budget, "mac": self.mac, "amount": amount, "spent": self.book.settled,
                        "held": self.book.held(time.time()), "req": uuid.uuid4().hex}
            sent = time.time()
            r = self.http.until("POST", "/give", body)
            with self.mu:
                self._apply(r, sent)
                self.gives += 1
                self.trace.append({"t": round(sent, 3), "took": round(time.time() - sent, 3), "give": round(amount, 4), "slice": r["slice"]})

    def _news(self, kind):
        if kind != "recall":
            return
        with self.mu:
            keep = self.keep if self.book.flight else 0.0
            r = max(0.0, self.book.limit - self.book.held(time.time()) - keep)
            self.book.limit -= r
            if r > 0:
                self.empty = False
        threading.Thread(target=self._give, args=(r,), daemon=True).start()

    def _renewer(self):
        while self.live:
            time.sleep(1.0)
            with self.mu:
                due = self.ttl and self.expires is not None and time.time() > self.expires - self.ttl * 2 / 3
                holds = self.book.limit > 1e-9
            if due and holds and self.live:
                try:
                    self._ask(renew=True)
                except Down:
                    pass

    def _lapsed(self, now):
        """Past the end of its slice, on its own clock, less a margin: the Mac must not spend it."""
        return self.expires is not None and now > self.expires - self.margin

    # -- the way --

    def room(self, now):
        return self.book.limit - self.book.held(now)

    def admit(self, cid, inp, res):
        t0 = time.time()
        while True:
            with self.mu:
                now = time.time()
                lapsed = self._lapsed(now)
                ok = not lapsed and self.book.admits(now, inp, res)
                if ok:
                    self.book.start(cid, now, inp, res)
                    self.waits.append(now - t0)
                    early = self.room(now) < self.low and not self.empty
                elif self.empty and self.pending is None and not lapsed:
                    return False
            if ok:
                if early:
                    self.ask()  # ahead of need, so the next call does not wait
                return True
            if lapsed:
                self._ask(renew=False) if self.pending is None else self.pending.wait()
            else:
                self.ask().wait()

    def settle(self, cid, cost):
        with self.mu:
            self.book.settle(cid, cost)

    def admit_whole(self, wid, bound):
        while True:
            with self.mu:
                now = time.time()
                ok = not self._lapsed(now) and self.book.admits_whole(now, bound)
                if ok:
                    self.book.start_whole(wid, bound)
                    return True
                if self.empty and self.pending is None:
                    return False
                want = bound - self.room(now)
            self.ask(want=want).wait()

    def settle_whole(self, wid, cost):
        with self.mu:
            self.book.settle_whole(wid, cost)

    def check_cut(self):
        """The simulator's rule: once what this Mac counts passes its slice, ask once more; cut when
        the ledger answers that nothing is left, or when no answer came within a second and a round
        trip, since this Mac cannot know what the others spent."""
        with self.mu:
            now = time.time()
            if not self.book.flight:
                self.over_since = None
                return False
            if self.book.counted(now) < self.book.limit and not self._lapsed(now):
                self.over_since = None
                return False
            if self.over_since is not None and self.rearm and not self.empty and self.answered_at > self.over_since and self.pending is None:
                self.over_since = None  # answered, yet still over: ask again rather than cut
            if self.over_since is None:
                self.over_since = now
                self.empty = False
                start = True
            else:
                start = False
            cut = (self.empty and self.pending is None) or now - self.over_since > 1 + self.rtt
            if cut:
                self.trace.append({"t": round(now, 3), "cut": "empty" if self.empty and self.pending is None else "no answer",
                                   "counted": round(self.book.counted(now), 4), "slice": self.book.limit})
        if start:
            self.ask()
        return cut

    def done(self):
        self.live = False
        with self.mu:
            back = max(0.0, self.book.limit - self.book.settled - sum(self.book.whole.values()))
            self.book.limit -= back
        if back > 0:
            self._give(back)

    def stats(self):
        return dict(super().stats(), events=self.watcher.events, messages=self.http.messages + self.watcher.events,
                    failures=self.http.failures, asks=self.asks, gives=self.gives, renews=self.renews, trace=self.trace)
