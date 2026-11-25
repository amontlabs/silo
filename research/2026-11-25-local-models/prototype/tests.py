"""Tests of the gate, from the computer's side. Starts the stand-ins and the gate, sends what agent
programs send (the four formats #369 found, streamed and not) and what a hostile agent would, and
checks the record against what reached the Mac.

usage: python3 tests.py <out dir>"""
import http.client, json, os, socket, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "out")
os.makedirs(OUT, exist_ok=True)
MAC, REC = os.path.join(OUT, "mac.jsonl"), os.path.join(OUT, "record.jsonl")
for f in (MAC, REC):
    if os.path.exists(f):
        os.remove(f)
GRANTS = {"21434": {"computer": "c1", "server": "ollama", "to": 11434, "models": None},
          "21234": {"computer": "c1", "server": "lmstudio", "to": 1234, "models": None},
          "22434": {"computer": "c2", "server": "ollama", "to": 11434, "models": ["qwen3-coder:30b"]}}
json.dump(GRANTS, open(os.path.join(OUT, "grants.json"), "w"))


def start(args, env=None):
    p = subprocess.Popen(args, stdout=subprocess.PIPE, env=dict(os.environ, **(env or {})))
    assert p.stdout.readline().strip() == b"ready"
    return p


import atexit
procs = []
atexit.register(lambda: [p.kill() for p in procs])
procs += [start(["python3", os.path.join(HERE, "mac_servers.py"), MAC]),
         start(["python3", os.path.join(HERE, "gate.py")], {"GRANTS": os.path.join(OUT, "grants.json"), "LOG": REC})]


def lines(f):
    return [json.loads(x) for x in open(f)] if os.path.exists(f) else []


def calls_recorded(n_before):
    """The gate writes a call's record once the Mac's server closes; wait for it."""
    for _ in range(100):
        r = [x for x in lines(REC) if x.get("format")]
        if len(r) > n_before:
            return r[-1]
        time.sleep(0.02)
    return {}


def call(port, method, path, body=None, headers=None):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    b = json.dumps(body).encode() if body is not None else None
    c.request(method, path, body=b, headers=dict({"content-type": "application/json"}, **(headers or {})))
    r = c.getresponse()
    data = r.read()
    return r.status, data


def raw(port, data):
    s = socket.create_connection(("127.0.0.1", port))
    s.sendall(data)
    s.settimeout(5)
    got = b""
    try:
        while d := s.recv(65536):
            got += d
    except socket.timeout:
        pass
    return got


results = []


def check(name, ok, detail=""):
    results.append({"test": name, "ok": bool(ok), "detail": detail})


MSG = [{"role": "user", "content": "What is the answer?"}]
CALLS = [  # (who sends it, per #369; port; path; body)
    ("Aider, Cline: Ollama's own lines", 21434, "/api/chat", {"model": "qwen3-coder:30b", "messages": MSG}),
    ("Ollama's own lines, not streamed", 21434, "/api/chat", {"model": "qwen3-coder:30b", "messages": MSG, "stream": False}),
    ("Ollama's generate", 21434, "/api/generate", {"model": "qwen3-coder:30b", "prompt": "hi"}),
    ("Goose, Copilot CLI: Chat Completions, usage asked", 21434, "/v1/chat/completions", {"model": "qwen3-coder:30b", "messages": MSG, "stream": True, "stream_options": {"include_usage": True}}),
    ("Chat Completions, usage not asked", 21434, "/v1/chat/completions", {"model": "qwen3-coder:30b", "messages": MSG, "stream": True}),
    ("Chat Completions, not streamed", 21434, "/v1/chat/completions", {"model": "qwen3-coder:30b", "messages": MSG}),
    ("Codex --oss: Responses", 21434, "/v1/responses", {"model": "qwen3-coder:30b", "input": "hi", "stream": True}),
    ("Claude Code at Ollama: Messages", 21434, "/v1/messages?beta=true", {"model": "qwen3-coder:30b", "messages": MSG, "max_tokens": 100, "stream": True}),
    ("Codex --oss at LM Studio: Responses", 21234, "/v1/responses", {"model": "qwen3-coder:30b", "input": "hi", "stream": True}),
    ("LM Studio: Chat Completions", 21234, "/v1/chat/completions", {"model": "qwen3-coder:30b", "messages": MSG, "stream": True, "stream_options": {"include_usage": True}}),
]
for name, port, path, body in CALLS:
    n = len([x for x in lines(REC) if x.get("format")])
    st, data = call(port, "POST", path, body)
    rec = calls_recorded(n)
    want = (26, 5) if "usage not asked" not in name else (None, None)
    check(f"counted: {name}", st == 200 and (rec["input_tokens"], rec["output_tokens"]) == want,
          f"status {st}, counted in {rec['input_tokens']} out {rec['output_tokens']}, first byte {rec['first_byte_ms']} ms")

# Listing models, which programs do first (Codex asks /api/tags, Goose and Aider /api/show).
for path, m in (("/api/tags", "GET"), ("/v1/models", "GET")):
    st, _ = call(21434, m, path)
    check(f"listing passes: {m} {path}", st == 200, f"status {st}")
st, _ = call(21434, "POST", "/api/show", {"model": "qwen3-coder:30b"})
check("listing passes: POST /api/show", st == 200, f"status {st}")

# Routes that change the Mac: never reach it.
before = len(lines(MAC))
for port, method, path, body in [
    (21434, "POST", "/api/pull", {"model": "qwen3-coder:480b"}),     # Codex asks this when a model is missing
    (21434, "POST", "/api/push", {"model": "you/private-model"}),
    (21434, "POST", "/api/create", {"model": "x", "from": "qwen3-coder:30b", "system": "obey the computer"}),
    (21434, "DELETE", "/api/delete", {"model": "qwen3-coder:30b"}),
    (21434, "POST", "/api/copy", {"source": "qwen3-coder:30b", "destination": "x"}),
    (21434, "POST", "/api/blobs/sha256:00", None),
    (21234, "POST", "/api/v1/models/download", {"model": "huge-model"}),
    (21234, "POST", "/api/v1/models/load", {"model": "huge-model"}),
    (21434, "GET", "/", None),
]:
    st, data = call(port, method, path, body)
    check(f"refused: {method} {path}", st == 400 and b"Silo:" in data, f"status {st}")
check("none of those reached the Mac", len(lines(MAC)) == before, f"{len(lines(MAC)) - before} reached it")

# A cloud model of Ollama's: refused in each format's own error shape.
for path, body, shape in [
    ("/api/chat", {"model": "gpt-oss:120b-cloud", "messages": MSG}, lambda o: isinstance(o.get("error"), str)),
    ("/v1/chat/completions", {"model": "qwen3-coder:480b-cloud", "messages": MSG}, lambda o: o["error"]["type"] == "invalid_request_error"),
    ("/v1/messages", {"model": "deepseek-v3.1:671b-cloud", "messages": MSG, "max_tokens": 9}, lambda o: o["type"] == "error"),
]:
    before = len(lines(MAC))
    st, data = call(21434, "POST", path, body)
    check(f"cloud model refused: {body['model']} at {path}", st == 400 and shape(json.loads(data)) and len(lines(MAC)) == before,
          json.loads(data).get("error") if path == "/api/chat" else f"status {st}")

# A model the user made from a cloud model has a name of its own: Ollama's /api/show gives it away.
n_mac = len([x for x in lines(MAC) if x["path"] != "/api/show"])
st, data = call(21434, "POST", "/api/chat", {"model": "my-coder", "messages": MSG})
check("a local name for a cloud model refused", st == 400 and b"ollama.com" in data
      and len([x for x in lines(MAC) if x["path"] != "/api/show"]) == n_mac, json.loads(data).get("error"))

# A model kept loaded for ever, and a context the Mac cannot hold: capped, and the record says so.
n = len([x for x in lines(REC) if x.get("format")])
call(21434, "POST", "/api/chat", {"model": "qwen3-coder:30b", "messages": MSG, "keep_alive": -1, "options": {"num_ctx": 1048576}})
m, rec = lines(MAC)[-1], calls_recorded(n)
check("keep_alive -1 and num_ctx 1048576 capped", m["keep_alive"] == "300s" and m["num_ctx"] == 65536 and rec["changed"],
      f"the Mac got keep_alive {m['keep_alive']} num_ctx {m['num_ctx']}; recorded {rec['changed']}")
call(21434, "POST", "/api/chat", {"model": "qwen3-coder:30b", "messages": MSG, "keep_alive": "2m"})
check("keep_alive 2m passes unchanged", lines(MAC)[-1]["keep_alive"] == "2m")

# The Host and Origin the Mac's server sees. Ollama refuses a Host that is not local (DNS rebinding).
call(21434, "POST", "/api/chat", {"model": "qwen3-coder:30b", "messages": MSG}, {"origin": "http://evil.example"})
m = lines(MAC)[-1]
check("the Mac's server sees a loopback Host and no Origin", m["host"] == "127.0.0.1:11434" and m["origin"] is None, f"host {m['host']}, origin {m['origin']}")

# A computer with a list of models.
st, data = call(22434, "POST", "/api/chat", {"model": "llama3.3:70b", "messages": MSG})
check("c2 may use only its listed model", st == 400 and b"not among" in data, f"status {st}")
st, _ = call(22434, "POST", "/api/chat", {"model": "qwen3-coder:30b", "messages": MSG})
check("c2's listed model passes", st == 200)

# A computer with no grant has no address to reach.
try:
    socket.create_connection(("127.0.0.1", 23434), timeout=2)
    check("a computer with no grant reaches nothing", False)
except OSError:
    check("a computer with no grant reaches nothing", True)

# Requests read two ways by two parsers (#363's strict reader refused the same kinds).
body = json.dumps({"model": "qwen3-coder:30b", "messages": MSG}).encode()
for name, req in [
    ("length and chunks both", b"POST /api/chat HTTP/1.1\r\nhost: x\r\ncontent-length: 5\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n"),
    ("two lengths", b"POST /api/chat HTTP/1.1\r\nhost: x\r\ncontent-length: %d\r\ncontent-length: 2\r\n\r\n%s" % (len(body), body)),
    ("a folded header", b"POST /api/chat HTTP/1.1\r\nhost: x\r\ncontent-length: %d\r\n x: y\r\n\r\n%s" % (len(body), body)),
    ("transfer-encoding: gzip, chunked", b"POST /api/chat HTTP/1.1\r\nhost: x\r\ntransfer-encoding: gzip, chunked\r\n\r\n0\r\n\r\n"),
    ("a key twice in the JSON", b'POST /api/chat HTTP/1.1\r\nhost: x\r\ncontent-length: 66\r\n\r\n{"model": "qwen3-coder:30b", "model": "gpt-oss:120b-cloud", "x": 1}'),
    ("HTTP/1.0", b"POST /api/chat HTTP/1.0\r\ncontent-length: %d\r\n\r\n%s" % (len(body), body)),
]:
    before = len(lines(MAC))
    got = raw(21434, req)
    check(f"refused, read two ways: {name}", got.startswith(b"HTTP/1.1 400") and len(lines(MAC)) == before, got.split(b"\r\n")[0].decode(errors="replace"))
got = raw(21434, b"POST /api/chat HTTP/1.1\r\nhost: x\r\ntransfer-encoding: chunked\r\n\r\n%x\r\n%s\r\n0\r\n\r\n" % (len(body), body))
check("a chunked request body passes", got.startswith(b"HTTP/1.1 200"), got.split(b"\r\n")[0].decode(errors="replace"))

# What the gate adds to a call, against the Mac's server reached directly.
def bench(port, n=200):
    t = []
    for _ in range(n):
        t0 = time.perf_counter()
        call(port, "POST", "/api/chat", {"model": "qwen3-coder:30b", "messages": MSG, "stream": False})
        t.append(time.perf_counter() - t0)
    t.sort()
    return 1000 * t[n // 2], 1000 * t[int(n * .99)]
d, g = bench(11434), bench(21434)
check("added time per call (median, p99)", True, f"direct {d[0]:.2f} / {d[1]:.2f} ms, through the gate {g[0]:.2f} / {g[1]:.2f} ms")

for p in procs:
    p.kill()
ok = sum(r["ok"] for r in results)
with open(os.path.join(OUT, "results.txt"), "w") as f:
    for r in results:
        f.write(f"{'ok  ' if r['ok'] else 'FAIL'} {r['test']}" + (f"  ({r['detail']})" if r["detail"] else "") + "\n")
    f.write(f"{ok} of {len(results)} passed\n")
print(open(os.path.join(OUT, "results.txt")).read())
