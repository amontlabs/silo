"""The ask path end to end, as one process on the Mac's side.

It plays four parts of Silo, each a thread:

  the boundary   an HTTP server standing in for the hosts main reaches (a Git server, a payments
                 API, S3, a package index). A rule holds pushes, POSTs, PUTs, DELETEs and the
                 package index. The hold lasts per client, as the proposed `hold-limit-length` says.
  the asks       one open ask per computer (`asks-for-a-burst`), lines matched by fingerprint, an
                 ask that outlives its hold and a late yes that lets the same request pass once
                 (`hold-sentence` as #364 rewrites it). Every change is a control line in the record.
  the forwarder  reads the record's control lines, as an extension granted to read them would
                 (`forwarding-tool`), waits GATHER seconds for a burst to settle, and sends each new
                 version of an ask to the phone, sealed and signed, through the relay.
  the answers    long-polls the relay for answers from the paired phone, checks each one (signed by
                 the phone, for this ask and exactly the list it saw, fresh, not seen before) and
                 applies it. Only an answer signed by Silo's own phone app counts (`ask-reaches-you`).

Environment: PORT (boundary), RELAY (http://127.0.0.1:8810), PAIRING (pairing.json), RECORD (path),
GIT_ROOT, HOLDS (JSON of client -> seconds), GATHER (2), ASK_LIFE (1800), KEEPALIVE (30), WHO (polarzero).
"""
import hashlib, http.server, json, os, select, socket, subprocess, threading, time, urllib.request
import link

PORT = int(os.environ.get("PORT", "8820"))
RELAY = os.environ.get("RELAY", "http://127.0.0.1:8810")
PAIR = json.load(open(os.environ.get("PAIRING", "pairing.json")))
RECORD = os.environ.get("RECORD", "record.jsonl")
GIT_ROOT = os.environ.get("GIT_ROOT", "/tmp/git-root")
HOLDS = {"git": 600, "aws": 50, "pip": 12, "default": 90}
HOLDS.update(json.loads(os.environ.get("HOLDS", "{}")))
GATHER = float(os.environ.get("GATHER", "2"))
ASK_LIFE = float(os.environ.get("ASK_LIFE", "1800"))
KEEPALIVE = float(os.environ.get("KEEPALIVE", "30"))
WHO = os.environ.get("WHO", "polarzero")
COMPUTER = "main"

rec_lock = threading.Lock()
recf = open(RECORD, "a")


def record(kind, **kv):
    kv = {"t": round(time.time(), 4), "kind": kind, **kv}
    with rec_lock:
        recf.write(json.dumps(kv) + "\n"); recf.flush()
    return kv


def client_of(ua):
    ua = (ua or "").lower()
    if ua.startswith("git/"): return "git"
    if "boto" in ua or "aws-cli" in ua or "aws-sdk" in ua: return "aws"
    if ua.startswith("pip/"): return "pip"
    return "default"


def clock(t):
    return time.strftime("%H:%M", time.localtime(t))


# ---------------------------------------------------------------- the asks

class Waiter:
    def __init__(self):
        self.ev = threading.Event(); self.outcome = None


class Line:
    def __init__(self, n, keys, desc):
        self.n, self.keys, self.desc = n, set(keys), desc
        self.waiting, self.verdict, self.spent, self.yes_until = [], None, False, None


class Ask:
    def __init__(self, n):
        self.id = f"ask_{n}"; self.lines = []; self.version = 0; self.versions = {}
        self.answered = None; self.closed = None; self.opened = time.time(); self.ends = self.opened + ASK_LIFE

    def listing(self, upto=None):
        lines = self.lines[:upto] if upto else self.lines
        return [l.desc for l in lines]

    def list_hash(self, upto=None):
        return hashlib.sha256(json.dumps(self.listing(upto)).encode()).hexdigest()[:32]


class Book:
    def __init__(self):
        self.lock = threading.Lock(); self.asks = []; self.n = 0; self.nonces = set()

    def _open_asks(self):
        return [a for a in self.asks if not a.closed]

    def _new_ask(self):
        self.n += 1; a = Ask(self.n); self.asks.append(a)
        record("ask.opened", ask=a.id, computer=COMPUTER, ends=a.ends)
        return a

    def _bump(self, a):
        a.version += 1
        a.versions[a.version] = (len(a.lines), a.list_hash())
        record("ask.version", ask=a.id, version=a.version, lines=a.listing(), list_hash=a.list_hash(),
               ends=a.ends)

    def hold(self, keys, desc):
        """Called by the boundary when a rule holds a request. Returns (line, waiter) for a request
        that must wait, or (line, outcome) when it is settled at once."""
        with self.lock:
            for a in self._open_asks():
                for l in a.lines:
                    if l.keys & set(keys):
                        l.keys |= set(keys)
                        if l.verdict == "yes" and not l.spent:
                            l.spent = True
                            record("request.passed", ask=a.id, line=l.n, why="late yes, spent once")
                            self._maybe_close(a)
                            return l, "pass"
                        if l.verdict == "no":
                            record("request.refused", ask=a.id, line=l.n, why="denied")
                            return l, "deny"
                        if l.verdict == "yes" and l.spent:
                            continue    # spent: a new ask, below
                        w = Waiter(); l.waiting.append(w)
                        record("request.joined", ask=a.id, line=l.n, desc=desc)
                        return l, w
            a = next((a for a in self._open_asks() if a.answered is None), None) or self._new_ask()
            l = Line(len(a.lines) + 1, keys, desc); l.ask = a; a.lines.append(l)
            w = Waiter(); l.waiting.append(w)
            record("request.held", ask=a.id, line=l.n, desc=desc)
            self._bump(a)
            return l, w

    def hold_ended(self, l, w, how):
        with self.lock:
            if w.outcome:
                return w.outcome
            l.waiting.remove(w)
            record("hold.ended", ask=l.ask.id, line=l.n, how=how)
            return None

    def answer(self, m):
        """Apply an answer from the phone. Returns None, or the reason it is refused."""
        with self.lock:
            a = next((a for a in self.asks if a.id == m.get("ask")), None)
            if not a: return "no such ask"
            if a.closed: return "ask closed: " + a.closed
            if a.answered is not None: return "ask already answered"
            v = m.get("version")
            if v not in a.versions: return "no such version"
            n, h = a.versions[v]
            if m.get("list_hash") != h: return "the list answered is not the list of that version"
            if m.get("verdict") not in ("yes", "no"): return "no verdict"
            if m["nonce"] in self.nonces: return "answer seen before"
            if not (m["issued"] - 300 <= time.time() <= m["expires"]): return "answer out of date"
            self.nonces.add(m["nonce"])
            a.answered = v
            record("ask.answered", ask=a.id, version=v, verdict=m["verdict"], lines=n,
                   issued=m["issued"], via="phone")
            for l in a.lines[:n]:
                l.verdict = m["verdict"]
                if l.waiting:
                    first, rest = l.waiting[-1], l.waiting[:-1]   # the newest copy passes
                    first.outcome = "pass" if l.verdict == "yes" else "deny"
                    if l.verdict == "yes": l.spent = True
                    for w in rest:
                        w.outcome = "answered-already-passed" if l.verdict == "yes" else "deny"
                    for w in l.waiting: w.ev.set()
                    l.waiting = []
                elif l.verdict == "yes":
                    l.yes_until = a.ends
            later = a.lines[n:]
            if later:   # held after the version you saw: they open the next ask
                a.lines = a.lines[:n]
                b = self._new_ask()
                for i, l in enumerate(later):
                    l.n = i + 1; l.ask = b; b.lines.append(l)
                self._bump(b)
            self._maybe_close(a)
            return None

    def _maybe_close(self, a):
        if a.answered is not None and all(l.verdict == "no" or l.spent for l in a.lines):
            a.closed = "answered and done"; record("ask.closed", ask=a.id, why=a.closed)

    def janitor(self):
        while True:
            time.sleep(1)
            with self.lock:
                for a in self._open_asks():
                    if time.time() > a.ends:
                        a.closed = "ask's own limit"; record("ask.closed", ask=a.id, why=a.closed)


BOOK = Book()


# ---------------------------------------------------------------- the boundary

def pkt(b): return b"%04x" % (len(b) + 4) + b


def ref_updates(data):
    cmds, i = [], 0
    while i + 4 <= len(data):
        n = int(data[i:i + 4], 16)
        if n == 0: return cmds, i + 4
        line = data[i + 4:i + n].split(b"\0")[0].decode(errors="replace").strip()
        cmds.append(line); i += n
    return cmds, i


def words(l, how, hold_s):
    a = l.ask
    if how == "deny":
        return [f"Silo: {WHO} said no to this request ({a.id}). Nothing was sent."]
    if how == "answered-already-passed":
        return [f"Silo: {WHO} said yes ({a.id}) and another copy of this request already passed. This copy was not sent."]
    return [f"Silo held this request for {WHO}'s answer for {int(hold_s)} s, and nobody answered yet. It was not denied. Nothing was sent.",
            f"The ask {a.id} stays open until {clock(a.ends)}. If {WHO} says yes before then, the same request passes once when it is sent again: run the same command again later."]


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a): pass

    def body(self):
        if self.headers.get("transfer-encoding", "").lower() == "chunked":
            out = b""
            while True:
                n = int(self.rfile.readline().strip(), 16)
                if n == 0: self.rfile.readline(); return out
                out += self.rfile.read(n); self.rfile.readline()
        return self.rfile.read(int(self.headers.get("content-length", 0)))

    def wait(self, l, w, hold_s, tick=None):
        """Wait for the answer, the hold's limit, or the client closing. Returns the outcome, or
        None if the hold ended without one ('closed' is returned when nothing can be sent back)."""
        start = last = time.time()
        while True:
            if w.ev.wait(0.2):
                return w.outcome
            if time.time() - start >= hold_s:
                return BOOK.hold_ended(l, w, "limit")
            r, _, _ = select.select([self.connection], [], [], 0)
            if r:
                try: peek = self.connection.recv(1, socket.MSG_PEEK)
                except OSError: peek = b""
                if not peek:
                    out = BOOK.hold_ended(l, w, "client closed")
                    return out or "closed"
            if tick and time.time() - last >= KEEPALIVE:
                tick(int(time.time() - start)); last = time.time()

    def git_backend(self, method, data):
        path, _, query = self.path.partition("?")
        env = dict(os.environ, GIT_PROJECT_ROOT=GIT_ROOT, GIT_HTTP_EXPORT_ALL="1", PATH_INFO=path,
                   QUERY_STRING=query, REQUEST_METHOD=method, CONTENT_TYPE=self.headers.get("content-type", ""),
                   CONTENT_LENGTH=str(len(data)), REMOTE_USER="agent", REMOTE_ADDR="127.0.0.1")
        p = subprocess.run(["git", "http-backend"], input=data, env=env, capture_output=True)
        head, _, rest = p.stdout.partition(b"\r\n\r\n")
        status, hdrs = 200, []
        for line in head.decode().split("\r\n"):
            k, _, v = line.partition(":")
            if k.lower() == "status": status = int(v.strip().split()[0])
            elif k: hdrs.append((k, v.strip()))
        return status, hdrs, rest

    def push(self, data):
        cmds, _ = ref_updates(data)
        host = self.headers.get("host"); repo = self.path.split("/git-receive-pack")[0]
        refs = ", ".join(c.split(" ", 2)[2].replace("refs/heads/", "") for c in cmds)
        desc = f"push {refs} to {host}{repo} ({cmds[0].split()[1][:7] if cmds else ''})"
        keys = ["push|" + host + repo + "|" + "|".join(sorted(cmds))]
        l, w = BOOK.hold(keys, desc)
        hold_s = HOLDS["git"]
        self.send_response(200)
        self.send_header("content-type", "application/x-git-receive-pack-result")
        self.send_header("transfer-encoding", "chunked"); self.end_headers()
        def chunk(b):
            self.wfile.write(b"%x\r\n" % len(b) + b + b"\r\n"); self.wfile.flush()
        def band2(s): chunk(pkt(b"\x02" + (s + "\n").encode()))
        if isinstance(w, str):
            outcome = w
        else:
            band2(f"Silo holds this push for {WHO}'s answer ({l.ask.id}). It waits up to {hold_s} s. Other work can go on.")
            outcome = self.wait(l, w, hold_s, tick=lambda s: band2(f"still held, {s} s"))
        if outcome == "closed":
            return
        if outcome == "pass":
            band2(f"Silo: {WHO} said yes ({l.ask.id}). Pushing.")
            st, hd, rest = self.git_backend("POST", data)
            chunk(rest); chunk(b"")
            record("request.forwarded", ask=l.ask.id, line=l.n, status=st)
            return
        for s in words(l, outcome or "limit", hold_s): band2(s)
        why = {"deny": f"denied by {WHO}", "answered-already-passed": "another copy passed"}.get(outcome, "held by Silo, not answered")
        report = pkt(b"unpack ok\n") + b"".join(pkt(f"ng {c.split(' ', 2)[2]} {why}\n".encode()) for c in cmds) + b"0000"
        chunk(pkt(b"\x01" + report) + b"0000"); chunk(b"")
        record("request.refused", ask=l.ask.id, line=l.n, why=outcome or "held, not answered")

    def api(self, method, data):
        host = self.headers.get("host"); path = self.path
        client = client_of(self.headers.get("user-agent"))
        keys = [f"{method}|{host}|{path}|" + hashlib.sha256(data).hexdigest()]
        if self.headers.get("amz-sdk-invocation-id"):
            keys.append("aws-inv|" + self.headers["amz-sdk-invocation-id"])
        desc = f"{method} {host}{path}" + (f" ({len(data)} bytes)" if data else "")
        l, w = BOOK.hold(keys, desc)
        hold_s = HOLDS[client]
        outcome = w if isinstance(w, str) else self.wait(l, w, hold_s)
        if outcome == "closed":
            return
        if outcome == "pass":
            if client == "pip":
                b = b"<!DOCTYPE html><html><body></body></html>\n"; ct = "text/html"
            elif client == "aws":
                b = b""; ct = "application/xml"
            else:
                b = b'{"id":"ch_1","status":"succeeded"}\n'; ct = "application/json"
            self.send_response(200); self.send_header("content-type", ct)
            if client == "aws": self.send_header("ETag", '"' + hashlib.md5(data).hexdigest() + '"')
            self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
            record("request.forwarded", ask=l.ask.id, line=l.n, status=200)
            return
        text = " ".join(words(l, outcome or "limit", hold_s))
        if client == "aws":
            b = (f"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>AccessDenied</Code><Message>{text}</Message>"
                 f"<RequestId>silo</RequestId></Error>").encode(); ct = "application/xml"
        else:
            b = (text + "\n").encode(); ct = "text/plain; charset=utf-8"
        try:
            self.send_response(403); self.send_header("content-type", ct)
            self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
        except OSError:
            pass
        record("request.refused", ask=l.ask.id, line=l.n, why=outcome or "held, not answered")

    def handle_any(self, method):
        data = self.body() if method in ("POST", "PUT", "PATCH", "DELETE") else b""
        path = self.path.split("?")[0]
        if ".git/" in path:
            if method == "POST" and path.endswith("git-receive-pack"):
                return self.push(data)
            st, hd, rest = self.git_backend(method, data)
            self.send_response(st)
            for k, v in hd: self.send_header(k, v)
            self.send_header("content-length", str(len(rest))); self.end_headers(); self.wfile.write(rest)
            return
        if method == "GET" and not path.startswith("/simple/"):
            b = b'{"ok":true}\n'
            self.send_response(200); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
            return
        self.api(method, data)

    def do_GET(self): self.handle_any("GET")
    def do_HEAD(self): self.handle_any("GET")
    def do_POST(self): self.handle_any("POST")
    def do_PUT(self): self.handle_any("PUT")
    def do_DELETE(self): self.handle_any("DELETE")


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True


# ---------------------------------------------------------------- the forwarder and the answers

def relay_post(topic, data):
    req = urllib.request.Request(f"{RELAY}/t/{topic}", data=data, method="POST")
    return json.load(urllib.request.urlopen(req, timeout=10))


def relay_poll(topic, since, wait=20):
    return json.load(urllib.request.urlopen(f"{RELAY}/t/{topic}?since={since}&wait={wait}", timeout=wait + 10))


def forwarder():
    """Reads the record's control lines, as an extension granted to read them would, and sends the
    newest version of each ask once it has been quiet for GATHER seconds."""
    pending = {}   # ask -> (last change time, line)
    f = open(RECORD)
    f.seek(0, 2)
    while True:
        for raw in f.readlines():
            m = json.loads(raw)
            if m["kind"] == "ask.version":
                pending[m["ask"]] = (time.time(), m)
            elif m["kind"] in ("ask.answered", "ask.closed"):
                pending.pop(m["ask"], None)
        for ask, (t, m) in list(pending.items()):
            if time.time() - t >= GATHER:
                del pending[ask]
                msg = {"type": "ask", "ask": ask, "version": m["version"], "lines": m["lines"],
                       "list_hash": m["list_hash"], "computer": COMPUTER, "ends": m["ends"], "sent": time.time()}
                blob = link.seal(msg, PAIR["mac"]["sign"], PAIR["phone"]["box_pub"])
                relay_post(PAIR["to_phone"], blob)
                record("ask.forwarded", ask=ask, version=m["version"], size=len(blob))
        time.sleep(0.05)


def answers():
    since = 0
    while True:
        try:
            msgs = relay_poll(PAIR["to_mac"], since)
        except Exception as e:
            record("relay.error", err=str(e)); time.sleep(1); continue
        for m in msgs:
            since = max(since, m["seq"])
            try:
                a = link.open_(link.unb64(m["b"]), PAIR["mac"]["box"], PAIR["phone"]["sign_pub"])
            except link.Refused as e:
                record("answer.refused", why=str(e)); continue
            record("answer.received", ask=a.get("ask"), version=a.get("version"), verdict=a.get("verdict"))
            why = BOOK.answer(a)
            if why:
                record("answer.refused", ask=a.get("ask"), why=why)


if __name__ == "__main__":
    for fn in (forwarder, answers, BOOK.janitor):
        threading.Thread(target=fn, daemon=True).start()
    record("listening", port=PORT, holds=HOLDS, gather=GATHER)
    S(("127.0.0.1", PORT), H).serve_forever()
