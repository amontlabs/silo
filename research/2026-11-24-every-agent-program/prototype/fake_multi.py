"""A fake model provider for two formats: OpenAI's Chat Completions and Google's generateContent.
A request that offers tools is the main conversation: its n-th such request gets the n-th step of
the scenario (tool calls), and later ones get "Done.". Requests offering no tools get a short text.
Every request is written whole to requests.jsonl; tool results are written to log.txt.
env: PORT, TLS_CERT, TLS_KEY, OUT, SCENARIO ({"steps":[{"calls":[{"name","input"}]}], "gemini_ids": bool})"""
import json, os, sys, time, uuid, http.server, ssl, threading, gzip, socketserver
OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
SCEN = json.load(open(os.environ["SCENARIO"])) if os.environ.get("SCENARIO") else {"steps": []}
LOGF = open(f"{OUT}/log.txt", "a"); REQF = open(f"{OUT}/requests.jsonl", "a"); lock = threading.Lock(); state = {"main": 0}
def log(s):
    with lock: LOGF.write(f"{time.time():.3f} {s}\n"); LOGF.flush()
def tool_names(body):
    names = []
    for t in body.get("tools", []) or []:
        if not isinstance(t, dict): continue
        if "function" in t: names.append(t["function"].get("name"))
        elif "name" in t: names.append(t["name"])
        for fd in t.get("functionDeclarations", []) or t.get("function_declarations", []) or []: names.append(fd.get("name"))
    return names
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass
    def send_json(self, o, code=200):
        b = json.dumps(o).encode(); self.send_response(code); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_GET(self):
        log(f"GET {self.path}")
        if "models" in self.path: return self.send_json({"object": "list", "data": [{"id": "fake-model", "object": "model"}], "models": []})
        self.send_json({}, 404)
    def do_POST(self):
        n = int(self.headers.get("content-length", 0)); raw = self.rfile.read(n)
        if (self.headers.get("content-encoding") or "") == "gzip": raw = gzip.decompress(raw)
        try: body = json.loads(raw)
        except Exception: body = {}
        req = body.get("request", body)  # Code Assist wraps the request
        with lock: REQF.write(json.dumps({"t": time.time(), "path": self.path, "headers": {k: (v if k.lower() not in ("authorization", "x-goog-api-key", "api-key") else "…") for k, v in self.headers.items()}, "body": body}) + "\n"); REQF.flush()
        names = tool_names(req)
        gem = "enerateContent" in self.path; ant = "/v1/messages" in self.path and "count_tokens" not in self.path
        if "count_tokens" in self.path: return self.send_json({"input_tokens": 100})
        if "/chat/completions" not in self.path and not gem and not ant:
            log(f"POST other {self.path}"); return self.send_json({})
        self.results(req, gem, ant)
        calls = []
        if names:
            k = state["main"]; state["main"] += 1
            log(f"main request {k} path={self.path} tools={names}")
            if k < len(SCEN["steps"]): calls = SCEN["steps"][k]["calls"]
        else:
            log(f"side request path={self.path}")
        return self.gemini(calls, req) if gem else self.anthropic(calls, req) if ant else self.chat(calls, req)
    def results(self, req, gem, ant=False):
        if ant:
            ms = req.get("messages", [])
            if ms and isinstance(ms[-1].get("content"), list):
                for c in ms[-1]["content"]:
                    if c.get("type") == "tool_result": log("RESULT " + json.dumps({"id": c.get("tool_use_id"), "is_error": c.get("is_error"), "content": c.get("content")})[:600])
            return
        if gem:
            cs = req.get("contents", [])
            for p in (cs[-1].get("parts", []) if cs else []):
                if "functionResponse" in p: log("RESULT " + json.dumps(p["functionResponse"])[:600])
        else:
            for m in reversed(req.get("messages", [])):
                if m.get("role") != "tool": break
                log("RESULT " + json.dumps({"id": m.get("tool_call_id"), "content": m.get("content")})[:600])
    def chat(self, calls, req):
        model = req.get("model", "fake-model"); cid = "chatcmpl-" + uuid.uuid4().hex[:12]
        def chunk(delta, fr=None, usage=None):
            o = {"id": cid, "object": "chat.completion.chunk", "created": int(time.time()), "model": model, "choices": [{"index": 0, "delta": delta, "finish_reason": fr}]}
            if usage: o["usage"] = usage
            return b"data: " + json.dumps(o).encode() + b"\n\n"
        evs = [chunk({"role": "assistant", "content": ""})]
        if calls:
            for i, c in enumerate(calls):
                tid = "call_" + uuid.uuid4().hex[:16]; args = json.dumps(c["input"])
                evs.append(chunk({"tool_calls": [{"index": i, "id": tid, "type": "function", "function": {"name": c["name"], "arguments": ""}}]}))
                for k in range(0, len(args), 12): evs.append(chunk({"tool_calls": [{"index": i, "function": {"arguments": args[k:k+12]}}]}))
            evs.append(chunk({}, "tool_calls"))
        else:
            evs.append(chunk({"content": "Done."})); evs.append(chunk({}, "stop"))
        evs.append(b"data: " + json.dumps({"id": cid, "object": "chat.completion.chunk", "model": model, "choices": [], "usage": {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}}).encode() + b"\n\n")
        evs.append(b"data: [DONE]\n\n")
        if not req.get("stream"):
            msg = {"role": "assistant", "content": None if calls else "Done."}
            if calls: msg["tool_calls"] = [{"id": "call_" + uuid.uuid4().hex[:16], "type": "function", "function": {"name": c["name"], "arguments": json.dumps(c["input"])}} for c in calls]
            return self.send_json({"id": cid, "object": "chat.completion", "model": model, "choices": [{"index": 0, "message": msg, "finish_reason": "tool_calls" if calls else "stop"}], "usage": {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}})
        self.stream(evs)
    def anthropic(self, calls, req):
        model = req.get("model", "claude-x"); mid = "msg_" + uuid.uuid4().hex[:20]
        def ev(t, d): d["type"] = t; return f"event: {t}\ndata: {json.dumps(d)}\n\n".encode()
        evs = [ev("message_start", {"message": {"id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "usage": {"input_tokens": 50, "output_tokens": 1}}})]
        blocks = [{"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:16], "name": c["name"], "input": c["input"]} for c in calls] or [{"type": "text", "text": "Done."}]
        for i, b in enumerate(blocks):
            if b["type"] == "text":
                evs.append(ev("content_block_start", {"index": i, "content_block": {"type": "text", "text": ""}})); evs.append(ev("content_block_delta", {"index": i, "delta": {"type": "text_delta", "text": b["text"]}}))
            else:
                evs.append(ev("content_block_start", {"index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}))
                js = json.dumps(b["input"])
                for k in range(0, len(js), 12): evs.append(ev("content_block_delta", {"index": i, "delta": {"type": "input_json_delta", "partial_json": js[k:k+12]}}))
            evs.append(ev("content_block_stop", {"index": i}))
        evs.append(ev("message_delta", {"delta": {"stop_reason": "tool_use" if calls else "end_turn"}, "usage": {"output_tokens": 20}})); evs.append(ev("message_stop", {}))
        if not req.get("stream"):
            return self.send_json({"id": mid, "type": "message", "role": "assistant", "model": model, "content": blocks, "stop_reason": "tool_use" if calls else "end_turn", "usage": {"input_tokens": 50, "output_tokens": 20}})
        self.stream(evs)
    def gemini(self, calls, req):
        parts = []
        for c in calls:
            fc = {"name": c["name"], "args": c["input"]}
            if SCEN.get("gemini_ids"): fc["id"] = "fc_" + uuid.uuid4().hex[:10]
            parts.append({"functionCall": fc})
        if not parts: parts = [{"text": "Done."}]
        o = {"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}], "usageMetadata": {"promptTokenCount": 50, "candidatesTokenCount": 20, "totalTokenCount": 70}, "modelVersion": "fake-gemini"}
        if "cloudcode" in (self.headers.get("host") or "") or "v1internal" in self.path: o = {"response": o}
        if "streamGenerateContent" not in self.path: return self.send_json(o)
        self.stream([b"data: " + json.dumps(o).encode() + b"\r\n\r\n"])
    def stream(self, evs):
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("transfer-encoding", "chunked"); self.end_headers()
        for e in evs:
            self.wfile.write(b"%x\r\n" % len(e) + e + b"\r\n"); self.wfile.flush(); time.sleep(0.005)
        self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()
class TS(socketserver.ThreadingMixIn, http.server.HTTPServer): daemon_threads = True
srv = TS((os.environ.get("BIND", "127.0.0.1"), int(os.environ.get("PORT", 8443))), H)
if os.environ.get("TLS_CERT"):
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); ctx.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"]); srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
srv.serve_forever()
