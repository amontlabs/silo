"""The hosts on the internet, played on one TLS port for every name: the gate in front of #374's
stand-in providers, Google Cloud Storage, Azure Resource Manager and a plain host for passing
bytes through. Each host accepts only the real value of its secret, and writes what credential each
request carried to hosts.log: "real:<secret>", "placeholder:<secret>", "none" or "other". That log
is how a test knows that no placeholder reached a host and that every swap happened.

  PORT, TLS_CERT, TLS_KEY   where to listen and the certificate for every host's name
  CONFIG                    the run's config.json, for the secrets
  PROVIDER                  host:port of #374's fake/provider.py, which bills the model calls
  UP_CA                     the authority of the provider's certificate
  OUT                       folder for hosts.log
Also a TCP banner on SSH_PORT standing for github.com:22.
"""
import http.client, http.server, json, os, socket, socketserver, ssl, threading, time

CONF = json.load(open(os.environ["CONFIG"]))["boundary"]
REAL, PH = {}, {}
for cp in CONF["computers"]:
    for s in cp.get("secrets", []):
        REAL[s["value"]] = s["name"]; PH[s["placeholder"]] = s["name"]
LOG = open(os.path.join(os.environ.get("OUT", "."), "hosts.log"), "a"); LOCK = threading.Lock()
PHOST, PPORT = os.environ["PROVIDER"].split(":")
UPCTX = ssl.create_default_context(cafile=os.environ["UP_CA"])
MODEL_HOSTS = {"api.anthropic.com", "api.openai.com", "chatgpt.com", "generativelanguage.googleapis.com"}

def classify(v):
    if not v: return "none"
    if v in REAL: return "real:" + REAL[v]
    if v in PH: return "placeholder:" + PH[v]
    return "other"

class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def cred(self):
        a = self.headers.get("x-api-key") or self.headers.get("x-goog-api-key") or self.headers.get("authorization", "")
        if a.lower().startswith("bearer "): a = a[7:]
        if not a and "key=" in self.path: a = self.path.split("key=", 1)[1].split("&")[0]
        return a

    def note(self, host, c):
        with LOCK:
            LOG.write(json.dumps({"t": time.time(), "host": host, "method": self.command, "path": self.path.split("?")[0], "cred": c}) + "\n"); LOG.flush()

    def send(self, code, body, ctype="application/json"):
        b = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code); self.send_header("content-type", ctype); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def handle_any(self):
        host = (self.headers.get("host") or "").split(":")[0]
        n = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(n) if n else b""
        c = classify(self.cred()); self.note(host, c)
        if host == "github.com":  # a host Silo passes through unread: no credential needed
            if self.path.startswith("/big"):
                size = int(self.path.split("bytes=")[1]) if "bytes=" in self.path else 1 << 20
                self.send_response(200); self.send_header("content-length", str(size)); self.end_headers()
                chunk = b"x" * 65536; left = size
                while left > 0:
                    self.wfile.write(chunk[:min(left, 65536)]); left -= 65536
                return
            return self.send(200, {"ok": True, "host": host})
        if not c.startswith("real:"):
            return self.send(401, {"error": {"type": "authentication_error", "message": f"invalid credential ({c})"}})
        if self.path.startswith("/big"):
            size = int(self.path.split("bytes=")[1]) if "bytes=" in self.path else 1 << 20
            self.send_response(200); self.send_header("content-length", str(size)); self.end_headers()
            chunk = b"x" * 65536; left = size
            while left > 0:
                self.wfile.write(chunk[:min(left, 65536)]); left -= 65536
            return
        if host in MODEL_HOSTS:
            return self.forward(host, body)
        if host == "storage.googleapis.com":
            return self.send(200, {"kind": "storage#buckets", "items": [{"name": "silo-test-bucket"}]})
        if host == "management.azure.com":
            return self.send(200, {"value": [{"subscriptionId": "00000000-0000-0000-0000-000000000001", "displayName": "silo test"}]})
        if host == "oauth2.googleapis.com":
            return self.send(200, {"access_token": "should-not-be-asked", "expires_in": 3599})
        if self.path.startswith("/echo"):
            return self.send(200, body or b"{}")
        return self.send(200, {"ok": True, "host": host})

    def forward(self, host, body):
        c = http.client.HTTPSConnection(PHOST, int(PPORT), context=UPCTX, timeout=300)
        c._tunnel_host = None
        hdrs = {k: v for k, v in self.headers.items() if k.lower() not in ("connection", "content-length", "transfer-encoding")}
        hdrs["content-length"] = str(len(body))
        c.sock = UPCTX.wrap_socket(socket.create_connection((PHOST, int(PPORT))), server_hostname=host)
        c.request(self.command, self.path, body, hdrs)
        r = c.getresponse()
        self.send_response(r.status)
        for k, v in r.getheaders():
            if k.lower() not in ("transfer-encoding", "content-length", "connection"): self.send_header(k, v)
        self.send_header("transfer-encoding", "chunked"); self.end_headers()
        while True:
            d = r.read1(65536)
            if not d: break
            self.wfile.write(b"%x\r\n%s\r\n" % (len(d), d)); self.wfile.flush()
        self.wfile.write(b"0\r\n\r\n"); self.wfile.flush(); c.close()

    do_GET = do_POST = do_PUT = do_DELETE = handle_any

class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True; allow_reuse_address = True; request_queue_size = 512
    def get_request(self):
        # Without this, a head and a body written apart wait 40 ms on a kept-alive connection
        # (Nagle's algorithm against a delayed acknowledgement), which would hide what is measured.
        s, a = self.socket.accept(); s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1); return s, a

def ssh_banner(port):
    l = socket.socket(); l.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); l.bind(("127.0.0.1", port)); l.listen(64)
    while True:
        s, _ = l.accept()
        def h(s=s):
            try:
                s.sendall(b"SSH-2.0-standin\r\n"); d = s.recv(100); s.sendall(b"echo:" + d)
            except OSError: pass
            s.close()
        threading.Thread(target=h, daemon=True).start()

if os.environ.get("SSH_PORT"):
    threading.Thread(target=ssh_banner, args=(int(os.environ["SSH_PORT"]),), daemon=True).start()
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); ctx.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"]); ctx.set_alpn_protocols(["http/1.1"])
srv = S(("127.0.0.1", int(os.environ.get("PORT", "9443"))), H)
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
srv.serve_forever()
