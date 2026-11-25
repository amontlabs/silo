"""What runs behind the previews in the tests, all on loopback.

  3100  computer c1's port 3000: a page that holds c1's secret and tries ways out (attack.html)
  3200  computer c2's port 3000: an ordinary app that logs every request it gets, with its cookies.
        What reaches it from c1's page has crossed from one computer to another.
  4000  the attacker's server on the internet; 4001/udp its STUN and TURN server

usage: python3 apps.py <hits.jsonl>"""
import http.server, json, os, socket, struct, sys, threading, urllib.parse

LOG = sys.argv[1]
HERE = os.path.dirname(os.path.abspath(__file__))
SECRET = "AKIA-C1-SECRET-777"


def log(d):
    with open(LOG, "a") as f:
        f.write(json.dumps(d) + "\n")


class Quiet(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def reply(self, code, body=b"", ctype="text/html", headers=()):
        self.send_response(code)
        self.send_header("content-type", ctype)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def body(self):
        n = int(self.headers.get("content-length") or 0)
        return self.rfile.read(n).decode(errors="replace") if n else ""


class C1(Quiet):
    sw = "sw-evil.js"

    def h(self):
        u = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(u.query)
        b = self.body()
        if u.path == "/sw.js":
            return self.reply(200, open(f"{HERE}/{C1.sw}", "rb").read(), "text/javascript")
        if u.path == "/ok":
            log({"to": "c1", "what": q.get("what", ["?"])[0], "detail": q.get("d", [""])[0], "body": b[:200],
                 "cookie": self.headers.get("cookie", "")})
            return self.reply(200, b"ok")
        if u.path == "/auth":
            return self.reply(401, b"", headers=[("www-authenticate", 'Basic realm="Silo needs your Mac password"')])
        if u.path == "/login":
            return self.reply(200, b"ok", headers=[("set-cookie", "session=signed-in; Path=/; HttpOnly")])
        if u.path == "/set-cookie":
            return self.reply(200, b"<p>cookies set by header", headers=[
                ("set-cookie", f"hdr_silo={SECRET}; Domain=silo.localhost; Path=/"),
                ("set-cookie", f"hdr_localhost={SECRET}; Domain=localhost; Path=/")])
        if u.path == "/" and os.environ.get("C1_PLAIN"):
            # A new computer that took the old one's name: an honest page, with no worker of its own.
            return self.reply(200, b"<!doctype html><title>new c1</title><p>the new computer's own page"
                              b"<script>fetch('/ok?what=new-computer-page')</script>")
        if u.path == "/":
            page = open(f"{HERE}/attack.html").read().replace("{{SECRET}}", SECRET)
            return self.reply(200, page.encode())
        self.reply(404)
    do_GET = do_POST = h


class C2(Quiet):
    """An app in another computer. What it receives, it logs."""
    def h(self):
        b = self.body()
        log({"to": "c2", "path": self.path[:200], "cookie": self.headers.get("cookie", ""),
             "secret": SECRET in urllib.parse.unquote(self.path) + b + (self.headers.get("cookie") or ""),
             "origin": self.headers.get("origin", "")})
        self.reply(200, b"<!doctype html><title>c2 app</title><p>c2", headers=[("access-control-allow-origin", "*")])
    do_GET = do_POST = h


class Attacker(Quiet):
    def h(self):
        q = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
        b = self.body()
        log({"to": "attacker", "via": q.get("via", ["?"])[0], "secret": SECRET in urllib.parse.unquote(self.path) + b,
             "host": self.headers.get("host", "")})
        self.reply(200, b"<p>attacker</p>", headers=[("access-control-allow-origin", "*")])
    do_GET = do_POST = h


def stun(port=4001):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("127.0.0.1", port))

    def attr(t, v):
        return struct.pack("!HH", t, len(v)) + v + b"\0" * (-len(v) % 4)
    while True:
        d, a = s.recvfrom(4096)
        if len(d) < 20:
            continue
        typ, ln, cookie = struct.unpack("!HHI", d[:8])
        tid = d[8:20]
        kind = {0x0001: "stun-binding", 0x0003: "turn-allocate"}.get(typ, hex(typ))
        log({"to": "attacker", "via": "webrtc-" + kind, "secret": SECRET.encode() in d or SECRET.replace("-", "").encode() in d})
        if typ == 0x0003 and b"\x00\x06" not in d[20:]:
            body = attr(0x0009, b"\0\0\x04\x01" + b"Unauthorized") + attr(0x0014, b"attacker") + attr(0x0015, b"nonce123")
            s.sendto(struct.pack("!HHI", 0x0113, len(body), cookie) + tid + body, a)


def serve(port, handler):
    s = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=s.serve_forever, daemon=True).start()


if os.environ.get("C1_SW"):
    C1.sw = os.environ["C1_SW"]
serve(int(os.environ.get("C1_PORT", 3100)), C1)
serve(3200, C2)
serve(4000, Attacker)
threading.Thread(target=stun, daemon=True).start()
print("ready", flush=True)
threading.Event().wait()
