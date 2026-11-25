"""A stand-in for Anthropic's and OpenAI's APIs, over TLS, that also speaks WebSocket.

It answers every model call with one short text reply, so a program ends its turn at once, and
writes one JSON line per event to LOG: each TLS connection (the name the client asked for and the
protocol it agreed, ALPN), each HTTP request, and each WebSocket message both ways. It is the
smallest stand-in that lets Claude Code and Codex finish one turn; the spend and tool-call
stand-ins of #374 and #385 are bigger and do not speak WebSocket.

- Anthropic: POST /v1/messages answers a stream of events; anything else answers 200 {}.
- OpenAI: POST /v1/responses answers a stream of events. GET /v1/responses with
  "Upgrade: websocket" opens a WebSocket (RFC 6455, no compression: the extension Codex asks for
  is not agreed), where each `response.create` message is answered with the same events, one per
  message. A message with "generate": false (Codex's warm-up) gets a reply with no output.
  With WS=0 the upgrade is refused with 404, as #374's stand-in does.

With TOOL set to a command, Claude Code's first model call is answered with a call of its Bash
tool running that command, and the tool's result is logged when it comes back.

Environment: PORT, TLS_CERT, TLS_KEY, LOG, WS (default 1), TOOL.
"""
import base64, hashlib, json, os, socket, ssl, struct, threading, time, uuid, gzip

PORT = int(os.environ.get("PORT", "18443"))
LOG = open(os.environ.get("LOG", "provider.jsonl"), "a")
WS = os.environ.get("WS", "1") == "1"
LOCK = threading.Lock()
REPLY = "Hello from the stand-in."
TOOL = os.environ.get("TOOL")
# Codex signed in with ChatGPT asks for its workspaces first. This answer gets it past two of its
# checks, not the third ("invalid account routing override"); its real shape was not found, so
# the model call of a ChatGPT sign-in was not reached here. See the README.
ACCOUNTS = {"accounts": [{"id": "acct-test", "account_id": "acct-test", "plan_type": "pro", "is_deactivated": False,
                          "workspace_backend_origin": "https://chatgpt.com", "structure": "personal"}],
            "account_ordering": ["acct-test"]}


def log(**o):
    o["t"] = round(time.time(), 3)
    with LOCK:
        LOG.write(json.dumps(o) + "\n"); LOG.flush()


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


def tool_result(body):
    """The text of the last tool result in an Anthropic request, or None."""
    for m in reversed(body.get("messages") or []):
        if m.get("role") == "user" and isinstance(m.get("content"), list):
            for b in m["content"]:
                if isinstance(b, dict) and b.get("type") == "tool_result":
                    c = b.get("content")
                    return c if isinstance(c, str) else json.dumps(c)
            return None
    return None


def anthropic_events(body):
    model = body.get("model", "claude")
    mid = "msg_" + uuid.uuid4().hex[:20]
    yield "message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "stop_sequence": None, "usage": {"input_tokens": 50, "output_tokens": 1, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}}}
    tools = [t.get("name") for t in body.get("tools") or []]
    result = tool_result(body)
    if result is not None:
        log(ev="tool-result", text=result[:400])
    if TOOL and "Bash" in tools and result is None:
        # TOOL: a command for Claude Code's Bash tool, asked once per conversation
        call = {"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": "Bash", "input": {}}
        yield "content_block_start", {"type": "content_block_start", "index": 0, "content_block": call}
        yield "content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": json.dumps({"command": TOOL, "description": "Reach the API"})}}
        yield "content_block_stop", {"type": "content_block_stop", "index": 0}
        yield "message_delta", {"type": "message_delta", "delta": {"stop_reason": "tool_use", "stop_sequence": None}, "usage": {"output_tokens": 20}}
        yield "message_stop", {"type": "message_stop"}
        return
    yield "content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}
    yield "content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": REPLY}}
    yield "content_block_stop", {"type": "content_block_stop", "index": 0}
    yield "message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None}, "usage": {"output_tokens": 6}}
    yield "message_stop", {"type": "message_stop"}


def responses_events(body):
    rid = "resp_" + uuid.uuid4().hex[:20]
    items = [] if body.get("generate") is False else [{"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": REPLY, "annotations": []}]}]
    yield {"type": "response.created", "sequence_number": 0, "response": {"id": rid, "object": "response", "status": "in_progress", "output": []}}
    for k, it in enumerate(items):
        yield {"type": "response.output_item.added", "output_index": k, "item": it}
        yield {"type": "response.output_item.done", "output_index": k, "item": it}
    yield {"type": "response.completed", "response": {"id": rid, "object": "response", "status": "completed", "output": items, "usage": {"input_tokens": 50, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 6, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 56}}}


def ws_frame(op, data):
    n = len(data)
    h = bytes([0x80 | op]) + (bytes([n]) if n < 126 else bytes([126]) + struct.pack(">H", n) if n < 65536 else bytes([127]) + struct.pack(">Q", n))
    return h + data


def ws_serve(c, hd, cid, sni):
    key = hd["sec-websocket-key"]
    acc = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
    c.s.sendall(f"HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {acc}\r\n\r\n".encode())
    log(ev="ws-open", conn=cid, sni=sni, asked_extensions=hd.get("sec-websocket-extensions"))
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
            log(ev="ws-in", conn=cid, type=body.get("type"), generate=body.get("generate"), items=len(body.get("input") or []), previous=body.get("previous_response_id"))
            for e in responses_events(body):
                c.s.sendall(ws_frame(1, json.dumps(e).encode()))
            log(ev="ws-out", conn=cid, events=4 if body.get("generate") is not False else 2)


def handle(raw, addr):
    cid = uuid.uuid4().hex[:8]
    sni = {}
    try:
        s = CTX.wrap_socket(raw, server_side=True)
    except (ssl.SSLError, OSError) as e:
        log(ev="tls-failed", conn=cid, error=str(e)[:200]); raw.close(); return
    log(ev="tls", conn=cid, sni=getattr(s, "_sni", None), alpn=s.selected_alpn_protocol(), version=s.version())
    c = Conn(s)
    try:
        while True:
            method, path, hd = c.head()
            body_raw = c.body(hd)
            if hd.get("content-encoding") == "gzip":
                body_raw = gzip.decompress(body_raw)
            log(ev="http", conn=cid, method=method, path=path, host=hd.get("host"), upgrade=hd.get("upgrade"), ua=hd.get("user-agent", "")[:80], bytes=len(body_raw))
            if hd.get("upgrade", "").lower() == "websocket":
                if WS:
                    return ws_serve(c, hd, cid, getattr(s, "_sni", None))
                send(s, 404, {"error": {"message": "no websocket here"}}); continue
            try:
                body = json.loads(body_raw) if body_raw else {}
            except ValueError:
                body = {}
            if method == "POST" and path.split("?")[0].endswith("/v1/messages"):
                sse_head(s)
                for name, d in anthropic_events(body):
                    s.sendall(f"event: {name}\ndata: {json.dumps(d)}\n\n".encode())
                return
            if method == "POST" and path.split("?")[0].endswith("/responses"):
                sse_head(s)
                for d in responses_events(body):
                    s.sendall(f"event: {d['type']}\ndata: {json.dumps(d)}\n\n".encode())
                return
            if "/accounts/check" in path:
                # Codex signed in with ChatGPT finds its workspace here before any model call
                send(s, 200, ACCOUNTS); continue
            send(s, 200, {} if "models" not in path else {"data": [], "models": []})
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
