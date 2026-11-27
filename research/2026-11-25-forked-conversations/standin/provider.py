"""A stand-in for Anthropic's Messages API and OpenAI's Responses API, listening on one port per
computer, so that every request is recorded with the computer that sent it.

The boundary knows which computer sends each request without asking the program. Here each
computer's programs are pointed at their own port, which plays that knowledge.

It plays a model that, for each new instruction of the user, runs one shell command and then
answers in text. It counts tokens with a stand-in tokenizer (4 bytes a token) and keeps a prompt
cache per credential, as the providers do: a cache is shared by every request sent with the same
key, from any computer. Each call is billed in `ledger.jsonl`; each request is kept in
`requests.jsonl` with its computer, headers and body.

Environment:
  PORTS   "main=8101,fork=8102,..."   one port per computer
  OUT     folder for requests.jsonl and ledger.jsonl
"""
import hashlib, http.server, json, os, threading, time, uuid

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
LOCK = threading.Lock()
REQ = open(f"{OUT}/requests.jsonl", "a"); LEDGER = open(f"{OUT}/ledger.jsonl", "a")
SEQ = [0]

# Prices in dollars per million tokens, stand-ins in the shape of the real tables.
PRICE = {"in": 5.0, "out": 25.0, "cache_read": 0.5, "cache_write": 6.25}

def toks(obj):
    s = obj if isinstance(obj, str) else json.dumps(obj, separators=(",", ":"), sort_keys=True)
    return max(1, (len(s.encode()) + 3) // 4)

# ---- prompt cache: by credential and prefix, shared across computers, as at a provider ----
CACHE = {}  # (credential, prefix hash) -> tokens

def cache_account(cred, pieces, mark_upto):
    """pieces: list of JSON-able items in order. Returns (read, write, total). The longest prefix
    already in the cache is read; everything up to the last breakpoint (mark_upto pieces) is written."""
    h = hashlib.sha256(cred.encode()); cum = 0; read = 0; keys = []
    for i, p in enumerate(pieces):
        h.update(json.dumps(p, sort_keys=True, separators=(",", ":")).encode()); cum += toks(p)
        k = h.hexdigest(); keys.append((k, cum))
        with LOCK:
            if CACHE.get((cred, k)) == cum: read = cum
    write = 0
    if mark_upto:
        k, c = keys[mark_upto - 1]
        if c > read: write = c - read
        with LOCK:
            for kk, cc in keys[:mark_upto]: CACHE[(cred, kk)] = cc
    return read, write, cum

def bill(o):
    o["cost"] = round((o["input"] * PRICE["in"] + o["output"] * PRICE["out"] + o["cache_read"] * PRICE["cache_read"]
                       + o["cache_write"] * PRICE["cache_write"]) / 1e6, 6)
    with LOCK: LEDGER.write(json.dumps(o) + "\n"); LEDGER.flush()
    return o

# ---- the scripted model ----

def anthropic_last_is_tool_result(msgs):
    if not msgs: return False
    c = msgs[-1].get("content")
    return isinstance(c, list) and any(b.get("type") == "tool_result" for b in c)

def has_tool(tools, name):
    return any(t.get("name") == name for t in (tools or []))

class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def computer(self):
        return self.server.computer

    def record(self, body):
        with LOCK:
            SEQ[0] += 1; n = SEQ[0]
            REQ.write(json.dumps({"n": n, "t": time.time(), "computer": self.computer(), "method": self.command,
                                  "path": self.path, "headers": {k.lower(): v for k, v in self.headers.items()},
                                  "body": body}) + "\n"); REQ.flush()
        return n

    def send_json(self, code, obj):
        b = json.dumps(obj).encode()
        self.send_response(code); self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def sse_start(self):
        self.send_response(200); self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache"); self.send_header("connection", "close"); self.end_headers()
        self.close_connection = True

    def sse(self, event, data):
        self.wfile.write(f"event: {event}\ndata: {json.dumps(data)}\n\n".encode()); self.wfile.flush()

    def do_GET(self):
        self.record(None)
        if self.path.startswith("/v1/models"):
            return self.send_json(200, {"data": [], "object": "list"})
        self.send_json(404, {"error": {"message": "not here"}})

    def do_HEAD(self):
        self.send_response(200); self.send_header("content-length", "0"); self.end_headers()

    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(n) if n else b""
        try: body = json.loads(raw or b"{}")
        except Exception: body = {"_raw": raw.decode(errors="replace")}
        rid = self.record(body)
        p = self.path.split("?")[0]
        if p.endswith("/v1/messages/count_tokens"):
            return self.send_json(200, {"input_tokens": toks(body.get("messages", []))})
        if p.endswith("/v1/messages"):
            return self.anthropic(rid, body)
        if p.endswith("/responses"):
            return self.openai_responses(rid, body)
        self.send_json(404, {"error": {"type": "not_found_error", "message": p}})

    # -- Anthropic Messages --
    def anthropic(self, rid, b):
        cred = self.headers.get("x-api-key") or self.headers.get("authorization") or ""
        system = b.get("system") or []
        if isinstance(system, str): system = [{"type": "text", "text": system}]
        pieces = [{"tools": b.get("tools") or []}] + list(system) + list(b.get("messages") or [])
        # The last cache breakpoint, as an index into pieces.
        mark = 0
        for i, p in enumerate(pieces):
            if '"cache_control"' in json.dumps(p): mark = i + 1
        read, write, total = cache_account(cred, pieces, mark)
        inp = total - read - write
        msgs = b.get("messages") or []
        tool = None
        if not anthropic_last_is_tool_result(msgs) and has_tool(b.get("tools"), "Bash"):
            tool = {"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": "Bash",
                    "input": {"command": f"echo step {len(msgs)}", "description": "Print a step"}}
            blocks = [tool]
        else:
            blocks = [{"type": "text", "text": f"Done with turn {len(msgs)}."}]
        out = toks(blocks) + 40
        usage = {"input_tokens": inp, "cache_read_input_tokens": read, "cache_creation_input_tokens": write, "output_tokens": out}
        bill({"rid": rid, "computer": self.computer(), "api": "anthropic", "model": b.get("model"), "input": inp,
              "cache_read": read, "cache_write": write, "output": out})
        mid = "msg_" + uuid.uuid4().hex[:24]
        stop = "tool_use" if tool else "end_turn"
        if not b.get("stream"):
            return self.send_json(200, {"id": mid, "type": "message", "role": "assistant", "model": b.get("model"),
                                        "content": blocks, "stop_reason": stop, "usage": usage})
        self.sse_start()
        self.sse("message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant",
                 "model": b.get("model"), "content": [], "stop_reason": None,
                 "usage": dict(usage, output_tokens=1)}})
        for i, blk in enumerate(blocks):
            if blk["type"] == "text":
                self.sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}})
                self.sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": blk["text"]}})
            else:
                self.sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": dict(blk, input={})})
                self.sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": json.dumps(blk["input"])}})
            self.sse("content_block_stop", {"type": "content_block_stop", "index": i})
        self.sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": out}})
        self.sse("message_stop", {"type": "message_stop"})

    # -- OpenAI Responses --
    def openai_responses(self, rid, b):
        cred = self.headers.get("authorization") or ""
        inp_items = b.get("input") or []
        if isinstance(inp_items, str): inp_items = [{"role": "user", "content": inp_items}]
        pieces = [{"instructions": b.get("instructions"), "tools": b.get("tools") or []}] + list(inp_items)
        # OpenAI caches automatically: every prefix is a candidate, the whole request is written.
        # The cache is the organisation's; `prompt_cache_key` only steers which server a request
        # goes to, so it is left out of the cache's identity here.
        read, write, total = cache_account(cred, pieces, len(pieces))
        inp = total - read
        last = inp_items[-1] if inp_items else {}
        tools = b.get("tools") or []
        fn = next((t for t in tools if t.get("type") == "function" and t.get("name") in ("shell", "exec_command", "shell_command")), None)
        resp_id = "resp_" + uuid.uuid4().hex[:24]
        has_exec = '"name": "exec"' in json.dumps(inp_items[:1]) or '"name":"exec"' in json.dumps(inp_items[:1])
        if last.get("type") not in ("function_call_output", "custom_tool_call_output") and not fn and has_exec:
            # Codex 0.172.0 offers its tools as one JavaScript `exec` custom tool, in an
            # `additional_tools` input item.
            item = {"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20],
                    "name": "exec", "input": f'const r = await tools.exec_command({{cmd: "echo step {len(inp_items)}"}});\ntext(r);', "status": "completed"}
        elif last.get("type") not in ("function_call_output", "custom_tool_call_output") and fn:
            args = {"command": ["bash", "-lc", f"echo step {len(inp_items)}"]} if fn["name"] == "shell" else {"cmd": f"echo step {len(inp_items)}"} if fn["name"] == "exec_command" else {"command": f"echo step {len(inp_items)}"}
            item = {"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20],
                    "name": fn["name"], "arguments": json.dumps(args), "status": "completed"}
        else:
            item = {"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "role": "assistant", "status": "completed",
                    "content": [{"type": "output_text", "text": f"Done with turn {len(inp_items)}.", "annotations": []}]}
        out = toks(item) + 40
        usage = {"input_tokens": inp + read, "input_tokens_details": {"cached_tokens": read},
                 "output_tokens": out, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": inp + read + out}
        # OpenAI bills cached input at the cache price and the rest as input; no write price.
        bill({"rid": rid, "computer": self.computer(), "api": "responses", "model": b.get("model"), "input": inp,
              "cache_read": read, "cache_write": 0, "output": out})
        resp = {"id": resp_id, "object": "response", "created_at": int(time.time()), "status": "completed",
                "model": b.get("model"), "output": [item], "usage": usage}
        if not b.get("stream"):
            return self.send_json(200, resp)
        self.sse_start()
        self.sse("response.created", {"type": "response.created", "response": dict(resp, status="in_progress", output=[], usage=None)})
        self.sse("response.output_item.added", {"type": "response.output_item.added", "output_index": 0, "item": item})
        if item["type"] == "message":
            self.sse("response.output_text.delta", {"type": "response.output_text.delta", "item_id": item["id"], "output_index": 0, "content_index": 0, "delta": item["content"][0]["text"]})
        self.sse("response.output_item.done", {"type": "response.output_item.done", "output_index": 0, "item": item})
        self.sse("response.completed", {"type": "response.completed", "response": resp})

class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True

def main():
    servers = []
    for part in os.environ["PORTS"].split(","):
        name, port = part.split("=")
        s = Server(("127.0.0.1", int(port)), H); s.computer = name; servers.append(s)
        threading.Thread(target=s.serve_forever, daemon=True).start()
    print("listening", os.environ["PORTS"], flush=True)
    threading.Event().wait()

if __name__ == "__main__":
    main()
