"""Grouping policies for asks, run live inside proxy.py with a simulated user.

    python3 proxy.py ... --policy policy.json

policy.json:
    {"hold_ops": "regex on service:Operation",   which requests are held
     "group": "request" | "host" | "computer" | "connection" | "program",
     "window": 3,            seconds an ask gathers after its first request before it is shown
     "join_while_shown": true,  a request that matches an ask already shown joins it (the list grows)
     "retry_join": true,     a copy of a held request (same amz-sdk-invocation-id, or same
                             method+path+body within retry_window) joins that request's line
     "retry_window": 120,
     "answer_after": 8,      seconds the user takes on each ask, one ask at a time
     "answer": "yes",
     "hold_limit": 90}

The rule that never bends: a yes covers exactly the lines the user saw, which is the version of
the list shown one second before the tap. A request that joined later, or arrives after the tap,
goes to a new ask. Asks are written to the --asks log, one JSON line each.
"""
import itertools
import json
import re
import threading
import time

SEEN_LAG = 1.0          # a line must have been on screen this long to be covered by a tap
ask_ids = itertools.count(1)


class Line:
    def __init__(self, rec, t):
        self.rec = rec
        self.joined = t
        self.copies = [threading.Event()]   # one event per waiting copy; the newest is released
        self.decision = None
        self.inv = rec.get("invocation_id")
        self.identity = rec["identity"]


class Ask:
    def __init__(self, key, t, window):
        self.id = next(ask_ids)
        self.key = key
        self.opened = t
        self.shown_at = t + window
        self.lines = []
        self.answered = None


class Engine:
    def __init__(self, cfg, log_path, clock):
        self.cfg = cfg
        self.clock = clock
        self.log = open(log_path, "a")
        self.lock = threading.Lock()
        self.asks = []          # open asks, oldest first
        self.user_free = 0.0
        self.current = None     # ask the user is looking at, and when they started
        self.done_lines = []    # (t, line) decided lines, for retry matching after the answer
        threading.Thread(target=self.user, daemon=True).start()

    def holds(self, op):
        return re.fullmatch(self.cfg["hold_ops"], op) is not None

    def key(self, rec):
        g = self.cfg["group"]
        if g == "request":
            return ("request", rec["id"])
        if g == "host":
            return ("host", rec["host"])
        if g == "connection":
            return ("connection", rec["conn"])
        if g == "program":
            m = re.search(r"(Terraform/[\d.]+|aws-cli/[\d.]+|[\w-]+/[\d.]+)", rec.get("ua") or "")
            return ("program", m.group(1) if m else rec.get("ua"))
        return ("computer",)

    def find_copy(self, rec, t):
        if not self.cfg.get("retry_join"):
            return None
        win = self.cfg.get("retry_window", 120)
        for a in self.asks:
            for ln in a.lines:
                if ln.decision is None and t - ln.joined <= win and (
                        (rec.get("invocation_id") and rec["invocation_id"] == ln.inv) or rec["identity"] == ln.identity):
                    return ln
        return None

    def hold(self, rec):
        """Blocks until the request's line is decided or its hold limit passes. Returns
        ("yes"|"no"|"limit"|"replaced", ask id)."""
        t = self.clock()
        with self.lock:
            ln = self.find_copy(rec, t)
            if ln:
                ev = threading.Event()
                ln.copies.append(ev)
                rec["joined_as_copy_of"] = ln.rec["id"]
                ask = next(a for a in self.asks if ln in a.lines)
            else:
                ln = Line(rec, t)
                ev = ln.copies[0]
                k = self.key(rec)
                ask = None
                for a in self.asks:
                    if a.key == k and a.answered is None and (t < a.shown_at or self.cfg.get("join_while_shown", True)):
                        ask = a
                        break
                if ask is None:
                    ask = Ask(k, t, self.cfg.get("window", 0))
                    self.asks.append(ask)
                ask.lines.append(ln)
        limit = self.cfg.get("hold_limit", 90)
        ev.wait(limit)
        with self.lock:
            if not ev.is_set():
                ln.copies.remove(ev)
                return "limit", ask.id
            if ev is not ln.copies[-1] and ln.decision == "yes":
                return "replaced", ask.id
            return ln.decision, ask.id

    def user(self):
        """One person, answering one ask at a time, `answer_after` seconds each."""
        r = self.cfg.get("answer_after", 8)
        while True:
            time.sleep(0.05)
            t = self.clock()
            with self.lock:
                if self.current is None:
                    shown = [a for a in self.asks if a.answered is None and a.shown_at <= t]
                    if shown and t >= self.user_free:
                        self.current = (shown[0], t)
                    continue
                ask, began = self.current
                if t < began + r:
                    continue
                covered = [ln for ln in ask.lines if ln.joined <= t - SEEN_LAG]
                late = [ln for ln in ask.lines if ln.joined > t - SEEN_LAG]
                ask.answered = t
                ans = self.cfg.get("answer", "yes")
                for ln in covered:
                    ln.decision = ans
                    if not ln.copies:
                        ln.late = True      # every copy hit the hold limit: a late yes, kept as a one-use grant
                    elif ans == "yes":
                        ln.copies[-1].set()
                        for ev in ln.copies[:-1]:
                            ev.set()
                    else:
                        for ev in ln.copies:
                            ev.set()
                self.asks.remove(ask)
                if late:   # lines the user did not see: a new ask, shown at once
                    nxt = Ask(ask.key, t, 0)
                    nxt.lines = late
                    self.asks.insert(0, nxt)
                self.log.write(json.dumps({
                    "ask": ask.id, "key": list(ask.key), "opened": round(ask.opened, 3),
                    "shown": round(ask.shown_at, 3), "answered": round(t, 3), "answer": ans,
                    "lines": [{"id": ln.rec["id"], "op": ln.rec["op"], "path": ln.rec["path"],
                               "copies": len(ln.copies), "late": getattr(ln, "late", False), "waited": round(t - ln.joined, 3)} for ln in covered],
                    "moved_to_new_ask": [ln.rec["id"] for ln in late]}) + "\n")
                self.log.flush()
                self.current = None
                self.user_free = t
