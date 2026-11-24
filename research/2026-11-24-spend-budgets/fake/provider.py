"""A stand-in for Anthropic, OpenAI and Google's model APIs, for measuring budgets at the boundary.

It plays a model that works through a long task: a number of tool calls in a row, a burst of
parallel subagents for Claude Code, one large file read. It counts tokens the way a provider
does (input, cache reads and writes with their lifetimes, hidden reasoning, output) with its own
stand-in tokenizer, reports them in each provider's own format, and writes what it bills for each
call to ledger.jsonl: what the boundary's count is checked against. A call the client drops
before the end is billed for what was generated up to that moment.

Run inside the experiment's network namespace only. Environment:
  PORT, TLS_CERT, TLS_KEY   where to listen, and the certificate for the providers' hosts
  OUT                       folder for ledger.jsonl and log.txt
  SCENARIO                  JSON file, see run/scenarios/
"""
import hashlib, json, math, os, random, socket, ssl, threading, time, uuid, http.server, gzip, zlib

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
SC = json.load(open(os.environ["SCENARIO"]))
RNG = random.Random(SC.get("seed", 1))
LOCK = threading.RLock()
LEDGER = open(f"{OUT}/ledger.jsonl", "a"); LOG = open(f"{OUT}/fake.log", "a")
T0 = time.time()

def log(s):
    with LOCK: LOG.write(f"{time.time()-T0:8.2f} {s}\n"); LOG.flush()

def ledger(o):
    with LOCK: LEDGER.write(json.dumps(o) + "\n"); LEDGER.flush()

def rand(lo_hi):
    with LOCK: return RNG.randint(*lo_hi)

def chance(p):
    with LOCK: return RNG.random() < p

# ---- a stand-in tokenizer: different text costs different tokens per byte ----

def toks(obj, ratio):
    s = obj if isinstance(obj, str) else json.dumps(obj, separators=(",", ":"))
    return max(1, math.ceil(len(s.encode()) / ratio))

def text_toks(s):
    return toks(s, 3.6)

# ---- prompt caching, one store for all providers ----

CACHE = {}  # prefix hash -> (tokens, expires_at)

def cache_lookup(pieces):
    """pieces: list of (hash_material, tokens, ttl or None). A ttl marks a cache breakpoint (Anthropic)
    or every piece is a candidate (automatic caching). Returns (read, write_short, write_long, total)."""
    h = hashlib.sha256(); cum = 0; best_read = 0; last_mark = 0; last_ttl = None; now = time.time()
    marks = []
    for mat, n, ttl in pieces:
        h.update(mat.encode() if isinstance(mat, str) else mat); cum += n
        key = h.hexdigest()
        if ttl:
            marks.append((key, cum, ttl))
        with LOCK:
            hit = CACHE.get(key)
        if hit and hit[1] > now and hit[0] == cum:
            best_read = cum
    write_short = write_long = 0
    if marks:
        key, cum_mark, ttl = marks[-1]
        if cum_mark > best_read:
            if ttl == "1h": write_long = cum_mark - best_read
            else: write_short = cum_mark - best_read
        with LOCK:
            for k, c, t in marks:
                CACHE[k] = (c, now + (3600 if t == "1h" else 300))
    return best_read, write_short, write_long, cum

# ---- plan windows, for a subscription sign-in ----

PLANS = {}

def plan_headers(account, cost):
    p = SC.get("plan")
    if not p: return []
    with LOCK:
        st = PLANS.setdefault(account, {"5h": p.get("start_5h", 0.1), "7d": p.get("start_7d", 0.3), "t": time.time()})
        now = time.time(); other = (now - st["t"]) * p.get("other_device_per_s", 0); st["t"] = now
        self5, self7 = cost * p.get("per_dollar_5h", 0.02), cost * p.get("per_dollar_7d", 0.004)
        st["5h"] = min(1.0, st["5h"] + self5 + other)
        st["7d"] = min(1.0, st["7d"] + self7 + other / 5)
        u5, u7 = st["5h"], st["7d"]
        st.setdefault("self5", 0); st["self5"] += self5; st.setdefault("other5", 0); st["other5"] += other
        ledger({"plan": account[-8:], "self_5h": st["self5"], "other_5h": st["other5"], "u5h": u5, "u7d": u7, "t": now - T0})
    reset = int(time.time()) + 3600
    return [("anthropic-ratelimit-unified-status", "allowed"),
            ("anthropic-ratelimit-unified-5h-utilization", f"{u5:.6f}"), ("anthropic-ratelimit-unified-5h-reset", str(reset)),
            ("anthropic-ratelimit-unified-7d-utilization", f"{u7:.6f}"), ("anthropic-ratelimit-unified-7d-reset", str(reset + 86400 * 3)),
            ("anthropic-ratelimit-unified-representative-claim", "five_hour")]

def api_cost(c):
    """A rough price, only to move the fake plan windows."""
    return (c.get("input", 0) + c.get("cache_write_short", 0) * 1.25 + c.get("cache_write_long", 0) * 2 + c.get("cache_read", 0) * 0.05) * 4e-6 + c.get("output", 0) * 2e-5

# ---- the model's task ----

def plan_step(conv_kind, step, tool_names):
    """What the model does at this step of its conversation: ("tools", [calls]) or ("text", words)."""
    if conv_kind == "side":
        return ("text", "A short title")
    if conv_kind == "sub":
        if step < SC.get("sub_steps", 4):
            return ("tools", [shell(tool_names, f"echo sub step {step} >> sub.log")])
        return ("text", "Subtask done. " * 20)
    n = SC.get("main_steps", 10)
    if step == SC.get("spawn_at", -1) and SC.get("subagents", 0) and ({"Task", "Agent"} & tool_names):
        name = "Task" if "Task" in tool_names else "Agent"
        return ("tools", [{"name": name, "input": {"description": f"part {k}", "prompt": f"SUBAGENT {k}: do part {k} of the task with the shell.", "subagent_type": "general-purpose"}} for k in range(SC["subagents"])])
    if step == SC.get("big_read_at", -1):
        return ("tools", [shell(tool_names, "cat big.txt | head -c %d" % SC.get("big_bytes", 400000))])
    if step < n:
        return ("tools", [shell(tool_names, f"echo step {step} >> work.log")])
    return ("text", "All done. " * 30)

def shell(tool_names, cmd):
    if "exec" in tool_names and "exec_command" not in tool_names:
        # Codex's code mode: one custom tool that runs JavaScript calling the nested tools.
        return {"name": "exec", "custom": True, "input": "const r = await tools.exec_command({cmd: %s});\ntext(JSON.stringify(r));" % json.dumps(cmd)}
    if "Bash" in tool_names: return {"name": "Bash", "input": {"command": cmd, "description": "work"}}
    if "exec_command" in tool_names: return {"name": "exec_command", "input": {"cmd": cmd}}
    if "shell" in tool_names: return {"name": "shell", "input": {"command": ["bash", "-lc", cmd]}}
    if "run_shell_command" in tool_names: return {"name": "run_shell_command", "input": {"command": cmd}}
    return {"name": sorted(tool_names)[0] if tool_names else "shell", "input": {"command": cmd}}

def think_tokens():
    return rand(SC.get("think", [500, 4000]))

# ---- HTTP ----

class Dropped(Exception):
    pass

class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def body(self):
        n = int(self.headers.get("content-length") or 0)
        if self.headers.get("transfer-encoding", "").lower() == "chunked":
            raw = b""
            while True:
                size = int(self.rfile.readline().split(b";")[0], 16)
                if size == 0: self.rfile.readline(); break
                raw += self.rfile.read(size); self.rfile.readline()
        else:
            raw = self.rfile.read(n)
        enc = (self.headers.get("content-encoding") or "").lower()
        if enc == "gzip": raw = gzip.decompress(raw)
        elif enc == "zstd":
            import zstandard; raw = zstandard.ZstdDecompressor().decompress(raw, max_output_size=1 << 28)
        elif enc == "deflate": raw = zlib.decompress(raw)
        try: return json.loads(raw)
        except Exception: return {}

    def credential(self):
        a = self.headers.get("x-api-key") or self.headers.get("x-goog-api-key") or self.headers.get("authorization", "")
        return a[7:] if a.lower().startswith("bearer ") else a

    def send_json(self, o, code=200, extra=()):
        b = json.dumps(o).encode()
        self.send_response(code); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b)))
        for k, v in extra: self.send_header(k, v)
        self.end_headers(); self.wfile.write(b)

    def do_GET(self):
        if (self.headers.get("upgrade") or "").lower() == "websocket":
            log(f"GET {self.path} websocket refused, the client falls back to HTTP")
            return self.send_json({"error": {"message": "no websocket here"}}, 404)
        if self.path.startswith("/v1/models"):
            return self.send_json({"data": [], "object": "list"})
        return self.send_json({"error": {"type": "not_found_error", "message": "not here"}}, 404)

    def do_HEAD(self):
        self.send_response(200); self.send_header("content-length", "0"); self.end_headers()

    def do_POST(self):
        host = (self.headers.get("host") or "").split(":")[0]
        body = self.body(); path = self.path
        if os.environ.get("DUMP"):
            with LOCK:
                n = len(os.listdir(OUT)); open(f"{OUT}/req-{n:04d}.json", "w").write(json.dumps({"path": path, "headers": dict(self.headers), "body": body}))
        try:
            if path.startswith("/v1/messages") and "count_tokens" not in path:
                return self.anthropic(body)
            if path.endswith("/responses"):
                return self.openai_responses(body)
            if path.endswith("/chat/completions"):
                return self.chat(body)
            if ":streamGenerateContent" in path or ":generateContent" in path:
                return self.gemini(body, path)
        except (BrokenPipeError, ConnectionResetError, Dropped, ssl.SSLError, OSError) as e:
            log(f"client dropped: {type(e).__name__}")
            return
        log(f"POST {host}{path} not a model call")
        return self.send_json({"error": {"type": "not_found_error", "message": "not here"}}, 404)

    # The stream is written slowly: hidden reasoning at the scenario's speed, then visible output.
    def pace(self, tokens):
        tps = SC.get("tps", 300)
        time.sleep(tokens / tps)

    def write(self, b):
        try:
            self.wfile.write(b); self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError, OSError):
            raise Dropped()

    def bill(self, rec, gen_out, completed):
        rec = dict(rec); rec["completed"] = completed
        if not completed:
            rec["billed_output"] = gen_out
        ledger(rec)

    # ---- Anthropic Messages ----
    def anthropic(self, body):
        model = body.get("model", "")
        tools = body.get("tools") or []
        names = {t.get("name") for t in tools if isinstance(t, dict)}
        msgs = body.get("messages") or []
        first = msgs[0] if msgs else {}
        first_text = json.dumps(first.get("content"))[:4000]
        kind = "sub" if "SUBAGENT" in first_text else ("main" if tools else "side")
        step = sum(1 for m in msgs if m.get("role") == "assistant")
        # input pieces: tools, system, then each message's blocks, with cache markers.
        pieces = []
        for t in tools:
            cc = t.get("cache_control") if isinstance(t, dict) else None
            pieces.append((json.dumps(t, sort_keys=True), toks(t, 3.0), (cc or {}).get("ttl", "5m") if cc else None))
        sysv = body.get("system")
        for b in (sysv if isinstance(sysv, list) else [{"type": "text", "text": sysv}] if sysv else []):
            if b.get("text", "").startswith("x-anthropic-billing-header"):
                continue  # Claude Code's per-request line; the stand-in, like the API, keeps it out of the cache key
            cc = b.get("cache_control")
            pieces.append((json.dumps(b.get("text", ""), sort_keys=True), text_toks(b.get("text", "")), (cc or {}).get("ttl", "5m") if cc else None))
        for m in msgs:
            content = m.get("content")
            blocks = content if isinstance(content, list) else [{"type": "text", "text": content or ""}]
            for b in blocks:
                cc = b.get("cache_control") if isinstance(b, dict) else None
                bb = {k: v for k, v in b.items() if k != "cache_control"} if isinstance(b, dict) else b
                ratio = 3.0 if isinstance(b, dict) and b.get("type") == "tool_result" else 3.8
                pieces.append((m.get("role", "") + json.dumps(bb, sort_keys=True), toks(bb, ratio), (cc or {}).get("ttl", "5m") if cc else None))
        read, ws_, wl, total = cache_lookup(pieces)
        uncached = total - read - ws_ - wl
        what, payload = plan_step(kind, step, names)
        thinking = think_tokens() if kind != "side" else 0
        blocks = []
        if thinking: blocks.append({"type": "thinking"})
        if what == "tools":
            blocks.append({"type": "text", "text": "Working on it."})
            for c in payload: blocks.append({"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": c["name"], "input": c["input"]})
            stop = "tool_use"
        else:
            blocks.append({"type": "text", "text": payload}); stop = "end_turn"
        visible = sum(text_toks(b["text"]) for b in blocks if b["type"] == "text") + sum(toks(b["input"], 3.0) + 5 for b in blocks if b["type"] == "tool_use")
        out_total = thinking + visible
        usage_start = {"input_tokens": uncached, "cache_creation_input_tokens": ws_ + wl, "cache_creation": {"ephemeral_5m_input_tokens": ws_, "ephemeral_1h_input_tokens": wl}, "cache_read_input_tokens": read, "output_tokens": 1}
        final_usage = dict(usage_start); final_usage["output_tokens"] = out_total
        speed = "fast" if body.get("speed") == "fast" else "standard"
        if speed == "fast": final_usage["speed"] = "fast"
        rec = {"provider": "anthropic", "kind": kind, "step": step, "model": model, "input": uncached, "cache_read": read, "cache_write_short": ws_, "cache_write_long": wl, "output": out_total, "reasoning": thinking, "speed": speed, "t": time.time() - T0}
        # A server-side fallback now and then: the first attempt is billed too, listed only in iterations.
        if kind != "side" and chance(SC.get("fallback_rate", 0)):
            fb_model = SC.get("fallback_model", "claude-sonnet-5-5"); first_out = rand([100, 300])
            final_usage["iterations"] = [
                {"type": "message", "model": model, "input_tokens": uncached, "cache_read_input_tokens": read, "cache_creation_input_tokens": ws_ + wl, "cache_creation": {"ephemeral_5m_input_tokens": ws_, "ephemeral_1h_input_tokens": wl}, "output_tokens": first_out},
                {"type": "fallback_message", "model": fb_model, "input_tokens": total, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0, "output_tokens": out_total}]
            rec["iterations"] = [{"model": model, "input": uncached, "cache_read": read, "cache_write_short": ws_, "cache_write_long": wl, "output": first_out}, {"model": fb_model, "input": total, "output": out_total}]
        acct = self.credential()
        extra = plan_headers(acct, api_cost(rec)) if acct.startswith("sk-ant-oat") else []
        mid = "msg_" + uuid.uuid4().hex[:20]
        if not body.get("stream"):
            time.sleep((thinking + visible) / SC.get("tps", 300))
            content = []
            for b in blocks:
                if b["type"] == "thinking": content.append({"type": "thinking", "thinking": "", "signature": "sig"})
                elif b["type"] == "text": content.append({"type": "text", "text": b["text"]})
                else: content.append(b)
            self.bill(rec, out_total, True)
            return self.send_json({"id": mid, "type": "message", "role": "assistant", "model": model, "content": content, "stop_reason": stop, "usage": final_usage}, extra=extra)
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("cache-control", "no-cache"); self.send_header("connection", "close")
        for k, v in extra: self.send_header(k, v)
        self.end_headers()
        gen = 0
        def ev(name, data): self.write(f"event: {name}\ndata: {json.dumps(data)}\n\n".encode())
        try:
            ev("message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "usage": usage_start}})
            for i, b in enumerate(blocks):
                if b["type"] == "thinking":
                    ev("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "thinking", "thinking": "", "signature": ""}})
                    left = thinking
                    while left > 0:
                        step_t = min(left, 400); self.pace(step_t); gen += step_t; left -= step_t
                        ev("ping", {"type": "ping"})
                    ev("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "signature_delta", "signature": "sig"}})
                elif b["type"] == "text":
                    ev("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}})
                    words = b["text"].split(" ")
                    for k in range(0, len(words), 8):
                        part = " ".join(words[k:k + 8]) + " "; self.pace(text_toks(part)); gen += text_toks(part)
                        ev("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": part}})
                else:
                    ev("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}})
                    js = json.dumps(b["input"]); self.pace(toks(js, 3.0)); gen += toks(js, 3.0) + 5
                    ev("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": js}})
                ev("content_block_stop", {"type": "content_block_stop", "index": i})
            md = {k: v for k, v in final_usage.items()}
            ev("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": md})
            ev("message_stop", {"type": "message_stop"})
        except Dropped:
            self.bill(rec, gen, False); raise
        self.bill(rec, out_total, True)

    # ---- OpenAI Responses ----
    def openai_responses(self, body):
        model = body.get("model", "")
        tools = body.get("tools") or []
        names = {t.get("name") or t.get("type") for t in tools if isinstance(t, dict)}
        inp = body.get("input") or []
        if isinstance(inp, str): inp = [{"type": "message", "role": "user", "content": inp}]
        for it in inp:
            if isinstance(it, dict) and it.get("type") == "additional_tools":
                for ns in it.get("tools") or []:
                    names |= {x.get("name") for x in ns.get("tools") or [] if isinstance(x, dict)}
        tools = tools or [1] if names else tools
        kind = "main" if tools else "side"
        step = sum(1 for i in inp if isinstance(i, dict) and i.get("type") in ("function_call", "custom_tool_call", "local_shell_call"))
        pieces = [(json.dumps(tools, sort_keys=True), toks(tools, 3.0) if tools else 0, None), (json.dumps(body.get("instructions", "")), text_toks(body.get("instructions") or " "), None)]
        for it in inp:
            ratio = 3.0 if isinstance(it, dict) and it.get("type", "").endswith("output") else 3.8
            pieces.append((json.dumps(it, sort_keys=True), toks(it, ratio), None))
        # Automatic caching: any earlier prefix, rounded down to 128 tokens, from 1024 up.
        read = self.auto_cache(pieces)
        total = sum(p[1] for p in pieces)
        write = max(0, total - read) if SC.get("openai_cache_writes", True) else 0
        uncached = total - read - write
        what, payload = plan_step(kind, step, names)
        thinking = think_tokens() if kind != "side" else 0
        items = []
        if thinking: items.append({"type": "reasoning", "id": "rs_" + uuid.uuid4().hex[:20], "summary": [], "encrypted_content": "x" * 64})
        if what == "tools":
            for c in payload:
                if c.get("custom"):
                    items.append({"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20], "name": c["name"], "input": c["input"], "status": "completed"})
                else:
                    items.append({"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20], "name": c["name"], "arguments": json.dumps(c["input"]), "status": "completed"})
        else:
            items.append({"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": payload, "annotations": []}]})
        visible = sum(toks(i["arguments"], 3.0) + 5 for i in items if i["type"] == "function_call") + sum(toks(i["input"], 3.0) + 5 for i in items if i["type"] == "custom_tool_call") + sum(text_toks(i["content"][0]["text"]) for i in items if i["type"] == "message")
        out_total = thinking + visible
        usage = {"input_tokens": total, "input_tokens_details": {"cached_tokens": read, "cache_write_tokens": write, "cache_write_tokens_details": {"ttl_30m_tokens": write, "ttl_12h_tokens": 0}}, "output_tokens": out_total, "output_tokens_details": {"reasoning_tokens": thinking}, "total_tokens": total + out_total}
        rec = {"provider": "openai", "format": "responses", "kind": kind, "step": step, "model": model, "input": uncached, "cache_read": read, "cache_write_short": write, "cache_write_long": 0, "output": out_total, "reasoning": thinking, "t": time.time() - T0}
        rid = "resp_" + uuid.uuid4().hex[:20]
        resp = {"id": rid, "object": "response", "model": model, "status": "in_progress", "output": [], "service_tier": "default"}
        if not body.get("stream"):
            time.sleep(out_total / SC.get("tps", 300)); self.bill(rec, out_total, True)
            done = dict(resp, status="completed", output=items, usage=usage); return self.send_json(done)
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("connection", "close"); self.end_headers()
        seq = [0]
        def ev(data):
            data["sequence_number"] = seq[0]; seq[0] += 1
            self.write(f"event: {data['type']}\ndata: {json.dumps(data)}\n\n".encode())
        gen = 0
        try:
            ev({"type": "response.created", "response": resp})
            for k, it in enumerate(items):
                if it["type"] == "reasoning":
                    ev({"type": "response.output_item.added", "output_index": k, "item": {"type": "reasoning", "id": it["id"], "summary": []}})
                    left = thinking
                    while left > 0:
                        st = min(left, 400); self.pace(st); gen += st; left -= st
                        self.write(b": keep-alive\n\n")
                    ev({"type": "response.output_item.done", "output_index": k, "item": it})
                elif it["type"] == "function_call":
                    ev({"type": "response.output_item.added", "output_index": k, "item": dict(it, arguments="", status="in_progress")})
                    self.pace(toks(it["arguments"], 3.0)); gen += toks(it["arguments"], 3.0) + 5
                    ev({"type": "response.function_call_arguments.delta", "output_index": k, "item_id": it["id"], "delta": it["arguments"]})
                    ev({"type": "response.function_call_arguments.done", "output_index": k, "item_id": it["id"], "arguments": it["arguments"]})
                    ev({"type": "response.output_item.done", "output_index": k, "item": it})
                elif it["type"] == "custom_tool_call":
                    ev({"type": "response.output_item.added", "output_index": k, "item": dict(it, input="", status="in_progress")})
                    self.pace(toks(it["input"], 3.0)); gen += toks(it["input"], 3.0) + 5
                    ev({"type": "response.custom_tool_call_input.delta", "output_index": k, "item_id": it["id"], "delta": it["input"]})
                    ev({"type": "response.custom_tool_call_input.done", "output_index": k, "item_id": it["id"], "input": it["input"]})
                    ev({"type": "response.output_item.done", "output_index": k, "item": it})
                else:
                    ev({"type": "response.output_item.added", "output_index": k, "item": dict(it, content=[], status="in_progress")})
                    words = it["content"][0]["text"].split(" ")
                    for w in range(0, len(words), 8):
                        part = " ".join(words[w:w + 8]) + " "; self.pace(text_toks(part)); gen += text_toks(part)
                        ev({"type": "response.output_text.delta", "output_index": k, "item_id": it["id"], "content_index": 0, "delta": part})
                    ev({"type": "response.output_item.done", "output_index": k, "item": it})
            ev({"type": "response.completed", "response": dict(resp, status="completed", output=items, usage=usage)})
        except Dropped:
            self.bill(rec, gen, False); raise
        self.bill(rec, out_total, True)

    def auto_cache(self, pieces):
        h = hashlib.sha256(); cum = 0; best = 0; now = time.time(); keys = []
        for mat, n, _ in pieces:
            h.update(mat.encode()); cum += n; k = "auto" + h.hexdigest(); keys.append((k, cum))
            with LOCK: hit = CACHE.get(k)
            if hit and hit[1] > now and hit[0] == cum: best = cum
        with LOCK:
            for k, c in keys: CACHE[k] = (c, now + 1800)
        best = (best // 128) * 128
        return best if best >= 1024 else 0

    # ---- OpenAI Chat Completions ----
    def chat(self, body):
        model = body.get("model", ""); msgs = body.get("messages") or []
        pieces = [(json.dumps(m, sort_keys=True), toks(m, 3.8), None) for m in msgs]
        read = self.auto_cache(pieces); total = sum(p[1] for p in pieces)
        thinking = think_tokens(); text = "Here is the answer. " * rand([5, 40])
        out_total = thinking + text_toks(text)
        rec = {"provider": "openai", "format": "chat", "kind": "main", "step": len(msgs), "model": model, "input": total - read, "cache_read": read, "cache_write_short": 0, "cache_write_long": 0, "output": out_total, "reasoning": thinking, "t": time.time() - T0, "usage_sent": (not body.get("stream")) or bool((body.get("stream_options") or {}).get("include_usage"))}
        usage = {"prompt_tokens": total, "prompt_tokens_details": {"cached_tokens": read}, "completion_tokens": out_total, "completion_tokens_details": {"reasoning_tokens": thinking}, "total_tokens": total + out_total}
        cid = "chatcmpl-" + uuid.uuid4().hex[:20]
        if not body.get("stream"):
            time.sleep(out_total / SC.get("tps", 300)); self.bill(rec, out_total, True)
            return self.send_json({"id": cid, "object": "chat.completion", "model": model, "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}], "usage": usage})
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("connection", "close"); self.end_headers()
        gen = 0
        try:
            self.pace(thinking); gen += thinking
            words = text.split(" ")
            for w in range(0, len(words), 8):
                part = " ".join(words[w:w + 8]) + " "; self.pace(text_toks(part)); gen += text_toks(part)
                self.write(f"data: {json.dumps({'id': cid, 'object': 'chat.completion.chunk', 'model': model, 'choices': [{'index': 0, 'delta': {'content': part}, 'finish_reason': None}], 'usage': None})}\n\n".encode())
            self.write(f"data: {json.dumps({'id': cid, 'object': 'chat.completion.chunk', 'model': model, 'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}], 'usage': None})}\n\n".encode())
            if (body.get("stream_options") or {}).get("include_usage"):
                self.write(f"data: {json.dumps({'id': cid, 'object': 'chat.completion.chunk', 'model': model, 'choices': [], 'usage': usage})}\n\n".encode())
            self.write(b"data: [DONE]\n\n")
        except Dropped:
            self.bill(rec, gen, False); raise
        self.bill(rec, out_total, True)

    # ---- Google Gemini ----
    def gemini(self, body, path):
        model = path.split("/models/")[1].split(":")[0]
        contents = body.get("contents") or []
        decls = [d for t in (body.get("tools") or []) for d in (t.get("functionDeclarations") or t.get("function_declarations") or [])]
        names = {d.get("name") for d in decls}
        kind = "main" if decls else "side"
        step = sum(1 for c in contents if c.get("role") == "model")
        pieces = [(json.dumps(body.get("tools"), sort_keys=True), toks(body.get("tools") or [], 3.0), None), (json.dumps(body.get("systemInstruction"), sort_keys=True), toks(body.get("systemInstruction") or "", 3.6), None)]
        pieces += [(json.dumps(c, sort_keys=True), toks(c, 3.6), None) for c in contents]
        read = self.auto_cache(pieces); total = sum(p[1] for p in pieces)
        what, payload = plan_step(kind, step, names)
        thinking = think_tokens() if kind != "side" else 0
        if what == "tools":
            parts = [{"functionCall": {"name": c["name"], "args": c["input"]}} for c in payload]
            visible = sum(toks(c["input"], 3.0) + 5 for c in payload)
        else:
            parts = [{"text": payload}]; visible = text_toks(payload)
        out_total = thinking + visible
        rec = {"provider": "google", "format": "gemini", "kind": kind, "step": step, "model": model, "input": total - read, "cache_read": read, "cache_write_short": 0, "cache_write_long": 0, "output": out_total, "reasoning": thinking, "t": time.time() - T0}
        um = {"promptTokenCount": total, "cachedContentTokenCount": read, "candidatesTokenCount": visible, "thoughtsTokenCount": thinking, "totalTokenCount": total + out_total}
        chunk_last = {"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}], "usageMetadata": um, "modelVersion": model}
        if ":generateContent" in path:
            time.sleep(out_total / SC.get("tps", 300)); self.bill(rec, out_total, True); return self.send_json(chunk_last)
        sse = "alt=sse" in path
        self.send_response(200); self.send_header("content-type", "text/event-stream" if sse else "application/json"); self.send_header("connection", "close"); self.end_headers()
        gen = 0
        try:
            if not sse: self.write(b"[")
            self.pace(thinking); gen += thinking
            first = {"candidates": [{"content": {"role": "model", "parts": []}, "index": 0}], "usageMetadata": {"promptTokenCount": total, "cachedContentTokenCount": read, "totalTokenCount": total}, "modelVersion": model}
            self.write((f"data: {json.dumps(first)}\n\n" if sse else json.dumps(first) + ",").encode())
            self.pace(visible); gen += visible
            self.write((f"data: {json.dumps(chunk_last)}\n\n" if sse else json.dumps(chunk_last) + "]").encode())
        except Dropped:
            self.bill(rec, gen, False); raise
        self.bill(rec, out_total, True)


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 128
    def get_request(self):
        sock, addr = self.socket.accept()
        return CTX.wrap_socket(sock, server_side=True, do_handshake_on_connect=False), addr
    def finish_request(self, request, client_address):
        try: request.do_handshake()
        except Exception as e:
            log(f"handshake failed {e}"); return
        super().finish_request(request, client_address)

CTX = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); CTX.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"]); CTX.set_alpn_protocols(["http/1.1"])

if __name__ == "__main__":
    srv = S((os.environ.get("BIND", "127.0.0.1"), int(os.environ.get("PORT", "8443"))), H)
    log("listening"); srv.serve_forever()
