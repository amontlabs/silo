"""Every case of the boundary end to end, on both ways in. Prints one line per case:
"<case> <c1 result> | <c2 result>" or "<case> <result>", then PASS or FAIL against what Silo needs.

usage: cases.py <out>    (a run started by up.sh, with #374's budgets off for c1: no limit)
Runs commands in the computers with in.sh and checks the stand-in hosts' log (what reached each
host, by credential) and the boundary's trace.
"""
import json, os, socket, ssl, struct, subprocess, sys, time

R = os.path.dirname(os.path.abspath(__file__)); OUT = os.path.abspath(sys.argv[1])
ENV = dict(l.strip().split("=", 1) for l in open(f"{OUT}/env") if "=" in l)
TLS = ENV["TLS"]
results = []

def run(c, *cmd, timeout=30, input=None):
    t = time.time()
    p = subprocess.run([f"{R}/in.sh", OUT, c, *cmd], capture_output=True, text=True, timeout=timeout, input=input)
    return p.returncode, p.stdout.strip(), p.stderr.strip(), time.time() - t

def curl(c, url, *args, timeout=20):
    rc, out, err, dt = run(c, "curl", "-sS", "-m", str(timeout), "-o", "/dev/stderr", "-w", "%{http_code}", url, *args, timeout=timeout + 5)
    return out or "000", err, rc, dt

def hosts_log():
    return [json.loads(l) for l in open(f"{OUT}/hosts.log")] if os.path.exists(f"{OUT}/hosts.log") else []

def mark():
    return len(hosts_log())

def seen_since(m):
    return [h["cred"] + "@" + h["host"] for h in hosts_log()[m:]]

def case(name, ok, detail):
    results.append((name, ok)); print(f"{'PASS' if ok else 'FAIL'}  {name}: {detail}", flush=True)

PH = lambda c, s: {"anthropic": f"sk-ant-api03-silo-placeholder-{c}-0000", "openai": f"sk-proj-silo-placeholder-{c}-0000", "gemini": f"AIzaSiloPlaceholder{c}000", "gcp": f"ya29.silo-placeholder-{c}"}[s]
MSG = json.dumps({"model": "claude-opus-5-5", "max_tokens": 100, "messages": [{"role": "user", "content": "hi"}]})
CHAT = json.dumps({"model": "gpt-6.2-sol", "stream": True, "stream_options": {"include_usage": True}, "messages": [{"role": "user", "content": "hi"}]})
GEM = json.dumps({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]})

for c in ("c1", "c2"):
    m = mark()
    code, body, rc, dt = curl(c, "https://api.anthropic.com/v1/messages", "-H", f"x-api-key: {PH(c, 'anthropic')}", "-H", "content-type: application/json", "-d", MSG)
    case(f"{c} anthropic call with its placeholder", code == "200" and seen_since(m) == ["real:anthropic@api.anthropic.com"], f"{code}, host saw {seen_since(m)}")
    m = mark()
    code, body, rc, dt = curl(c, "https://api.openai.com/v1/chat/completions", "-H", f"authorization: Bearer {PH(c, 'openai')}", "-H", "content-type: application/json", "-d", CHAT)
    case(f"{c} openai stream with its placeholder", code == "200" and seen_since(m) == ["real:openai@api.openai.com"] and "usage" in body, f"{code}, host saw {seen_since(m)}")
    m = mark()
    code, body, rc, dt = curl(c, f"https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-pro-preview:generateContent?key={PH(c, 'gemini')}", "-H", "content-type: application/json", "-d", GEM)
    case(f"{c} gemini key in the query", code == "200" and seen_since(m) == ["real:gemini@generativelanguage.googleapis.com"], f"{code}, host saw {seen_since(m)}")
    # The cloud metadata server, then the token's use.
    code, body, rc, dt = curl(c, "http://169.254.169.254/computeMetadata/v1/instance/service-accounts/default/token", "-H", "Metadata-Flavor: Google", timeout=5)
    tok = json.loads(body).get("access_token", "") if code == "200" else ""
    case(f"{c} google metadata token is a placeholder", tok == PH(c, "gcp"), f"{code} {tok or body[:80]!r}")
    if tok:
        m = mark()
        code, body, rc, dt = curl(c, "https://storage.googleapis.com/storage/v1/b?project=silo-test", "-H", f"authorization: Bearer {tok}")
        case(f"{c} google storage with the metadata placeholder", code == "200" and seen_since(m) == ["real:gcp@storage.googleapis.com"], f"{code}, host saw {seen_since(m)}")
    code, body, rc, dt = curl(c, "http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/", "-H", "Metadata: true", timeout=5)
    tok = json.loads(body).get("access_token", "") if code == "200" else ""
    case(f"{c} azure metadata token is a JWT placeholder", tok.count(".") == 2 and tok.endswith("silo-placeholder-signature"), f"{code} {tok[-40:] or body[:80]!r}")
    if tok:
        m = mark()
        code, body, rc, dt = curl(c, "https://management.azure.com/subscriptions?api-version=2022-12-01", "-H", f"authorization: Bearer {tok}")
        case(f"{c} azure with the metadata placeholder", code == "200" and seen_since(m) == ["real:azure@management.azure.com"], f"{code}, host saw {seen_since(m)}")
    # Refusals.
    m = mark()
    code, body, rc, dt = curl(c, "https://storage.googleapis.com/storage/v1/b", "-H", f"x-api-key: {PH(c, 'anthropic')}")
    case(f"{c} a placeholder to another allowed host", code in ("400", "403") and seen_since(m) == [] and "may not go to" in body, f"{code}, host saw {seen_since(m)}: {body[:110]!r}")
    m = mark()
    code, body, rc, dt = curl(c, "https://api.anthropic.com/v1/messages", "-H", "x-api-key: sk-ant-api03-REAL-anthropic-key-9f8e7d6c", "-H", "content-type: application/json", "-d", MSG)
    case(f"{c} a real key pasted in the computer", code == "400" and seen_since(m) == [], f"{code}, host saw {seen_since(m)}: {body[:100]!r}")
    other = "c2" if c == "c1" else "c1"
    m = mark()
    code, body, rc, dt = curl(c, "https://api.anthropic.com/v1/messages", "-H", f"x-api-key: {PH(other, 'anthropic')}", "-H", "content-type: application/json", "-d", MSG)
    case(f"{c} another computer's placeholder", code == "400" and seen_since(m) == [], f"{code}, host saw {seen_since(m)}")
    # The same on a host that is not a model's, where #374's check of credentials does not run.
    az_other = next(s["placeholder"] for k in json.load(open(f"{OUT}/config.json"))["boundary"]["computers"] if k["id"] == other for s in k["secrets"] if s["name"] == "azure")
    m = mark()
    code, body, rc, dt = curl(c, "https://management.azure.com/subscriptions?api-version=2022-12-01", "-H", f"authorization: Bearer {az_other}")
    case(f"{c} another computer's placeholder, not a model host", code in ("400", "403") and seen_since(m) == [], f"{code}, host saw {seen_since(m)}: {body[:100]!r}")
    code, body, rc, dt = curl(c, "https://evil.example/", timeout=5)
    case(f"{c} a host not allowed", code == "000" or code == "403", f"{code} rc={rc} in {dt:.2f}s: {body[:90]!r}")
    # A host passed through unread: the program sees the host's own certificate.
    rc, out, err, dt = run(c, "curl", "-sS", "-m", "10", "--cacert", f"{TLS}/up-ca.crt", "https://github.com/")
    case(f"{c} github.com passed through, host's own certificate", rc == 0 and '"host": "github.com"' in out, f"rc={rc} {out[:60]!r} {err[:80]!r}")
    # SSH to github.com:22.
    if c == "c1":
        script = "import socket;s=socket.create_connection(('127.0.0.1',3128));s.sendall(b'CONNECT github.com:22 HTTP/1.1\\r\\n\\r\\n');h=s.recv(200);s.sendall(b'SSH-2.0-client\\r\\n');import time;time.sleep(0.3);print(repr(h.split(b'\\r\\n')[0]),repr(s.recv(200)))"
    else:
        script = "import socket;s=socket.create_connection(('github.com',22),5);s.sendall(b'SSH-2.0-client\\r\\n');import time;time.sleep(0.3);print(repr(s.recv(200)))"
    rc, out, err, dt = run(c, "python3", "-c", script)
    case(f"{c} ssh to github.com:22", "SSH-2.0-standin" in out, f"{out[:120]} {err[-100:]}")
    # TLS that names another host than the one connected to.
    if c == "c1":
        script = ("import socket,ssl;s=socket.create_connection(('127.0.0.1',3128));s.sendall(b'CONNECT api.anthropic.com:443 HTTP/1.1\\r\\n\\r\\n');s.recv(200);"
                  "x=ssl.create_default_context().wrap_socket(s,server_hostname='storage.googleapis.com')")
    else:
        script = ("import socket,ssl;a=socket.gethostbyname('api.anthropic.com');s=socket.create_connection((a,443),5);"
                  "x=ssl.create_default_context().wrap_socket(s,server_hostname='storage.googleapis.com')")
    rc, out, err, dt = run(c, "python3", "-c", script)
    case(f"{c} TLS names another host than the connection", rc != 0, f"rc={rc} {err.splitlines()[-1][:110] if err else ''}")
    # HTTP/2 only (gRPC): the boundary offers HTTP/1.1 only.
    rc, out, err, dt = run(c, "curl", "-sS", "-m", "10", "--http2", "-o", "/dev/null", "-w", "%{http_version}", "https://storage.googleapis.com/storage/v1/b", "-H", f"authorization: Bearer {PH(c, 'gcp')}")
    case(f"{c} a client asking for HTTP/2 gets HTTP/1.1", out == "1.1", f"http_version={out} rc={rc}")

# The CONNECT way only.
code, body, rc, dt = curl("c1", "http://github.com/", timeout=5)
case("c1 plain http to an allowed host", code == "403", f"{code} {body[:90]!r}")
rc, out, err, dt = run("c1", "python3", "-c", "import socket;s=socket.create_connection(('127.0.0.1',3128));s.sendall(b'CONNECT 93.184.215.14:443 HTTP/1.1\\r\\n\\r\\n');print(s.recv(300).split(b'\\r\\n')[0])")
case("c1 CONNECT to an address, not a name", "403" in out, out)
rc, out, err, dt = run("c1", "python3", "-c", "import socket;s=socket.create_connection(('127.0.0.1',3128));s.sendall(b'\\x16\\x03\\x01garbage\\r\\n\\r\\n');print(s.recv(300).split(b'\\r\\n')[0])")
case("c1 garbage instead of CONNECT", "403" in out, out)
# The hand-off way only.
rc, out, err, dt = run("c2", "python3", "-c", "import socket\nfor n in ('x7f3a.exfil.example','api.anthropic.com','metadata.google.internal'):\n  try: print(n, socket.gethostbyname(n))\n  except Exception as e: print(n, 'no answer')")
case("c2 lookups: an unknown name gets no answer", "x7f3a.exfil.example no answer" in out and "api.anthropic.com 198.18." in out, out.replace("\n", "; "))
rc, out, err, dt = run("c2", "python3", "-c", "import socket,ssl;s=socket.create_connection(('203.0.113.10',443),5);x=ssl.create_default_context().wrap_socket(s,server_hostname='api.anthropic.com');x.sendall(b'GET /v1/models HTTP/1.1\\r\\nhost: api.anthropic.com\\r\\nx-api-key: " + PH("c2", "anthropic") + "\\r\\n\\r\\n');print(x.recv(200).split(b'\\r\\n')[0])")
case("c2 an address no lookup gave, with an allowed name in TLS", "200" in out, f"{out} {err[-90:]}")
rc, out, err, dt = run("c2", "python3", "-c", "import socket\ntry:\n s=socket.create_connection(('10.0.0.5',8080),5);s.sendall(b'GET / HTTP/1.0\\r\\n\\r\\n');print('got',s.recv(100))\nexcept Exception as e: print('no',e)")
case("c2 a private address on the local network", not out.startswith("got") or out == "got b''", out[:100])
# A broken hand-off header straight to the boundary's socket (a gvproxy that went wrong).
s = socket.socket(socket.AF_UNIX); s.connect(f"{OUT}/c2.handoff.sock"); s.sendall(b"PROXY TCP4 1.2.3.4 5.6.7.8 1 2\r\n" + b"x" * 40)
s.settimeout(3)
try: got = s.recv(100)
except Exception as e: got = repr(e)
case("broken PROXY header on the hand-off socket", got == b"" or "ConnectionReset" in str(got), f"closed with {got!r}")
# A slow head: nothing after connecting.
t = time.time(); rc, out, err, dt = run("c1", "python3", "-c", "import socket;s=socket.create_connection(('127.0.0.1',3128));s.settimeout(30);print(s.recv(300).split(b'\\r\\n')[0])", timeout=40)
case("c1 a connection that sends nothing", dt < 15, f"closed after {dt:.1f}s: {out}")

trace = [json.loads(l) for l in open(f"{OUT}/trace.jsonl")]
mints = [t for t in trace if t.get("kind") == "open" and t.get("mint_ms", 0) > 0]
print(f"certificates made: {len(mints)}, ms each: {sorted(round(t['mint_ms'], 2) for t in mints)}")
real_values = [s["value"] for cp in json.load(open(f"{OUT}/config.json"))["boundary"]["computers"] for s in cp["secrets"]]
leaks = []
for root, _, files in os.walk(f"{OUT}"):
    for f in files:
        if f in ("config.json",) or f.endswith(".sock"): continue
        p = os.path.join(root, f)
        try: b = open(p, "rb").read()
        except OSError: continue
        for v in real_values:
            if v.encode() in b: leaks.append((p.replace(OUT, "out"), v[:12]))
case("no real value in the record, the trace or any computer's files", not leaks, f"{leaks[:5]}")
print(f"{sum(ok for _, ok in results)}/{len(results)} cases pass")
