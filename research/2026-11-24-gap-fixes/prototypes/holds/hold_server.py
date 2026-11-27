"""A stand-in for Silo's boundary holding one request.

Plain HTTP on 127.0.0.1. Two kinds of path:
  /repo.git/...   a Git smart-HTTP server (git http-backend). The ref listing passes at once; the
                  POST of git-receive-pack (the push itself) is held.
  anything else   an API stand-in. GET passes at once; POST, PUT, PATCH and DELETE are held.

A held request waits HOLD seconds (or forever with HOLD=-1), then gets the status 403 and a body
in Silo's words, held and not answered. While it waits, the server watches the socket and logs the
moment the client closes it (what `program-gives-up-first` calls the close). With MODE=allow the
held request is passed after the wait, as if you had said yes in time.

For a push, the log also keeps the ref updates the client asked for (old SHA, new SHA, ref) and a
hash of the pack, to measure what stays the same between two tries of one push.

Run: HOLD=90 PORT=8700 GIT_ROOT=/path/to/bare/parent python3 hold_server.py
"""
import hashlib, http.server, json, os, select, socket, subprocess, sys, threading, time

HOLD = float(os.environ.get("HOLD", "90"))
MODE = os.environ.get("MODE", "refuse")
GIT_ROOT = os.environ.get("GIT_ROOT", "/tmp/git-root")
LOG = open(os.environ.get("LOGF", "hold.log"), "a")
T0 = time.time()
lock = threading.Lock()

BODY = (
    "Silo held this request for your answer, and nobody answered within 90 seconds.\n"
    "It was not denied. Nothing was sent.\n"
    "The ask stays open in polarzero's Silo app. If they say yes, the same request will pass "
    "once in the next 30 minutes: run the same command again then.\n"
    "Ask id: ask_7f3k2. Request: POST {path} on {host}.\n"
)


def log(**kv):
    kv["t"] = round(time.time() - T0, 2)
    with lock:
        LOG.write(json.dumps(kv) + "\n"); LOG.flush()


def pkt_commands(data):
    """The ref updates at the start of a receive-pack request: pkt-lines up to the flush."""
    cmds, i = [], 0
    while i + 4 <= len(data):
        n = int(data[i:i + 4], 16)
        if n == 0:
            return cmds, i + 4
        line = data[i + 4:i + n].split(b"\0")[0].decode(errors="replace").strip()
        old, new, ref = line.split(" ", 2)
        cmds.append({"old": old, "new": new, "ref": ref})
        i += n
    return cmds, i


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def body(self):
        if self.headers.get("transfer-encoding", "").lower() == "chunked":
            out = b""
            while True:
                n = int(self.rfile.readline().strip(), 16)
                if n == 0:
                    self.rfile.readline(); return out
                out += self.rfile.read(n); self.rfile.readline()
        return self.rfile.read(int(self.headers.get("content-length", 0)))

    def wait(self, what):
        """Hold: wait HOLD seconds, noticing a close by the client. True if the client is still there."""
        start = time.time()
        log(ev="held", what=what, ua=self.headers.get("user-agent"))
        while HOLD < 0 or time.time() - start < HOLD:
            r, _, _ = select.select([self.connection], [], [], 0.5)
            if r:
                try:
                    peek = self.connection.recv(1, socket.MSG_PEEK)
                except OSError:
                    peek = b""
                if not peek:
                    log(ev="client-closed", what=what, after=round(time.time() - start, 2))
                    return False
        log(ev="hold-ended", what=what, after=round(time.time() - start, 2))
        return True

    def refuse(self, path):
        b = BODY.format(path=path, host=self.headers.get("host")).encode()
        try:
            self.send_response(403)
            self.send_header("content-type", "text/plain; charset=utf-8")
            self.send_header("content-length", str(len(b)))
            self.end_headers(); self.wfile.write(b)
            log(ev="refused-403", path=path)
        except OSError as e:
            log(ev="refuse-failed", err=str(e))

    def refuse_push(self, cmds, data):
        """Refuse a push in Git's own terms, so that git prints Silo's words: 'remote:' lines on
        the side band, and each ref rejected with a reason in the report."""
        n = int(data[:4], 16)
        first = data[4:n]
        caps = first.split(b"\0", 1)[1].decode(errors="replace") if b"\0" in first else ""
        def pkt(b): return b"%04x" % (len(b) + 4) + b
        report = pkt(b"unpack ok\n") + b"".join(pkt(f"ng {c['ref']} held by Silo, not answered\n".encode()) for c in cmds) + b"0000"
        words = [l + "\n" for l in BODY.format(path="git push", host=self.headers.get("host")).splitlines()]
        if "side-band" in caps:
            out = b"".join(pkt(b"\x02" + w.encode()) for w in words) + pkt(b"\x01" + report) + b"0000"
        else:
            out = report
        self.send_response(200)
        self.send_header("content-type", "application/x-git-receive-pack-result")
        self.send_header("content-length", str(len(out)))
        self.end_headers(); self.wfile.write(out)
        log(ev="refused-in-git-terms", caps=caps)

    def hold_push_early(self, cmds, data):
        """Say at once, in Git's own terms, that the push is held, then wait. The status 200 and
        the first 'remote:' line go out before the wait; a short 'remote:' line follows every
        KEEPALIVE seconds; at the end each ref is rejected with a reason (or, with ALLOW_AFTER set,
        passed to git http-backend as a yes would)."""
        def pkt(b): return b"%04x" % (len(b) + 4) + b
        self.send_response(200)
        self.send_header("content-type", "application/x-git-receive-pack-result")
        self.send_header("transfer-encoding", "chunked")
        self.end_headers()
        def chunk(b):
            self.wfile.write(b"%x\r\n" % len(b) + b + b"\r\n"); self.wfile.flush()
        refs = ", ".join(c["ref"].replace("refs/heads/", "") for c in cmds)
        chunk(pkt(f"\x02Silo holds this push of {refs} for polarzero's answer (ask ask_7f3k2). It waits up to {int(HOLD)} s. Other work can go on.\n".encode()))
        log(ev="early-words-sent")
        start = time.time(); ka = float(os.environ.get("KEEPALIVE", "30"))
        last = start
        while time.time() - start < HOLD:
            r, _, _ = select.select([self.connection], [], [], 0.5)
            if r and not self.connection.recv(1, socket.MSG_PEEK):
                log(ev="client-closed", what="push", after=round(time.time() - start, 2)); return
            if time.time() - last >= ka:
                chunk(pkt(f"\x02still held, {int(time.time() - start)} s\n".encode())); last = time.time()
        report = pkt(b"unpack ok\n") + b"".join(pkt(f"ng {c['ref']} held by Silo, not answered\n".encode()) for c in cmds) + b"0000"
        chunk(pkt(b"\x02Nobody answered. Nothing was sent. The ask stays open: run the same push again after polarzero answers.\n") + pkt(b"\x01" + report) + b"0000")
        chunk(b"")
        log(ev="hold-ended-early-mode", after=round(time.time() - start, 2))

    def git(self, method, data):
        path, _, query = self.path.partition("?")
        env = dict(os.environ, GIT_PROJECT_ROOT=GIT_ROOT, GIT_HTTP_EXPORT_ALL="1", PATH_INFO=path,
                   QUERY_STRING=query, REQUEST_METHOD=method, CONTENT_TYPE=self.headers.get("content-type", ""),
                   CONTENT_LENGTH=str(len(data)), REMOTE_USER="agent", REMOTE_ADDR="127.0.0.1")
        p = subprocess.run(["git", "http-backend"], input=data, env=env, capture_output=True)
        head, _, rest = p.stdout.partition(b"\r\n\r\n")
        status = 200; hdrs = []
        for line in head.decode().split("\r\n"):
            k, _, v = line.partition(":")
            if k.lower() == "status":
                status = int(v.strip().split()[0])
            elif k:
                hdrs.append((k, v.strip()))
        self.send_response(status)
        for k, v in hdrs:
            self.send_header(k, v)
        self.send_header("content-length", str(len(rest)))
        self.end_headers(); self.wfile.write(rest)

    def dump(self, method, data):
        """With DUMP_DIR set, keep each held request whole, for retry_matcher.py."""
        d = os.environ.get("DUMP_DIR")
        if not d:
            return
        import base64
        n = len(os.listdir(d))
        json.dump({"method": method, "host": self.headers.get("host"), "path": self.path,
                   "headers": dict(self.headers.items()), "body": base64.b64encode(data).decode()},
                  open(f"{d}/{n:03d}.json", "w"))

    def handle_any(self, method):
        data = self.body() if method != "GET" else b""
        if method != "GET":
            self.dump(method, data)
        path = self.path.split("?")[0]
        if ".git/" in path:
            if method == "POST" and path.endswith("git-receive-pack"):
                cmds, off = pkt_commands(data)
                log(ev="push", commands=cmds, pack_sha=hashlib.sha256(data[off:]).hexdigest()[:16],
                    body_sha=hashlib.sha256(data).hexdigest()[:16], size=len(data))
                if MODE == "gitearly":
                    return self.hold_push_early(cmds, data)
                if not self.wait("push"):
                    return
                if MODE == "gitwords":
                    return self.refuse_push(cmds, data)
                if MODE == "drop":
                    log(ev="dropped"); self.close_connection = True
                    self.connection.shutdown(socket.SHUT_RDWR); return
                if MODE != "allow":
                    return self.refuse(path)
            return self.git(method, data)
        if method == "GET":
            b = b'{"ok":true}\n'
            self.send_response(200); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
            return
        log(ev="api", method=method, path=self.path, body_sha=hashlib.sha256(data).hexdigest()[:16],
            idem=self.headers.get("idempotency-key"))
        if not self.wait(f"{method} {path}"):
            return
        if MODE == "allow":
            b = b'{"id":"ch_fake","status":"succeeded"}\n'
            self.send_response(200); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
            return
        self.refuse(path)

    def do_GET(self): self.handle_any("GET")
    def do_POST(self): self.handle_any("POST")
    def do_PUT(self): self.handle_any("PUT")
    def do_PATCH(self): self.handle_any("PATCH")
    def do_DELETE(self): self.handle_any("DELETE")


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8700"))
    log(ev="listening", port=port, hold=HOLD, mode=MODE)
    S(("127.0.0.1", port), H).serve_forever()
