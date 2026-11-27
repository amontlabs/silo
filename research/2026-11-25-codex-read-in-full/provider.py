"""A stand-in for OpenAI's API and ChatGPT's backend, over TLS and WebSocket, for Codex only.

Grown from #403's provider.py. What it adds:

- ChatGPT's workspace check answers with `account_routing_override`, which Codex 0.172.0 requires
  to be one of "NO_CONSTRAINT", "us" or "us_cr" (codex-rs/app-server/src/request_processors/
  account_processor/workspace_routing.rs, `resolve_routing`). #403's answer had none, so Codex
  stopped there with "invalid account routing override".
- A scripted model. The first call of a turn is answered with one tool call: in code mode (the
  request offers `exec` in an `additional_tools` item) a `custom_tool_call` to `exec` whose input
  is the JavaScript in SCRIPT; otherwise a `function_call` to `exec_command` running CMD. A call
  whose input ends with a tool's output is answered with a short text, which ends the turn.
- Every model request body is kept whole in BODIES/<case>-<n>.json, over HTTP or WebSocket, with
  the output items of the reply, so the tool calls and what Codex sends after they ran can be
  read offline (read.py).
- Every request is logged with its header names, and the values of the few headers that say who
  is calling (`chatgpt-account-id`, `x-openai-account-routing-override`, `originator`, `version`,
  `session_id`, `openai-beta`). `authorization` is logged as its scheme only.
- DENY: comma-separated path pieces answered 403 instead, to see which requests Codex needs.
- POST /oauth/token (auth.openai.com) answers a refreshed sign-in, for the refresh case.
- WS=0 refuses every WebSocket upgrade with 404, as #374's stand-in does, so Codex sends its model
  calls as plain requests.

Environment: PORT, TLS_CERT, TLS_KEY, LOG, BODIES, CASE_FILE (a file holding the case's name),
SCRIPT (a file), CMD, DENY, WS (default 1).
"""
import base64, hashlib, json, os, socket, ssl, struct, threading, time, uuid, gzip

PORT = int(os.environ.get("PORT", "18443"))
LOG = open(os.environ.get("LOG", "provider.jsonl"), "a")
BODIES = os.environ.get("BODIES", "bodies")
CASE_FILE = os.environ.get("CASE_FILE", "")
WS = os.environ.get("WS", "1") == "1"
DENY = [d for d in os.environ.get("DENY", "").split(",") if d]
LOCK = threading.Lock()
SEQ = [0]
REPLY = "Done."
CMD = os.environ.get("CMD", "echo made-by-exec-command > made.txt")
SHOWN = ("chatgpt-account-id", "x-openai-account-routing-override", "originator", "version",
         "session_id", "openai-beta", "x-codex-installation-id", "x-openai-fedramp")
os.makedirs(BODIES, exist_ok=True)


def case():
    try:
        return open(CASE_FILE).read().strip() or "none"
    except OSError:
        return "none"


def deny_now():
    try:
        return [d.strip() for d in open(CASE_FILE + ".deny").read().split(",") if d.strip()]
    except OSError:
        return DENY


def b64url(o):
    return base64.urlsafe_b64encode(json.dumps(o).encode()).rstrip(b"=").decode()


def jwt(claims):
    return b64url({"alg": "none", "typ": "JWT"}) + "." + b64url(claims) + ".sig"


def chatgpt_claims(hours=24):
    now = int(time.time())
    return {"email": "night@example.org", "exp": now + hours * 3600, "iat": now,
            "https://api.openai.com/auth": {"chatgpt_plan_type": "pro", "chatgpt_account_id": "acct-test",
                                            "chatgpt_user_id": "user-test"}}


ACCOUNTS = {"accounts": [{"id": "acct-test", "account_id": "acct-test", "plan_type": "pro", "is_deactivated": False,
                          "workspace_backend_origin": "https://chatgpt.com", "account_routing_override": "NO_CONSTRAINT",
                          "structure": "personal"}],
            "account_ordering": ["acct-test"]}


def log(**o):
    o["t"] = round(time.time(), 3)
    o.setdefault("case", case())
    with LOCK:
        LOG.write(json.dumps(o) + "\n"); LOG.flush()


def keep_body(body, how, reply):
    with LOCK:
        SEQ[0] += 1
        n = SEQ[0]
    name = f"{BODIES}/{case()}-{n:03d}.json"
    with open(name, "w") as f:
        json.dump({"via": how, "body": body, "reply": reply}, f, indent=1)
    return name


class Conn:
    def __init__(self, s):
        self.s = s; self.buf = b""

    def read(self, n):
        while len(self.buf) < n:
            d = self.s.recv(65536)
            if not d:
                raise EOFError
            self.buf += d
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def head(self):
        while b"\r\n\r\n" not in self.buf:
            d = self.s.recv(65536)
            if not d:
                raise EOFError
            self.buf += d
        h, self.buf = self.buf.split(b"\r\n\r\n", 1)
        lines = h.decode("latin-1").split("\r\n")
        method, path, _ = lines[0].split(" ", 2)
        hd = {}
        for l in lines[1:]:
            k, _, v = l.partition(":"); hd[k.strip().lower()] = v.strip()
        return method, path, hd

    def body(self, hd):
        if "content-length" in hd:
            return self.read(int(hd["content-length"]))
        if hd.get("transfer-encoding", "").lower() == "chunked":
            out = b""
            while True:
                while b"\r\n" not in self.buf:
                    self.buf += self.s.recv(65536)
                size, self.buf = self.buf.split(b"\r\n", 1)
                n = int(size.split(b";")[0], 16)
                if n == 0:
                    self.read(2); return out
                out += self.read(n); self.read(2)
        return b""


def send(s, code, body, ctype="application/json", extra=""):
    if isinstance(body, (dict, list)):
        body = json.dumps(body).encode()
    s.sendall(f"HTTP/1.1 {code} X\r\ncontent-type: {ctype}\r\ncontent-length: {len(body)}\r\n{extra}\r\n".encode() + body)


def sse_head(s):
    s.sendall(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n")


def offered(body):
    names = [t.get("name") or t.get("type") for t in body.get("tools") or []]
    for it in body.get("input") or []:
        if isinstance(it, dict) and it.get("type") == "additional_tools":
            for ns in it.get("tools") or []:
                names += [t.get("name") for t in ns.get("tools") or []]
    return names


def after_tool(body):
    items = [i for i in body.get("input") or [] if isinstance(i, dict)]
    return bool(items) and items[-1].get("type") in ("function_call_output", "custom_tool_call_output")


def reply_items(body):
    if body.get("generate") is False:
        return []
    msg = {"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "status": "completed", "role": "assistant",
           "content": [{"type": "output_text", "text": REPLY, "annotations": []}]}
    if after_tool(body):
        return [msg]
    names = offered(body)
    cid = "call_" + uuid.uuid4().hex[:20]
    script = os.environ.get("SCRIPT")
    if "exec" in names and "exec_command" not in names and script:
        return [{"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:20], "call_id": cid, "name": "exec",
                 "input": open(script).read(), "status": "completed"}]
    if "exec_command" in names:
        return [{"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": cid, "name": "exec_command",
                 "arguments": json.dumps({"cmd": CMD}), "status": "completed"}]
    return [msg]


def responses_events(items):
    rid = "resp_" + uuid.uuid4().hex[:20]
    yield {"type": "response.created", "sequence_number": 0, "response": {"id": rid, "object": "response", "status": "in_progress", "output": []}}
    for k, it in enumerate(items):
        yield {"type": "response.output_item.added", "output_index": k, "item": it}
        yield {"type": "response.output_item.done", "output_index": k, "item": it}
    yield {"type": "response.completed", "response": {"id": rid, "object": "response", "status": "completed", "output": items, "usage": {"input_tokens": 50, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 6, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 56}}}


def ws_frame(op, data):
    n = len(data)
    h = bytes([0x80 | op]) + (bytes([n]) if n < 126 else bytes([126]) + struct.pack(">H", n) if n < 65536 else bytes([127]) + struct.pack(">Q", n))
    return h + data


def ws_serve(c, hd, cid, sni, path):
    key = hd["sec-websocket-key"]
    acc = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
    c.s.sendall(f"HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {acc}\r\n\r\n".encode())
    log(ev="ws-open", conn=cid, sni=sni, path=path)
    msg = b""
    while True:
        b1, b2 = c.read(2)
        op = b1 & 0x0F; n = b2 & 0x7F
        if n == 126:
            n = struct.unpack(">H", c.read(2))[0]
        elif n == 127:
            n = struct.unpack(">Q", c.read(8))[0]
        mask = c.read(4) if b2 & 0x80 else b"\0\0\0\0"
        data = bytes(x ^ mask[i % 4] for i, x in enumerate(c.read(n)))
        if op == 8:
            log(ev="ws-close", conn=cid); c.s.sendall(ws_frame(8, data[:2])); return
        if op == 9:
            c.s.sendall(ws_frame(10, data)); continue
        if op in (0, 1, 2):
            msg += data
            if not b1 & 0x80:
                continue
            m, msg = msg, b""
            try:
                body = json.loads(m)
            except ValueError:
                log(ev="ws-in-unreadable", conn=cid, bytes=len(m)); continue
            items = reply_items(body)
            kept = keep_body(body, "websocket " + path, items)
            log(ev="ws-in", conn=cid, type=body.get("type"), generate=body.get("generate"), items=len(body.get("input") or []),
                previous=body.get("previous_response_id"), kept=kept)
            for e in responses_events(items):
                c.s.sendall(ws_frame(1, json.dumps(e).encode()))


def shown(hd):
    out = {k: hd[k] for k in SHOWN if k in hd}
    if "authorization" in hd:
        out["authorization"] = hd["authorization"].split(" ")[0] + " …"
    return out


def handle(raw, addr):
    cid = uuid.uuid4().hex[:8]
    try:
        s = CTX.wrap_socket(raw, server_side=True)
    except (ssl.SSLError, OSError) as e:
        log(ev="tls-failed", conn=cid, error=str(e)[:200]); raw.close(); return
    sni = getattr(s, "_sni", None)
    log(ev="tls", conn=cid, sni=sni, alpn=s.selected_alpn_protocol())
    c = Conn(s)
    try:
        while True:
            method, path, hd = c.head()
            body_raw = c.body(hd)
            if hd.get("content-encoding") == "gzip":
                body_raw = gzip.decompress(body_raw)
            elif hd.get("content-encoding") == "zstd":
                import zstandard
                body_raw = zstandard.ZstdDecompressor().decompress(body_raw, max_output_size=1 << 28)
            denied = any(d in path for d in deny_now())
            log(ev="http", conn=cid, sni=sni, method=method, path=path, host=hd.get("host"), upgrade=hd.get("upgrade"),
                headers=sorted(hd), who=shown(hd), bytes=len(body_raw), denied=denied)
            if denied:
                send(s, 403, {"error": {"message": "denied by the test", "type": "forbidden"}}); continue
            if hd.get("upgrade", "").lower() == "websocket":
                if not WS:
                    send(s, 404, {"error": {"message": "no websocket here"}}); continue
                return ws_serve(c, hd, cid, sni, path)
            try:
                body = json.loads(body_raw) if body_raw else {}
            except ValueError:
                body = {}
            p = path.split("?")[0]
            if method == "POST" and p.endswith("/responses"):
                items = reply_items(body)
                kept = keep_body(body, "http " + path, items)
                log(ev="http-model", conn=cid, kept=kept)
                sse_head(s)
                for d in responses_events(items):
                    s.sendall(f"event: {d['type']}\ndata: {json.dumps(d)}\n\n".encode())
                return
            if "/accounts/check" in p:
                send(s, 200, ACCOUNTS); continue
            if p == "/oauth/token":
                tok = jwt(chatgpt_claims())
                send(s, 200, {"id_token": tok, "access_token": tok, "refresh_token": "rt-test-2"}); continue
            send(s, 200, {} if "models" not in p else {"data": [], "models": []})
    except (EOFError, OSError, ssl.SSLError, ValueError):
        pass
    finally:
        try:
            s.close()
        except OSError:
            pass


CTX = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
CTX.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"])
CTX.set_alpn_protocols(["http/1.1"])


def on_sni(sock, name, ctx):
    sock._sni = name


CTX.sni_callback = on_sni
L = socket.socket(); L.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
L.bind(("127.0.0.1", PORT)); L.listen(64)
while True:
    a, addr = L.accept()
    threading.Thread(target=handle, args=(a, addr), daemon=True).start()
