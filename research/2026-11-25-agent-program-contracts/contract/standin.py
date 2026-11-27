"""The model providers, played for a contract run, and a recording of everything a program sends.

It answers Anthropic Messages, OpenAI Responses (HTTP stream and WebSocket), OpenAI Chat
Completions and Gemini, as the providers do on 25 November 2026, with one scripted task: the
model's first answer in a conversation that offers tools is one shell tool call, and its answer
once that call's result came back is a short text. A conversation that offers no tools (a title,
a summary) gets a short text. Every token count is fixed by the request, so what the boundary
counts can be compared with what was billed.

It writes, in OUT:
  requests.jsonl  every request as received: method, host, path, headers (credentials reduced to
                  their kind), the decoded body, its encoding, and the transport
  sent.jsonl      for each model call: the format, the model, the usage billed, and each tool
                  call sent (id, name, input and its sha256 as the wire carries it)
  standin.log     anything else

Run inside the contract's network namespace only. Environment: PORT, TLS_CERT, TLS_KEY, OUT,
WS (1: accept WebSockets, the default; 0: refuse them with 404 as #374's stand-in did).
"""
import base64, gzip, hashlib, json, os, socket, ssl, struct, threading, time, uuid, zlib, http.server

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
LOCK = threading.Lock()
FILES = {n: open(f"{OUT}/{n}", "a") for n in ("requests.jsonl", "sent.jsonl", "standin.log")}
T0 = time.time()
CMD = os.environ.get("CMD", "echo contract-ok > contract-proof.txt")


def write(name, o):
    with LOCK:
        FILES[name].write((o if isinstance(o, str) else json.dumps(o)) + "\n"); FILES[name].flush()


def log(s):
    write("standin.log", f"{time.time()-T0:8.2f} {s}")


def sha(s):
    return hashlib.sha256(s.encode() if isinstance(s, str) else s).hexdigest()


def cred_kind(h):
    """What kind of credential a request carries, never its value."""
    for k in ("x-api-key", "x-goog-api-key", "authorization"):
        v = h.get(k)
        if not v:
            continue
        v = v[7:] if v.lower().startswith("bearer ") else v
        for pre in ("sk-ant-oat", "sk-ant-api", "sk-proj-", "sk-", "AIza", "ya29.", "eyJ"):
            if v.startswith(pre):
                return f"{k}:{pre}"
        return f"{k}:other"
    return "none"


SECRET_HEADERS = {"x-api-key", "x-goog-api-key", "authorization", "cookie", "chatgpt-account-id"}


def tokens_of(obj):
    return max(1, len(json.dumps(obj, separators=(",", ":"))) // 4)


# ---- the scripted model ----

def shell_call(names, custom_names, cmd):
    """The one tool call the model makes, in whichever shell tool the program offers."""
    if "exec" in custom_names and "exec_command" not in names:
        # Codex's code mode (#399): one custom JavaScript tool that calls the nested tools.
        return {"name": "exec", "custom": True,
                "input": "const r = await tools.exec_command({cmd: %s});\ntext(JSON.stringify(r));" % json.dumps(cmd)}
    if "Bash" in names: return {"name": "Bash", "input": {"command": cmd, "description": "Write the proof file"}}
    if "exec_command" in names: return {"name": "exec_command", "input": {"cmd": cmd}}
    if "shell_command" in names: return {"name": "shell_command", "input": {"command": cmd}}
    if "shell" in names: return {"name": "shell", "input": {"command": ["bash", "-lc", cmd]}}
    if "run_shell_command" in names: return {"name": "run_shell_command", "input": {"command": cmd}}
    return None


FINAL = "Contract task done: the proof file is written."


# ---- HTTP and WebSocket ----

class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def raw_body(self):
        if (self.headers.get("transfer-encoding") or "").lower() == "chunked":
            raw = b""
            while True:
                n = int(self.rfile.readline().strip().split(b";")[0], 16)
                if n == 0:
                    self.rfile.readline(); break
                raw += self.rfile.read(n); self.rfile.readline()
        else:
            raw = self.rfile.read(int(self.headers.get("content-length") or 0))
        return raw

    def decode(self, raw):
        enc = (self.headers.get("content-encoding") or "").lower()
        try:
            if enc == "gzip": raw = gzip.decompress(raw)
            elif enc == "deflate": raw = zlib.decompress(raw)
            elif enc == "zstd":
                import zstandard; raw = zstandard.ZstdDecompressor().decompress(raw, max_output_size=1 << 28)
            elif enc == "br":
                import brotli; raw = brotli.decompress(raw)
        except Exception as e:
            log(f"cannot decode {enc}: {e}")
        try:
            return json.loads(raw) if raw else None
        except Exception:
            return {"_not_json": len(raw)}

    def note(self, method, body, transport):
        host = (self.headers.get("host") or "").split(":")[0]
        hdrs = {k.lower(): ("<" + cred_kind({k.lower(): v}) + ">" if k.lower() in SECRET_HEADERS else v) for k, v in self.headers.items()}
        write("requests.jsonl", {"t": round(time.time() - T0, 3), "method": method, "host": host, "path": self.path,
                                 "transport": transport, "credential": cred_kind({k.lower(): v for k, v in self.headers.items()}),
                                 "encoding": self.headers.get("content-encoding"), "headers": hdrs, "body": body})
        return host

    def send_json(self, o, code=200):
        b = json.dumps(o).encode()
        self.send_response(code); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b)))
        self.end_headers(); self.wfile.write(b)

    def sse(self, events, named=True):
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("cache-control", "no-cache")
        self.send_header("transfer-encoding", "chunked"); self.end_headers()
        for name, data in events:
            chunk = (f"event: {name}\n" if named and name else "") + f"data: {data if isinstance(data, str) else json.dumps(data)}\n\n"
            b = chunk.encode(); self.wfile.write(b"%x\r\n" % len(b) + b + b"\r\n")
        self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()

    def do_GET(self):
        if (self.headers.get("upgrade") or "").lower() == "websocket":
            return self.websocket()
        self.note("GET", None, "http")
        if "/models" in self.path:
            return self.send_json({"data": [], "object": "list", "models": []})
        return self.send_json({"error": {"type": "not_found_error", "message": "not here"}}, 404)

    def do_HEAD(self):
        self.note("HEAD", None, "http")
        self.send_response(200); self.send_header("content-length", "0"); self.end_headers()

    def do_POST(self):
        body = self.decode(self.raw_body())
        host = self.note("POST", body, "http")
        p = self.path.split("?")[0]
        try:
            if p.endswith("/messages/count_tokens"):
                return self.send_json({"input_tokens": tokens_of(body)})
            if p.endswith("/v1/messages"):
                return self.anthropic(host, body or {})
            if p.endswith("/responses"):
                return self.responses_http(host, body or {})
            if p.endswith("/chat/completions"):
                return self.chat(host, body or {})
            if ":countTokens" in p:
                return self.send_json({"totalTokens": tokens_of(body)})
            if ":streamGenerateContent" in p or ":generateContent" in p:
                return self.gemini(host, body or {})
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError) as e:
            log(f"client dropped: {e!r}"); return
        log(f"POST {host}{self.path}: not a model call")
        return self.send_json({"error": {"type": "not_found_error", "message": "not here"}}, 404)

    # ---- Anthropic Messages ----
    def anthropic(self, host, body):
        tools = body.get("tools") or []
        names = {t.get("name") for t in tools if isinstance(t, dict)}
        msgs = body.get("messages") or []
        results = sum(1 for m in msgs if isinstance(m.get("content"), list) for b in m["content"] if isinstance(b, dict) and b.get("type") == "tool_result")
        call = shell_call(names, set(), CMD) if tools and results == 0 else None
        usage = {"input_tokens": tokens_of(body), "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0, "output_tokens": 0}
        mid = "msg_" + uuid.uuid4().hex[:20]
        blocks = [{"type": "text", "text": "Working on it." if call else (FINAL if tools else "A short title")}]
        if call:
            blocks.append({"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": call["name"], "input": call["input"]})
        usage["output_tokens"] = 11 + sum(len(json.dumps(b)) // 4 for b in blocks)
        stop = "tool_use" if call else "end_turn"
        self.sent(host, "anthropic", body.get("model"), usage, [(b["id"], b["name"], json.dumps(b["input"])) for b in blocks if b["type"] == "tool_use"], bool(tools))
        if not body.get("stream"):
            return self.send_json({"id": mid, "type": "message", "role": "assistant", "model": body.get("model"), "content": blocks, "stop_reason": stop, "stop_sequence": None, "usage": usage})
        start = dict(usage, output_tokens=1)
        ev = [("message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant", "model": body.get("model"), "content": [], "stop_reason": None, "stop_sequence": None, "usage": start}})]
        for i, b in enumerate(blocks):
            if b["type"] == "text":
                ev.append(("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}))
                ev.append(("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": b["text"]}}))
            else:
                ev.append(("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}))
                ev.append(("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": json.dumps(b["input"])}}))
            ev.append(("content_block_stop", {"type": "content_block_stop", "index": i}))
        ev.append(("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": usage}))
        ev.append(("message_stop", {"type": "message_stop"}))
        self.sse(ev)

    # ---- OpenAI Responses ----
    def responses_events(self, host, body):
        tools = body.get("tools") or []
        names = {t.get("name") or t.get("type") for t in tools if isinstance(t, dict)}
        custom = {t.get("name") for t in tools if isinstance(t, dict) and t.get("type") == "custom"}
        inp = body.get("input") or []
        if isinstance(inp, str): inp = [{"type": "message", "role": "user", "content": inp}]
        for it in inp:
            if isinstance(it, dict) and it.get("type") == "additional_tools":
                for ns in it.get("tools") or []:
                    for x in ns.get("tools") or [ns]:
                        if isinstance(x, dict):
                            names.add(x.get("name")); custom.add(x.get("name")) if x.get("type") == "custom" else None
        offers = bool(names - {None})
        if body.get("generate") is False:
            # Codex's prewarm over the WebSocket: the request is sent ahead, nothing is generated.
            rid = "resp_" + uuid.uuid4().hex[:20]
            usage = {"input_tokens": 0, "output_tokens": 0, "total_tokens": 0}
            done = {"id": rid, "object": "response", "model": body.get("model"), "status": "completed", "output": [], "usage": usage}
            self.sent(host, "responses", body.get("model"), usage, [], offers, prewarm=True)
            return [{"type": "response.created", "response": dict(done, status="in_progress"), "sequence_number": 0},
                    {"type": "response.completed", "response": done, "sequence_number": 1}], done
        results = sum(1 for i in inp if isinstance(i, dict) and i.get("type", "").endswith("_output"))
        call = shell_call(names, custom, CMD) if offers and results == 0 else None
        items = []
        if call and call.get("custom"):
            items.append({"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20], "name": call["name"], "input": call["input"], "status": "completed"})
        elif call:
            items.append({"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20], "name": call["name"], "arguments": json.dumps(call["input"]), "status": "completed"})
        else:
            items.append({"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": FINAL if offers else "A short title", "annotations": []}]})
        inp_t = tokens_of(body); out_t = 11 + sum(len(json.dumps(i)) // 4 for i in items)
        usage = {"input_tokens": inp_t, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": out_t, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": inp_t + out_t}
        self.sent(host, "responses", body.get("model"), usage, [(i["call_id"], i["name"], i.get("arguments", i.get("input"))) for i in items if i["type"] != "message"], offers)
        rid = "resp_" + uuid.uuid4().hex[:20]
        resp = {"id": rid, "object": "response", "created_at": int(time.time()), "model": body.get("model"), "status": "in_progress", "output": [], "usage": None}
        ev = [{"type": "response.created", "response": resp}]
        for k, it in enumerate(items):
            if it["type"] == "function_call":
                ev.append({"type": "response.output_item.added", "output_index": k, "item": dict(it, arguments="", status="in_progress")})
                ev.append({"type": "response.function_call_arguments.delta", "output_index": k, "item_id": it["id"], "delta": it["arguments"]})
                ev.append({"type": "response.function_call_arguments.done", "output_index": k, "item_id": it["id"], "arguments": it["arguments"]})
            elif it["type"] == "custom_tool_call":
                ev.append({"type": "response.output_item.added", "output_index": k, "item": dict(it, input="", status="in_progress")})
                ev.append({"type": "response.custom_tool_call_input.delta", "output_index": k, "item_id": it["id"], "delta": it["input"]})
                ev.append({"type": "response.custom_tool_call_input.done", "output_index": k, "item_id": it["id"], "input": it["input"]})
            else:
                ev.append({"type": "response.output_item.added", "output_index": k, "item": dict(it, content=[], status="in_progress")})
                ev.append({"type": "response.output_text.delta", "output_index": k, "item_id": it["id"], "content_index": 0, "delta": it["content"][0]["text"]})
            ev.append({"type": "response.output_item.done", "output_index": k, "item": it})
        ev.append({"type": "response.completed", "response": dict(resp, status="completed", output=items, usage=usage)})
        for n, e in enumerate(ev): e["sequence_number"] = n
        return ev, dict(resp, status="completed", output=items, usage=usage)

    def responses_http(self, host, body):
        ev, done = self.responses_events(host, body)
        if not body.get("stream"):
            return self.send_json(done)
        self.sse([(e["type"], e) for e in ev])

    def websocket(self):
        host = self.note("GET", None, "websocket")
        if os.environ.get("WS", "1") == "0":
            log(f"websocket refused {self.path}")
            return self.send_json({"error": {"message": "no websocket here"}}, 404)
        key = self.headers.get("sec-websocket-key", "")
        acc = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.send_response(101); self.send_header("upgrade", "websocket"); self.send_header("connection", "Upgrade")
        self.send_header("sec-websocket-accept", acc); self.end_headers(); self.wfile.flush()
        log(f"websocket open {host}{self.path}")
        self.close_connection = True
        while True:
            try:
                op, data = self.ws_read()
            except (ConnectionError, ssl.SSLError, struct.error, ValueError) as e:
                log(f"websocket ended: {e!r}"); return
            if op == 8:
                self.ws_send(8, data[:2]); return
            if op == 9:
                self.ws_send(10, data); continue
            if op != 1:
                continue
            try:
                msg = json.loads(data)
            except Exception:
                log("websocket: a message that is not JSON"); continue
            write("requests.jsonl", {"t": round(time.time() - T0, 3), "method": "WS", "host": host, "path": self.path, "transport": "websocket",
                                     "credential": cred_kind({k.lower(): v for k, v in self.headers.items()}), "encoding": None, "headers": {}, "body": msg})
            if msg.get("type") != "response.create":
                continue
            body = {k: v for k, v in msg.items() if k != "type"}
            ev, _ = self.responses_events(host, body)
            for e in ev:
                self.ws_send(1, json.dumps(e).encode())

    def ws_read(self):
        msg, op0 = b"", None
        while True:
            h = self.rfile.read(2)
            if len(h) < 2: raise ConnectionError("closed")
            fin, op, masked, n = h[0] & 0x80, h[0] & 0x0F, h[1] & 0x80, h[1] & 0x7F
            if n == 126: n = struct.unpack(">H", self.rfile.read(2))[0]
            elif n == 127: n = struct.unpack(">Q", self.rfile.read(8))[0]
            mask = self.rfile.read(4) if masked else b"\0\0\0\0"
            data = bytes(b ^ mask[i % 4] for i, b in enumerate(self.rfile.read(n)))
            if h[0] & 0x40: raise ValueError("compressed frame: no extension was agreed")
            if op >= 8: return op, data
            op0 = op if op0 is None else op0
            msg += data
            if fin: return op0, msg

    def ws_send(self, op, data):
        n = len(data)
        head = bytes([0x80 | op]) + (bytes([n]) if n < 126 else (bytes([126]) + struct.pack(">H", n) if n < 65536 else bytes([127]) + struct.pack(">Q", n)))
        self.wfile.write(head + data); self.wfile.flush()

    # ---- OpenAI Chat Completions (no tool calls: no program in the suite speaks it) ----
    def chat(self, host, body):
        usage = {"prompt_tokens": tokens_of(body), "completion_tokens": 12, "total_tokens": tokens_of(body) + 12}
        self.sent(host, "chat", body.get("model"), usage, [], bool(body.get("tools")))
        return self.send_json({"id": "chatcmpl-x", "object": "chat.completion", "model": body.get("model"), "choices": [{"index": 0, "message": {"role": "assistant", "content": FINAL}, "finish_reason": "stop"}], "usage": usage})

    # ---- Google Gemini ----
    def gemini(self, host, body):
        model = self.path.split("/models/")[1].split(":")[0] if "/models/" in self.path else (body.get("model") or "")
        req = body.get("request", body)  # Code Assist wraps the request
        decls = [d for t in (req.get("tools") or []) for d in (t.get("functionDeclarations") or t.get("function_declarations") or [])]
        names = {d.get("name") for d in decls}
        contents = req.get("contents") or []
        results = sum(1 for c in contents for p in (c.get("parts") or []) if isinstance(p, dict) and "functionResponse" in p)
        call = shell_call(names, set(), CMD) if decls and results == 0 else None
        if call:
            parts = [{"functionCall": {"name": call["name"], "args": call["input"], "id": "gfc_" + uuid.uuid4().hex[:12]}}]
        else:
            gc = req.get("generationConfig") or {}
            side = json.dumps({}) if "json" in (gc.get("responseMimeType") or "") else "A short title"
            parts = [{"text": FINAL if decls else side}]
        inp_t = tokens_of(req); out_t = 11 + len(json.dumps(parts)) // 4
        um = {"promptTokenCount": inp_t, "candidatesTokenCount": out_t, "totalTokenCount": inp_t + out_t}
        self.sent(host, "gemini", model, um, [(p["functionCall"].get("id"), p["functionCall"]["name"], json.dumps(p["functionCall"]["args"])) for p in parts if "functionCall" in p], bool(decls))
        last = {"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}], "usageMetadata": um, "modelVersion": model}
        if "request" in body:
            last = {"response": last, "traceId": "t"}
        if ":generateContent" in self.path:
            return self.send_json(last)
        if "alt=sse" in self.path:
            return self.sse([("", last)], named=False)
        return self.send_json([last])

    def sent(self, host, fmt, model, usage, calls, offers_tools, prewarm=False):
        write("sent.jsonl", {"prewarm": prewarm, "t": round(time.time() - T0, 3), "host": host, "path": self.path, "format": fmt, "model": model, "usage": usage,
                             "offers_tools": offers_tools,
                             "tool_calls": [{"id": i, "name": n, "input": inp, "input_sha256": sha(inp or "")} for i, n, inp in calls]})


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 64

    def get_request(self):
        sock, addr = self.socket.accept()
        return CTX.wrap_socket(sock, server_side=True, do_handshake_on_connect=False), addr

    def finish_request(self, request, client_address):
        try:
            request.do_handshake()
        except Exception as e:
            log(f"handshake failed {e}"); return
        super().finish_request(request, client_address)


CTX = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
CTX.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"])
CTX.set_alpn_protocols(["http/1.1"])

if __name__ == "__main__":
    srv = S((os.environ.get("BIND", "127.0.0.1"), int(os.environ.get("PORT", "8443"))), H)
    log("listening"); srv.serve_forever()
