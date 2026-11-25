"""Latency and throughput through the boundary, from inside a computer, or directly to the
stand-in hosts for the baseline. Prints one JSON line per measure.

usage: bench.py <how> [n]      ANTH and GCP: the Anthropic and Google keys or placeholders to send
  how: c1 (CONNECT to the relay at 127.0.0.1:3128), c2 (connect to the name; gvproxy hands it
  over), direct (no boundary: straight to the stand-in at 127.0.0.1:9443, with the real key)
"""
import http.client, json, os, socket, ssl, statistics, sys, threading, time

HOW = sys.argv[1]; N = int(sys.argv[2]) if len(sys.argv) > 2 else 200
ANTH, GCP = os.environ["ANTH"], os.environ["GCP"]
CTX = ssl.create_default_context(cafile=os.environ.get("CA") or os.environ.get("SSL_CERT_FILE"))
MSG = json.dumps({"model": "claude-opus-5-5", "max_tokens": 50, "messages": [{"role": "user", "content": "hi"}]}).encode()

def sock(host, port=443):
    if HOW == "c1":
        s = socket.create_connection(("127.0.0.1", 3128))
        s.sendall(f"CONNECT {host}:{port} HTTP/1.1\r\nhost: {host}:{port}\r\n\r\n".encode())
        b = b""
        while b"\r\n\r\n" not in b: b += s.recv(1)
        assert b" 200 " in b.split(b"\r\n")[0], b
        return s
    if HOW == "c2": return socket.create_connection((host, port))
    return socket.create_connection(("127.0.0.1", 9443))

class Conn(http.client.HTTPSConnection):
    def connect(self):
        self.sock = CTX.wrap_socket(sock(self.host), server_hostname=self.host)

def pct(xs, p): return sorted(xs)[min(len(xs) - 1, int(len(xs) * p / 100))]
def ms(xs): return {"p50": round(pct(xs, 50) * 1000, 2), "p90": round(pct(xs, 90) * 1000, 2), "p99": round(pct(xs, 99) * 1000, 2), "n": len(xs)}

def call(c):
    """A request the stand-in answers at once: what is measured is the way there and back."""
    c.request("GET", "/storage/v1/b?project=silo-test", headers={"authorization": "Bearer " + GCP})
    r = c.getresponse(); r.read(); assert r.status == 200, r.status

def model(c):
    """A model call: #374's counting and admission, and the stand-in model's own time."""
    c.request("POST", "/v1/messages", MSG, {"x-api-key": ANTH, "content-type": "application/json"})
    r = c.getresponse(); r.read(); assert r.status == 200, r.status

def out(name, **kw): print(json.dumps({"how": HOW, "measure": name, **kw}), flush=True)

H = "storage.googleapis.com"
# Connection set-up: TCP (and CONNECT), then TLS.
tcp, tls = [], []
for i in range(N):
    t0 = time.perf_counter(); s = sock(H); t1 = time.perf_counter()
    x = CTX.wrap_socket(s, server_hostname=H); t2 = time.perf_counter(); x.close()
    tcp.append(t1 - t0); tls.append(t2 - t1)
out("connect", **ms(tcp)); out("tls", **ms(tls))
# A request with a placeholder swapped, on a new connection each time, and on one kept alive.
fresh = []
for i in range(N):
    t0 = time.perf_counter(); c = Conn(H); call(c); c.close(); fresh.append(time.perf_counter() - t0)
out("request, new connection", **ms(fresh))
c = Conn(H); call(c); kept = []
for i in range(N):
    t0 = time.perf_counter(); call(c); kept.append(time.perf_counter() - t0)
out("request, kept-alive", **ms(kept)); c.close()
c = Conn("api.anthropic.com"); model(c); mc = []
for i in range(min(N, 100)):
    t0 = time.perf_counter(); model(c); mc.append(time.perf_counter() - t0)
out("model call, kept-alive", **ms(mc)); c.close()
# Time to the first byte of a streamed reply.
first = []
STREAM = json.dumps({"model": "claude-opus-5-5", "max_tokens": 50, "stream": True, "messages": [{"role": "user", "content": "hi"}]}).encode()
for i in range(min(N, 100)):
    c = Conn("api.anthropic.com"); t0 = time.perf_counter()
    c.request("POST", "/v1/messages", STREAM, {"x-api-key": ANTH, "content-type": "application/json"})
    r = c.getresponse(); r.read1(1); first.append(time.perf_counter() - t0); r.read(); c.close()
out("first byte of a stream, new connection", **ms(first))
# Many at once: 32 programs, each making calls on new connections.
lat = []; lock = threading.Lock(); errs = [0]
def worker():
    for i in range(max(1, N // 10)):
        t0 = time.perf_counter()
        try:
            c = Conn(H); call(c); c.close()
            with lock: lat.append(time.perf_counter() - t0)
        except Exception:
            with lock: errs[0] += 1
t0 = time.perf_counter(); ts = [threading.Thread(target=worker) for _ in range(32)]
[t.start() for t in ts]; [t.join() for t in ts]; wall = time.perf_counter() - t0
out("32 at once, new connections", calls_per_s=round(len(lat) / wall, 1), errors=errs[0], **ms(lat))
# Bytes: a host passed through unread, and a host Silo reads.
for host, label in (("github.com", "pass"), ("storage.googleapis.com", "read")):
    size = 200 << 20
    c = Conn(host); t0 = time.perf_counter()
    c.request("GET", f"/big?bytes={size}", headers={"authorization": "Bearer " + GCP, "host": host})
    r = c.getresponse(); got = 0
    while True:
        d = r.read(1 << 20)
        if not d: break
        got += len(d)
    dt = time.perf_counter() - t0; c.close()
    out(f"download, host {label}", mbit_s=round(got * 8 / dt / 1e6, 0), bytes=got)
