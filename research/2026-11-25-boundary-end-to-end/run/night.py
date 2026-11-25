"""A heavy night from inside one computer: agents, each with subagents, each a loop of model turns
and the tool calls between them. Run inside c1 or c2 with run/in.sh. Writes one JSON line per
operation to stdout.

usage: night.py <c1|c2> <loops> <seconds>
Environment: CA (Silo's authority and the stand-ins'), ANTH, OPENAI, GCP (this computer's
placeholders), THINK_MS (lo,hi: the pause after each tool call), DESTRUCTIVE (the share of tool
calls that are destructive platform writes), HOSTILE (the share that are #401's requests), SEED.

Each loop is one agent or subagent. A turn is a streamed model call (Anthropic for two loops in
three, OpenAI Responses for the rest), with the conversation growing each turn, then one to three
tool calls drawn from: Cloud Storage with the placeholder (a host Silo reads, a secret swapped),
a read of a platform's API (seen by the precaution), a write to a platform's API that is not
destructive, a destructive one (held for the person), a fetch from github.com (passed through
unread), and #401's requests that try to slip past the count.
"""
import http.client, json, os, random, socket, ssl, sys, threading, time

C, LOOPS, SECS = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])
CTX = ssl.create_default_context(cafile=os.environ["CA"])
ANTH, OPENAI, GCP = os.environ["ANTH"], os.environ["OPENAI"], os.environ["GCP"]
GH, VERCEL = f"ghp_siloPlaceholder{C}0000", f"vercel_silo_placeholder_{C}"
THINK = [int(x) for x in os.environ.get("THINK_MS", "200,1500").split(",")]
P_DESTR = float(os.environ.get("DESTRUCTIVE", "0.01"))
P_HOSTILE = float(os.environ.get("HOSTILE", "0.02"))
RNG = random.Random(int(os.environ.get("SEED", "1")) * 1000 + (1 if C == "c1" else 2))
LOCK = threading.Lock()
T_END = time.time() + SECS


def out(**kw):
    with LOCK:
        sys.stdout.write(json.dumps({"c": C, **kw}) + "\n")
        sys.stdout.flush()


def raw_sock(host, port=443):
    if C == "c1":
        s = socket.create_connection(("127.0.0.1", 3128), timeout=200)
        s.sendall(f"CONNECT {host}:{port} HTTP/1.1\r\nhost: {host}:{port}\r\n\r\n".encode())
        b = b""
        while b"\r\n\r\n" not in b:
            x = s.recv(1)
            if not x:
                raise ConnectionError("relay closed")
            b += x
        if b" 200 " not in b.split(b"\r\n")[0]:
            raise ConnectionRefusedError(b.split(b"\r\n")[0].decode(errors="replace"))
        return s
    return socket.create_connection((host, port), timeout=200)


class Conn(http.client.HTTPSConnection):
    def connect(self):
        self.sock = CTX.wrap_socket(raw_sock(self.host), server_hostname=self.host)


class Loop:
    def __init__(self, i):
        self.i, self.turn, self.convo = i, 0, []
        self.rng = random.Random(RNG.random())
        self.keep = {}  # kept-alive connections by host

    def conn(self, host):
        c = self.keep.get(host)
        if c is None:
            c = self.keep[host] = Conn(host, timeout=200)
        return c

    def req(self, kind, host, method, path, body=None, headers=None, keep=True):
        t0 = time.time()
        c = self.conn(host) if keep else Conn(host, timeout=200)
        try:
            c.request(method, path, body, headers or {})
            r = c.getresponse()
            data = r.read()
            st = r.status
        except Exception as e:  # noqa: BLE001 - every failure is a result here
            self.keep.pop(host, None)
            c.close()
            out(k=kind, loop=self.i, t=t0, ms=round((time.time() - t0) * 1000, 2), status=0, err=type(e).__name__ + ":" + str(e)[:80])
            return None
        if not keep or r.getheader("connection", "").lower() == "close":
            self.keep.pop(host, None)
            c.close()
        out(k=kind, loop=self.i, t=t0, ms=round((time.time() - t0) * 1000, 2), status=st,
            **({"why": data[:160].decode(errors="replace")} if st >= 400 else {}))
        return st

    def model(self):
        self.turn += 1
        self.convo.append({"role": "user", "content": f"step {self.turn}: " + "x" * self.rng.randint(200, 2000)})
        anth = self.i % 3 != 2
        host = "api.anthropic.com" if anth else "api.openai.com"
        if anth:
            body = {"model": "claude-opus-5-5", "max_tokens": 2000, "stream": True, "messages": self.convo[-40:]}
            hdr = {"x-api-key": ANTH, "anthropic-version": "2023-06-01", "content-type": "application/json"}
            path = "/v1/messages"
        else:
            body = {"model": "gpt-5.5", "stream": True, "input": [{"role": m["role"], "content": m["content"]} for m in self.convo[-40:]]}
            hdr = {"authorization": "Bearer " + OPENAI, "content-type": "application/json"}
            path = "/v1/responses"
        t0 = time.time()
        c = Conn(host, timeout=200)
        try:
            c.request("POST", path, json.dumps(body), hdr)
            r = c.getresponse()
            first = r.read(1)
            t1 = time.time()
            rest = r.read()
            st = r.status
        except Exception as e:  # noqa: BLE001
            c.close()
            out(k="model", loop=self.i, t=t0, ms=round((time.time() - t0) * 1000, 2), status=0, err=type(e).__name__ + ":" + str(e)[:80])
            return
        c.close()
        out(k="model", loop=self.i, t=t0, ms=round((time.time() - t0) * 1000, 2), ttfb=round((t1 - t0) * 1000, 2), status=st,
            bytes=len(first) + len(rest), **({"why": (first + rest)[:160].decode(errors="replace")} if st >= 400 else {}))
        self.convo.append({"role": "assistant", "content": "done " + str(self.turn)})

    def hostile(self):
        """#401's requests: a path spelt another way, a header folded or ended by a bare LF. Each
        must be refused; none may reach the provider uncounted."""
        n = self.rng.randrange(4)
        msg = json.dumps({"model": "claude-opus-5-5", "max_tokens": 100, "messages": [{"role": "user", "content": "hi"}]})
        if n < 2:
            path = ["//v1/messages", "/v1/./messages"][n]
            return self.req("hostile-path", "api.anthropic.com", "POST", path, msg, {"x-api-key": ANTH, "content-type": "application/json"}, keep=False)
        t0 = time.time()
        try:
            s = CTX.wrap_socket(raw_sock("api.anthropic.com"), server_hostname="api.anthropic.com")
            if n == 2:  # Transfer-Encoding ended by a bare LF: the boundary sees a length, the host a chunked body
                head = f"POST /v1/messages HTTP/1.1\r\nhost: api.anthropic.com\r\nx-api-key: {ANTH}\r\ncontent-length: 4\r\ntransfer-encoding: chunked\n\r\n"
            else:  # Content-Length folded onto two lines
                head = f"POST /v1/messages HTTP/1.1\r\nhost: api.anthropic.com\r\nx-api-key: {ANTH}\r\ncontent-length:\r\n {len(msg)}\r\n\r\n"
            s.sendall(head.encode() + (f"{len(msg):x}\r\n{msg}\r\n0\r\n\r\n" if n == 2 else msg).encode())
            s.settimeout(10)
            data = s.recv(200)
            s.close()
            st = int(data.split(b" ")[1]) if data.startswith(b"HTTP/") else 0
        except Exception as e:  # noqa: BLE001
            st, data = 0, str(e).encode()
        out(k="hostile-" + ("lf" if n == 2 else "fold"), loop=self.i, t=t0, ms=round((time.time() - t0) * 1000, 2), status=st, why=data[:120].decode(errors="replace"))

    def tool(self):
        x = self.rng.random()
        if x < P_DESTR:
            return self.req("destructive", "api.github.com", "DELETE", f"/repos/polar/scratch-{self.i}-{self.turn}", headers={"authorization": "Bearer " + GH}, keep=False)
        x -= P_DESTR
        if x < P_HOSTILE:
            return self.hostile()
        x = self.rng.random()
        if x < 0.45:
            return self.req("cloud", "storage.googleapis.com", "GET", "/storage/v1/b?project=silo-test", headers={"authorization": "Bearer " + GCP})
        if x < 0.65:
            return self.req("platform-read", "api.github.com", "GET", f"/repos/polar/silo/pulls/{self.turn}", headers={"authorization": "Bearer " + GH})
        if x < 0.80:
            return self.req("platform-write", "api.vercel.com", "POST", "/v9/projects/silo/env", json.dumps({"key": f"K{self.turn}", "value": "v", "type": "plain", "target": ["preview"]}), {"content-type": "application/json", "authorization": "Bearer " + VERCEL})
        return self.req("pass", "github.com", "GET", "/polar/silo/info/refs?service=git-upload-pack")

    def run(self):
        time.sleep(self.rng.random() * 3)
        while time.time() < T_END:
            self.model()
            for _ in range(self.rng.randint(1, 3)):
                if time.time() >= T_END:
                    break
                self.tool()
                time.sleep(self.rng.randint(*THINK) / 1000)


threads = [threading.Thread(target=Loop(i).run, daemon=True) for i in range(LOOPS)]
for t in threads:
    t.start()
for t in threads:
    t.join(SECS + 300)
