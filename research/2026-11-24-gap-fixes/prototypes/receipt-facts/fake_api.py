"""A fake Anthropic Messages API that plays a scripted "model" for the real Claude Code.

Adapted from the experiments rig of 24 November 2026 (fake_anthropic.py), without its probe.
At every request of the main conversation it first takes a checkpoint of the project tree, as
`checkpoint-at-each-turn` would at the boundary, then logs the request whole and answers with the
next scripted step. Env: SCENARIO, OUT, PORT, PROJECT."""
import hashlib, http.server, json, os, threading, time, uuid

OUT = os.environ["OUT"]; PROJECT = os.environ["PROJECT"]
SCEN = json.load(open(os.environ["SCENARIO"]))
REQF = open(f"{OUT}/requests.jsonl", "a"); CPF = open(f"{OUT}/checkpoints.jsonl", "a")
lock = threading.Lock(); state = {"step": 0}


def checkpoint(label):
    """Read the whole tree, as the engine would read a disk checkpoint from outside."""
    files = {}
    for root, dirs, names in os.walk(PROJECT):
        rel_root = os.path.relpath(root, PROJECT)
        # Of .git, only what decides whether checks run and where work went: hooks, config, refs.
        if rel_root == ".git":
            dirs[:] = [d for d in dirs if d in ("hooks", "refs")]
            names = [n for n in names if n in ("config", "HEAD")]
        dirs[:] = [d for d in dirs if d not in ("objects", "logs")]
        for n in names:
            p = os.path.join(root, n)
            try:
                data = open(p, "rb").read()
            except OSError:
                continue
            rel = os.path.relpath(p, PROJECT)
            entry = {"sha": hashlib.sha256(data).hexdigest()}
            if len(data) < 200_000:
                try:
                    entry["text"] = data.decode()
                except UnicodeDecodeError:
                    pass
            files[rel] = entry
    CPF.write(json.dumps({"t": time.monotonic_ns(), "label": label, "files": files}) + "\n"); CPF.flush()


def sse(ev, data):
    return f"event: {ev}\ndata: {json.dumps(data)}\n\n".encode()


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_GET(self):
        self.send_response(404); self.send_header("content-length", "0"); self.end_headers()

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("content-length", 0)))
        try:
            body = json.loads(raw)
        except Exception:
            body = {}
        if "/messages" not in self.path or "count_tokens" in self.path:
            return self.json({"input_tokens": 100} if "count_tokens" in self.path else {})
        main = "Bash" in [t.get("name") for t in body.get("tools", [])]
        with lock:
            if main:
                checkpoint(f"request {state['step']}")
            REQF.write(json.dumps({"t": time.monotonic_ns(), "path": self.path, "main": main, "body": body}) + "\n"); REQF.flush()
            if not main:
                blocks, stop = [{"type": "text", "text": "Side answer"}], "end_turn"
            elif state["step"] < len(SCEN["steps"]):
                st = SCEN["steps"][state["step"]]; state["step"] += 1
                blocks = [{"type": "text", "text": st["text"]}] if st.get("text") else []
                blocks += [{"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": "Bash",
                            "input": {"command": c, "description": "step"}} for c in st["commands"]]
                stop = "tool_use"
            else:
                blocks, stop = [{"type": "text", "text": SCEN["final"]}], "end_turn"
        self.reply(body, blocks, stop)

    def json(self, obj):
        b = json.dumps(obj).encode()
        self.send_response(200); self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def reply(self, body, blocks, stop):
        model = body.get("model", "claude-x"); mid = "msg_" + uuid.uuid4().hex[:24]
        whole = {"id": mid, "type": "message", "role": "assistant", "model": model, "content": blocks,
                 "stop_reason": stop, "stop_sequence": None, "usage": {"input_tokens": 50, "output_tokens": 20}}
        if not body.get("stream"):
            return self.json(whole)
        ev = [sse("message_start", {"type": "message_start", "message": dict(whole, content=[], stop_reason=None,
              usage={"input_tokens": 50, "output_tokens": 1, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0})})]
        for i, b in enumerate(blocks):
            if b["type"] == "text":
                ev.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}))
                ev.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": b["text"]}}))
            else:
                ev.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}))
                ev.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": json.dumps(b["input"])}}))
            ev.append(sse("content_block_stop", {"type": "content_block_stop", "index": i}))
        ev.append(sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": 20}}))
        ev.append(sse("message_stop", {"type": "message_stop"}))
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("connection", "close"); self.end_headers()
        for e in ev:
            self.wfile.write(e); self.wfile.flush()
        self.close_connection = True


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    S(("127.0.0.1", int(os.environ["PORT"])), H).serve_forever()
