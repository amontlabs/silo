"""Stand-ins for a model server on the Mac: Ollama on 11434 and LM Studio on 1234, both on the Mac's
loopback only. Each answers in the shapes its real server uses (Ollama's API docs, OpenAI's, and
Anthropic's; LM Studio's routes as #369 listed them), streams like it, and reports usage like it.
Each request it gets is logged, so a test can tell what reached the Mac.

Like Ollama, the stand-in refuses a request whose Host is not a loopback or local name, which is how
Ollama guards against DNS rebinding.

usage: python3 mac_servers.py <log.jsonl>"""
import http.server, json, sys, threading, time

LOG = sys.argv[1]
PROMPT_TOKENS, OUT_WORDS = 26, ["The", " answer", " is", " 42", "."]


def log(d):
    with open(LOG, "a") as f:
        f.write(json.dumps(d) + "\n")


class Server(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    kind = "?"

    def log_message(self, *a):
        pass

    def send(self, code, body, ctype="application/json"):
        b = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def stream(self, ctype, chunks):
        self.send_response(200)
        self.send_header("content-type", ctype)
        self.send_header("transfer-encoding", "chunked")
        self.end_headers()
        for c in chunks:
            c = c.encode()
            self.wfile.write(b"%x\r\n%s\r\n" % (len(c), c))
            self.wfile.flush()
            time.sleep(0.01)
        self.wfile.write(b"0\r\n\r\n")

    def handle_any(self):
        n = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(n) if n else b""
        body = json.loads(raw) if raw.startswith(b"{") else {}
        path = self.path.split("?")[0]
        host = (self.headers.get("host") or "").rsplit(":", 1)[0]
        log({"server": self.kind, "method": self.command, "path": path, "host": self.headers.get("host"),
             "origin": self.headers.get("origin"), "model": body.get("model"), "keep_alive": body.get("keep_alive"),
             "num_ctx": (body.get("options") or {}).get("num_ctx")})
        if self.kind == "ollama" and host not in ("127.0.0.1", "localhost", "::1", "[::1]"):
            return self.send(403, b"", "text/plain")
        model = body.get("model", "qwen3-coder:30b")
        st = body.get("stream", self.kind == "ollama" and path.startswith("/api/"))
        out = "".join(OUT_WORDS)
        n_out = len(OUT_WORDS)
        if path in ("/api/tags", "/api/v0/models", "/api/v1/models"):
            return self.send(200, {"models": [{"name": "qwen3-coder:30b"}, {"name": "gpt-oss:120b-cloud"}]})
        if path == "/v1/models":
            return self.send(200, {"object": "list", "data": [{"id": "qwen3-coder:30b"}]})
        if path == "/api/show":
            # Ollama's show names where a cloud model runs. "my-coder" stands for a model the user made
            # with `ollama create my-coder` FROM qwen3-coder:480b-cloud.
            if model.endswith("-cloud") or model == "my-coder":
                return self.send(200, {"details": {"family": "qwen3"}, "remote_model": "qwen3-coder:480b",
                                       "remote_host": "https://ollama.com:443", "capabilities": ["completion", "tools"]})
            return self.send(200, {"details": {"family": "qwen3"}, "capabilities": ["completion", "tools"]})
        if path in ("/api/version",):
            return self.send(200, {"version": "0.40.18"})
        if path in ("/api/chat", "/api/generate"):
            key = "message" if path == "/api/chat" else "response"
            piece = (lambda w: {"role": "assistant", "content": w}) if key == "message" else (lambda w: w)
            final = {"model": model, "done": True, "done_reason": "stop", "prompt_eval_count": PROMPT_TOKENS, "eval_count": n_out,
                     key: piece("")}
            if not st:
                return self.send(200, dict(final, **{key: piece(out)}))
            return self.stream("application/x-ndjson", [json.dumps({"model": model, key: piece(w), "done": False}) + "\n" for w in OUT_WORDS]
                               + [json.dumps(final) + "\n"])
        if path in ("/v1/chat/completions", "/api/v0/chat/completions"):
            usage = {"prompt_tokens": PROMPT_TOKENS, "completion_tokens": n_out, "total_tokens": PROMPT_TOKENS + n_out}
            if not st:
                return self.send(200, {"id": "c1", "object": "chat.completion", "model": model,
                                       "choices": [{"index": 0, "message": {"role": "assistant", "content": out}, "finish_reason": "stop"}], "usage": usage})
            ch = [f"data: {json.dumps({'id': 'c1', 'object': 'chat.completion.chunk', 'choices': [{'index': 0, 'delta': {'content': w}}]})}\n\n" for w in OUT_WORDS]
            if (body.get("stream_options") or {}).get("include_usage"):
                ch.append(f"data: {json.dumps({'id': 'c1', 'object': 'chat.completion.chunk', 'choices': [], 'usage': usage})}\n\n")
            return self.stream("text/event-stream", ch + ["data: [DONE]\n\n"])
        if path == "/v1/responses":
            usage = {"input_tokens": PROMPT_TOKENS, "output_tokens": n_out, "total_tokens": PROMPT_TOKENS + n_out}
            resp = {"id": "r1", "object": "response", "model": model, "status": "completed",
                    "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": out}]}], "usage": usage}
            if not st:
                return self.send(200, resp)
            ev = [("response.created", {"type": "response.created", "response": dict(resp, status="in_progress", usage=None, output=[])})]
            ev += [("response.output_text.delta", {"type": "response.output_text.delta", "delta": w}) for w in OUT_WORDS]
            ev += [("response.completed", {"type": "response.completed", "response": resp})]
            return self.stream("text/event-stream", [f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in ev])
        if path == "/v1/messages":
            if not st:
                return self.send(200, {"id": "m1", "type": "message", "role": "assistant", "model": model,
                                       "content": [{"type": "text", "text": out}], "stop_reason": "end_turn",
                                       "usage": {"input_tokens": PROMPT_TOKENS, "output_tokens": n_out}})
            ev = [("message_start", {"type": "message_start", "message": {"id": "m1", "type": "message", "role": "assistant", "model": model, "content": [],
                                                                         "usage": {"input_tokens": PROMPT_TOKENS, "output_tokens": 1}}}),
                  ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})]
            ev += [("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": w}}) for w in OUT_WORDS]
            ev += [("content_block_stop", {"type": "content_block_stop", "index": 0}),
                   ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": n_out}}),
                   ("message_stop", {"type": "message_stop"})]
            return self.stream("text/event-stream", [f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in ev])
        if path in ("/api/pull", "/api/push", "/api/create", "/api/copy", "/api/delete", "/api/v1/models/download", "/api/v1/models/load"):
            return self.send(200, {"status": "success"})  # what the real server would do: download, delete, load
        return self.send(404, {"error": "not found"})

    do_GET = do_POST = do_DELETE = do_HEAD = handle_any


class Ollama(Server):
    kind = "ollama"


class LMStudio(Server):
    kind = "lmstudio"


for port, h in ((11434, Ollama), (1234, LMStudio)):
    s = http.server.ThreadingHTTPServer(("127.0.0.1", port), h)
    threading.Thread(target=s.serve_forever, daemon=True).start()
print("ready", flush=True)
threading.Event().wait()
