"""A scripted model provider: the Anthropic Messages API and the OpenAI Responses API.

It stands in for the model only. The agent programs are the real Claude Code and Codex; what they
run, they run for real. A script is picked by a phrase of what you asked, found in the request.
Each step of a script is one reply: some text, and at most one tool call. The step is the number of
replies already in the conversation, so a retry gets the same reply.

Usage numbers are made to look like a real conversation: the input grows with the history, and
what an earlier request already sent counts as read from the cache.
"""
import json, os, sys, time, uuid, random, http.server, threading

SCRIPTS = json.load(open(os.environ["SCRIPTS"]))
DELAY = [float(x) for x in os.environ.get("MODEL_DELAY", "1.5,3.5").split(",")]
lock = threading.Lock()


def pick(body_text):
    for s in SCRIPTS:
        if s["match"] in body_text:
            return s
    return None


def toks(obj):
    return max(1, len(json.dumps(obj)) // 4)


def wait():
    time.sleep(random.uniform(*DELAY))


# ---------------------------------------------------------------- Anthropic

def a_sse(ev, data):
    return f"event: {ev}\ndata: {json.dumps(data)}\n\n".encode()


def a_reply(model, blocks, stop, usage):
    mid = "msg_" + uuid.uuid4().hex[:24]
    out = [a_sse("message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "stop_sequence": None, "usage": {**usage, "output_tokens": 1}}})]
    for i, b in enumerate(blocks):
        if b["type"] == "text":
            out.append(a_sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}))
            out.append(a_sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": b["text"]}}))
        else:
            out.append(a_sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}))
            out.append(a_sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": json.dumps(b["input"])}}))
        out.append(a_sse("content_block_stop", {"type": "content_block_stop", "index": i}))
    out.append(a_sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": usage["output_tokens"]}}))
    out.append(a_sse("message_stop", {"type": "message_stop"}))
    whole = {"id": mid, "type": "message", "role": "assistant", "model": model, "content": blocks, "stop_reason": stop, "stop_sequence": None, "usage": usage}
    return out, whole


def a_call(c):
    """One step's call, in Claude Code's tools."""
    if "bash" in c:
        inp = {"command": c["bash"], "description": c.get("why", "Run a command")}
        if c.get("timeout"):
            inp["timeout"] = c["timeout"]
        return "Bash", inp
    if "read" in c:
        return "Read", {"file_path": c["read"]}
    if "write" in c:
        return "Write", {"file_path": c["write"], "content": c["content"]}
    raise ValueError(c)


def anthropic(handler, body):
    model = body.get("model", "claude-opus-5-5")
    tools = [t.get("name") for t in body.get("tools", [])]
    msgs = body.get("messages", [])
    hist = toks(body.get("system", "")) + toks(body.get("tools", [])) + toks(msgs)
    last = toks(msgs[-1:]) if msgs else 0
    script = pick(json.dumps(msgs[:1])) if "Bash" in tools else None
    if not script:
        # a side request: a title, a summary, a check. One short answer.
        blocks = [{"type": "text", "text": "Ok."}]
        usage = {"input_tokens": hist, "output_tokens": 4, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
        return handler.send_anthropic(body, model, blocks, "end_turn", usage)
    home = script["home"]
    step = sum(1 for m in msgs if m.get("role") == "assistant")
    steps = script["steps"]
    blocks = []
    if step < len(steps):
        st = steps[step]
        if st.get("text"):
            blocks.append({"type": "text", "text": st["text"]})
        if st.get("call"):
            c = {k: (v.replace("~/", home + "/") if isinstance(v, str) else v) for k, v in st["call"].items()}
            name, inp = a_call(c)
            blocks.append({"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:22], "name": name, "input": inp})
        stop = "tool_use" if st.get("call") else "end_turn"
    else:
        blocks.append({"type": "text", "text": script["final"]})
        stop = "end_turn"
    out = toks(blocks) + random.randint(40, 400)  # thinking is billed as output
    cached = max(0, hist - last) if step else 0
    usage = {"input_tokens": hist - cached, "output_tokens": out, "cache_creation_input_tokens": last if step else hist, "cache_read_input_tokens": cached}
    usage["input_tokens"] = max(1, usage["input_tokens"] - usage["cache_creation_input_tokens"])
    wait()
    return handler.send_anthropic(body, model, blocks, stop, usage)


# ---------------------------------------------------------------- OpenAI Responses

def o_sse(data):
    return f"event: {data['type']}\ndata: {json.dumps(data)}\n\n".encode()


def openai(handler, body):
    inp = body.get("input", [])
    tools = [t.get("name") or t.get("type") for t in body.get("tools", [])]
    script = pick(json.dumps(inp))
    items = []
    if not script:
        items.append(o_msg("Ok."))
    else:
        home = script["home"]
        step = sum(1 for i in inp if isinstance(i, dict) and i.get("type") in ("function_call", "custom_tool_call", "local_shell_call"))
        steps = script["steps"]
        if step < len(steps):
            st = steps[step]
            if st.get("text"):
                items.append(o_msg(st["text"]))
            c = st["call"]
            cmd = c["bash"].replace("~/", home + "/")
            cid = "call_" + uuid.uuid4().hex[:20]
            if "exec_command" in tools:
                args = {"cmd": cmd, "yield_time_ms": c.get("yield_ms", 10000)}
                items.append({"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": cid, "name": "exec_command", "arguments": json.dumps(args), "status": "completed"})
            else:
                args = {"command": ["bash", "-lc", cmd], "timeout_ms": c.get("yield_ms", 10000)}
                items.append({"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": cid, "name": "shell", "arguments": json.dumps(args), "status": "completed"})
        else:
            items.append(o_msg(script["final"]))
        wait()
    hist = toks(body.get("instructions", "")) + toks(body.get("tools", [])) + toks(inp)
    last = toks(inp[-1:]) if inp else 0
    usage = {"input_tokens": hist, "input_tokens_details": {"cached_tokens": max(0, hist - last - 200)}, "output_tokens": toks(items) + random.randint(40, 300), "output_tokens_details": {"reasoning_tokens": 0}}
    usage["total_tokens"] = usage["input_tokens"] + usage["output_tokens"]
    rid = "resp_" + uuid.uuid4().hex[:20]
    evs = [o_sse({"type": "response.created", "sequence_number": 0, "response": {"id": rid, "object": "response", "status": "in_progress", "model": body.get("model"), "output": []}})]
    for k, it in enumerate(items):
        evs.append(o_sse({"type": "response.output_item.added", "output_index": k, "item": it}))
        evs.append(o_sse({"type": "response.output_item.done", "output_index": k, "item": it}))
    evs.append(o_sse({"type": "response.completed", "response": {"id": rid, "object": "response", "status": "completed", "model": body.get("model"), "output": items, "usage": usage}}))
    handler.send_response(200); handler.send_header("content-type", "text/event-stream"); handler.send_header("connection", "close"); handler.end_headers()
    for e in evs:
        handler.wfile.write(e); handler.wfile.flush()
    handler.close_connection = True


def o_msg(t):
    return {"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": t, "annotations": []}]}


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_GET(self):
        b = b'{"models":[],"data":[]}' if "models" in self.path else b"{}"
        self.send_response(200 if "models" in self.path else 404); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def do_HEAD(self):
        self.send_response(200); self.send_header("content-length", "0"); self.end_headers()

    def do_POST(self):
        n = int(self.headers.get("content-length", 0)); raw = self.rfile.read(n)
        if (self.headers.get("content-encoding") or "") == "gzip":
            import gzip; raw = gzip.decompress(raw)
        elif (self.headers.get("content-encoding") or "") == "zstd":
            self.send_response(415); self.send_header("content-length", "0"); self.end_headers(); return
        try:
            body = json.loads(raw)
        except Exception:
            body = {}
        if "count_tokens" in self.path:
            return self.json({"input_tokens": toks(body)})
        path = self.path.split("?", 1)[0]
        if path.rstrip("/").endswith("/messages"):
            return anthropic(self, body)
        if "/responses" in self.path:
            return openai(self, body)
        return self.json({})

    def json(self, o, code=200):
        b = json.dumps(o).encode(); self.send_response(code); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def send_anthropic(self, body, model, blocks, stop, usage):
        evs, whole = a_reply(model, blocks, stop, usage)
        if not body.get("stream"):
            return self.json(whole)
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("connection", "close"); self.end_headers()
        for e in evs:
            self.wfile.write(e); self.wfile.flush()
        self.close_connection = True


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    S(("127.0.0.1", int(os.environ["PORT"])), H).serve_forever()
