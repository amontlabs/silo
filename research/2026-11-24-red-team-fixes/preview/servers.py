"""The preview test, all on loopback.

  3000  the app an agent serves inside a computer whose network is closed. Its page holds a secret
        and tries every way out it can (attack.html).
  4000  the attacker's server on the internet; 4001/udp its STUN server. Both log every hit.
  5000  a service on the Mac that trusts localhost origins (a dev server, a local model's API).
  7000  Silo's forwarder, which shows port 3000 on the Mac in one of four ways, by MODE:
        plain   http://localhost:7000/            the app as it is (today's port forwarding)
        csp     http://localhost:7000/            plus a strict Content-Security-Policy
        origin  http://3000.c1.silo.localhost:7000/  a name of its own, plus the policy
        shell   http://preview.silo.localhost:7000/c1/3000  a page of Silo's that holds the app's
                origin in a sandboxed frame; the frame cannot navigate the page, open a window,
                download, or navigate itself away from its origin.
        browser http://3000.c1.silo.localhost:7000/ in a browser Silo starts, whose every request
                goes through a proxy (7100) that applies the computer's own network rules, and
                whose WebRTC may use no UDP that bypasses that proxy.
usage: MODE=<mode> python3 servers.py <hits.jsonl>"""
import http.server, json, os, socket, sys, threading, urllib.parse, urllib.request
LOG = sys.argv[1]; MODE = os.environ.get("MODE", "plain")
SECRET = "AKIA-FAKE-SECRET-123"
HERE = os.path.dirname(os.path.abspath(__file__))
APP_ORIGIN = "http://3000.c1.silo.localhost:7000"
CSP_APP = ("default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; "
           "img-src 'self' data:; media-src 'self'; font-src 'self'; frame-src 'self'; child-src 'self'; worker-src 'self'; "
           "form-action 'self'; base-uri 'none'; object-src 'none'; manifest-src 'self'")
if os.environ.get("CSP_WEBRTC"): CSP_APP += "; webrtc 'block'"
CSP_SHELL = f"default-src 'none'; style-src 'unsafe-inline'; frame-src {APP_ORIGIN}; form-action 'none'; base-uri 'none'"
SHELL = """<!doctype html><meta charset=utf-8><title>c1 · port 3000 · Silo</title>
<style>body{margin:0;font:13px system-ui}header{padding:6px 10px;background:#222;color:#eee}iframe{border:0;width:100%%;height:90vh}</style>
<header>Port 3000 of computer c1. What is below comes from the computer.</header>
<iframe src="%s/{{Q}}" sandbox="allow-scripts allow-same-origin allow-forms allow-modals" allow="" referrerpolicy="no-referrer"></iframe>""" % APP_ORIGIN

def log(d):
    with open(LOG, "a") as f: f.write(json.dumps(d) + "\n")

def serve(port, handler, host="127.0.0.1"):
    s = http.server.ThreadingHTTPServer((host, port), handler); threading.Thread(target=s.serve_forever, daemon=True).start()

class Quiet(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass
    def reply(self, code, body=b"", ctype="text/html", headers=()):
        self.send_response(code); self.send_header("content-type", ctype)
        for k, v in headers: self.send_header(k, v)
        self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)

class Computer(Quiet):
    def do_GET(self):
        p = urllib.parse.urlparse(self.path).path
        if p == "/sw.js":
            return self.reply(200, open(f"{HERE}/sw.js", "rb").read(), "text/javascript")
        if p == "/ok" or p == "/ws" or p == "/pixel.png":
            log({"to": "computer", "what": urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query).get("what", [p])[0]})
            return self.reply(404 if p == "/pixel.png" else 200)
        if p == "/": return self.reply(200, open(f"{HERE}/attack.html").read().replace("{{SECRET}}", SECRET).encode())
        return self.reply(404)
    do_POST = do_GET

class Attacker(Quiet):
    def h(self):
        q = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
        n = int(self.headers.get("content-length") or 0); body = self.rfile.read(n).decode(errors="replace") if n else ""
        log({"to": "attacker", "via": q.get("via", ["?"])[0], "secret": SECRET in urllib.parse.unquote(self.path) + body,
             "mac": q.get("got", [None])[0], "cookie": q.get("c", [None])[0]})
        self.reply(200, b"<p>attacker</p>", headers=[("access-control-allow-origin", "*")])
    do_GET = do_POST = h

class MacService(Quiet):
    def do_GET(self):
        o = self.headers.get("origin", ""); log({"to": "mac-service", "origin": o})
        hs = [("access-control-allow-origin", o)] if ("localhost" in o or "127.0.0.1" in o) else []
        self.reply(200, b'{"models":["private-model-on-the-mac"]}', "application/json", hs)

class Forwarder(Quiet):
    def fwd(self):
        host = (self.headers.get("host") or "").split(":")[0]
        p = urllib.parse.urlparse(self.path).path
        if MODE == "shell" and host == "preview.silo.localhost":
            if p == "/c1/3000": return self.reply(200, SHELL.replace("{{Q}}", ("?" + urllib.parse.urlparse(self.path).query) if urllib.parse.urlparse(self.path).query else "").encode(), headers=[("content-security-policy", CSP_SHELL), ("x-frame-options", "DENY")])
            return self.reply(404)
        if MODE in ("origin", "shell", "browser") and host != "3000.c1.silo.localhost": return self.reply(421)
        if MODE in ("plain", "csp") and host not in ("localhost", "127.0.0.1"): return self.reply(421)
        n = int(self.headers.get("content-length") or 0); data = self.rfile.read(n) if n else None
        req = urllib.request.Request("http://127.0.0.1:3000" + self.path, data=data, method=self.command)
        with urllib.request.urlopen(req) as r:
            body = r.read(); hs = [(k, v) for k, v in r.getheaders() if k.lower() not in ("content-length", "connection", "transfer-encoding", "content-security-policy", "content-security-policy-report-only", "set-cookie")]
            if r.headers.get("set-cookie"): hs.append(("set-cookie", r.headers["set-cookie"]))
        if MODE != "plain":
            hs += [("content-security-policy", CSP_APP), ("permissions-policy", "camera=(), microphone=(), geolocation=(), usb=(), serial=(), hid=(), bluetooth=(), payment=(), clipboard-read=(), display-capture=()"),
                   ("cross-origin-opener-policy", "same-origin"), ("referrer-policy", "no-referrer")]
        if MODE == "shell":
            hs.append(("content-security-policy", f"frame-ancestors http://preview.silo.localhost:7000"))
        self.reply(200, body, r.headers.get("content-type", "text/html"), hs)
    do_GET = do_POST = fwd

def stun(port=4001):
    """The attacker's STUN/TURN server. A TURN Allocate without credentials gets 401 with a realm
    and a nonce, as RFC 8656 says, so the browser sends it again with the USERNAME it was given."""
    import struct
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind(("127.0.0.1", port))
    def attr(t, v): return struct.pack("!HH", t, len(v)) + v + b"\0" * (-len(v) % 4)
    while True:
        d, a = s.recvfrom(4096)
        if len(d) < 20: continue
        typ, ln, cookie = struct.unpack("!HHI", d[:8]); tid = d[8:20]
        kind = {0x0001: "stun-binding", 0x0003: "turn-allocate"}.get(typ, hex(typ))
        log({"to": "attacker", "via": "webrtc-" + kind, "secret": SECRET.encode() in d or SECRET.replace("-", "").encode() in d, "bytes": len(d)})
        if typ == 0x0003 and b"\x00\x06" not in d[20:]:
            body = attr(0x0009, b"\0\0\x04\x01" + b"Unauthorized") + attr(0x0014, b"attacker") + attr(0x0015, b"nonce123")
            s.sendto(struct.pack("!HHI", 0x0113, len(body), cookie) + tid + body, a)

ALLOWED = {"3000.c1.silo.localhost:7000", "preview.silo.localhost:7000"}

class RuleProxy(Quiet):
    """MODE=browser: a browser of Silo's own on the Mac, whose every request goes through this proxy,
    which applies the computer's network rules (here: closed, so only the preview itself passes).
    What the page tries is denied and recorded, as if the computer had tried it."""
    def deny(self, target):
        log({"to": "denied by the computer's rules", "target": target[:80], "secret": SECRET in target})
        self.reply(403, b"denied by the rules of computer c1")
    def do_CONNECT(self):
        if self.path not in ALLOWED: return self.deny(self.path)
        up = socket.create_connection(("127.0.0.1", 7000))
        self.send_response(200); self.end_headers(); self.wfile.flush()
        c = self.connection
        def pipe(a, b):
            try:
                while (d := a.recv(65536)): b.sendall(d)
            except OSError: pass
            finally:
                try: b.shutdown(socket.SHUT_WR)
                except OSError: pass
        t = threading.Thread(target=pipe, args=(up, c), daemon=True); t.start(); pipe(c, up); t.join(); self.close_connection = True
    def any(self):
        u = urllib.parse.urlparse(self.path)
        if f"{u.hostname}:{u.port}" not in ALLOWED: return self.deny(self.path)
        n = int(self.headers.get("content-length") or 0); data = self.rfile.read(n) if n else None
        hs = {k: v for k, v in self.headers.items() if k.lower() not in ("proxy-connection", "connection", "host")}
        req = urllib.request.Request("http://127.0.0.1:7000" + (u.path or "/") + ("?" + u.query if u.query else ""), data=data, method=self.command, headers=dict(hs, Host=f"{u.hostname}:{u.port}"))
        try:
            with urllib.request.urlopen(req) as r: body = r.read(); code = r.status; rh = r.getheaders()
        except urllib.error.HTTPError as e: body = e.read(); code = e.code; rh = e.headers.items()
        self.reply(code, body, dict(rh).get("content-type", "text/html"), [(k, v) for k, v in rh if k.lower() not in ("content-length", "connection", "content-type", "transfer-encoding")])
    do_GET = do_POST = do_HEAD = any

serve(3000, Computer); serve(4000, Attacker); serve(5000, MacService); serve(7000, Forwarder)
if os.environ.get("PROXY"): serve(7100, RuleProxy)
threading.Thread(target=stun, daemon=True).start()
threading.Event().wait()
