"""Copied from #407 (research/2026-11-25-plan-windows/prototype/), itself from #355, with three
additions for research/2026-11-25-spend-report:
- TLS when CERT and KEY are set, so the boundary in front of it sees only encrypted bytes;
- connections kept open between calls, with chunked streams, as a real provider does;
- `visible_chars_per_token` in the scenario streams that many characters of text per output token
  before each tool call (default 0: the short text of #355), and `hidden_share` keeps that share
  of the output tokens out of the stream, as hidden reasoning is.
`commands` in the scenario, {step: command}, has the agent run that command at that step, as an
agent inside the computer would.
`delay_s` in the scenario holds each reply that long, so a run can be cut while it works.

A fake Anthropic Messages API for recording what Silo's boundary would see.

It answers the real Claude Code with a scripted tool loop, and on every reply it returns:
- a `usage` block computed by a small prompt-cache model (reads, 5m and 1h writes, uncached input),
  driven by the `cache_control` markers the real request carries;
- optionally another `model` than the one asked, from a given step on;
- the unified rate-limit headers of a subscription, from a window model in which another device
  of the same account also spends.

A virtual clock lets a scenario put a gap between two steps (for a cache entry to expire)
without waiting. Every exchange is written to trace.jsonl as one line: what the boundary saw.

Run: SCENARIO=scenario.json OUT=out PORT=8411 python3 fake_anthropic_usage.py
"""
import hashlib, http.server, json, os, random, threading, time, uuid, gzip

OUT = os.environ.get("OUT", "out")
os.makedirs(OUT, exist_ok=True)
SCEN = json.load(open(os.environ["SCENARIO"]))
TRACE = open(f"{OUT}/trace.jsonl", "a")
lock = threading.Lock()
rng = random.Random(SCEN.get("seed", 1))

state = {"step": 0, "offset": 0.0}
cache = {}  # prefix hash -> expiry (virtual seconds)
# window model: utilization as a fraction of capacity in "weighted tokens"
win = {"u5": SCEN.get("u5_start", 0.20), "u7": SCEN.get("u7_start", 0.30)}
CAP5 = SCEN.get("cap5_weighted", 4.0e6)   # weighted tokens in a 5-hour window
CAP7 = SCEN.get("cap7_weighted", 4.0e7)   # weighted tokens in a 7-day window
# weights per token kind, relative to uncached input
W = {"input": 1.0, "cache_read": 0.1, "cache_5m": 1.25, "cache_1h": 2.0, "output": 5.0}


def now():
    return time.time() + state["offset"]


def tokens_of(obj):
    return max(1, len(json.dumps(obj, separators=(",", ":"))) // 4)


def breakpoints(body):
    """The request in render order (tools, system, messages), with the cumulative token count at
    each block that carries cache_control, and that marker's ttl."""
    parts = []
    for t in body.get("tools", []) or []:
        parts.append(t)
    sysv = body.get("system")
    if isinstance(sysv, str):
        parts.append({"type": "text", "text": sysv})
    elif isinstance(sysv, list):
        parts.extend(sysv)
    for m in body.get("messages", []):
        c = m.get("content")
        if isinstance(c, str):
            parts.append({"role": m["role"], "text": c})
        else:
            for b in c or []:
                parts.append(dict(b, _role=m["role"]))
    bps, cum, h, positions = [], 0, hashlib.sha256(body.get("model", "").encode()), []
    for p in parts:
        cc = p.get("cache_control")
        q = {k: v for k, v in p.items() if k != "cache_control"}
        cum += tokens_of(q)
        h.update(json.dumps(q, sort_keys=True).encode())
        positions.append((cum, h.hexdigest()[:16]))
        if cc:
            bps.append({"tokens": cum, "hash": h.hexdigest()[:16], "ttl": cc.get("ttl", "5m"), "pos": len(positions) - 1})
    top = body.get("cache_control")
    if top:  # automatic caching: one more breakpoint on the last block
        bps.append({"tokens": cum, "hash": h.hexdigest()[:16], "ttl": top.get("ttl", "5m"), "pos": len(positions) - 1})
    return bps, cum, positions


def usage_for(body, out_tokens):
    bps, total, positions = breakpoints(body)
    t = now()
    read_upto = 0
    for bp in bps:  # longest cached prefix, looking back up to 20 blocks from each breakpoint
        for i in range(bp["pos"], max(-1, bp["pos"] - 20), -1):
            exp = cache.get(positions[i][1])
            if exp and exp > t:
                read_upto = max(read_upto, positions[i][0])
                cache[positions[i][1]] = max(exp, t + (3600 if bp["ttl"] == "1h" else 300))
                break
    w5 = w1 = 0
    last = read_upto
    for bp in bps:
        life = 3600 if bp["ttl"] == "1h" else 300
        if bp["tokens"] > last:
            if bp["ttl"] == "1h":
                w1 += bp["tokens"] - last
            else:
                w5 += bp["tokens"] - last
            last = bp["tokens"]
        cache[bp["hash"]] = max(cache.get(bp["hash"], 0), t + life)  # a read refreshes the timer
    u = {
        "input_tokens": total - last,
        "cache_creation_input_tokens": w5 + w1,
        "cache_read_input_tokens": read_upto,
        "cache_creation": {"ephemeral_5m_input_tokens": w5, "ephemeral_1h_input_tokens": w1},
        "output_tokens": out_tokens,
    }
    return u, [b["ttl"] for b in bps]


def spend_window(u):
    wt = (W["input"] * u["input_tokens"] + W["cache_read"] * u["cache_read_input_tokens"]
          + W["cache_5m"] * u["cache_creation"]["ephemeral_5m_input_tokens"]
          + W["cache_1h"] * u["cache_creation"]["ephemeral_1h_input_tokens"]
          + W["output"] * u["output_tokens"])
    other = SCEN.get("other_device_weighted_per_call", 0) * rng.random() * 2
    win["u5"] += (wt + other) / CAP5
    win["u7"] += (wt + other) / CAP7
    return wt


def unified_headers():
    t = int(now())
    q = SCEN.get("utilization_digits")  # None: full float, 2: two decimals
    f = (lambda x: f"{round(x, q)}") if q is not None else (lambda x: repr(x))
    status = "allowed" if win["u5"] < 0.9 and win["u7"] < 0.75 else "allowed_warning"
    if win["u5"] >= 1 or win["u7"] >= 1:
        status = "rejected"
    claim = "five_hour" if win["u5"] >= win["u7"] else "seven_day"
    h = {
        "anthropic-ratelimit-unified-status": status,
        "anthropic-ratelimit-unified-reset": str(t + 3 * 3600),
        "anthropic-ratelimit-unified-representative-claim": claim,
        "anthropic-ratelimit-unified-fallback-percentage": "0.5",
        "anthropic-ratelimit-unified-5h-utilization": f(win["u5"]),
        "anthropic-ratelimit-unified-5h-reset": str(t + 3 * 3600),
        "anthropic-ratelimit-unified-7d-utilization": f(win["u7"]),
        "anthropic-ratelimit-unified-7d-reset": str(t + int(SCEN.get("reset7_days", 4) * 86400)),
        "anthropic-ratelimit-unified-overage-status": "rejected",
        "anthropic-ratelimit-unified-overage-disabled-reason": "org_level_disabled",
    }
    if SCEN.get("overage_from_step") is not None and state["step"] > SCEN["overage_from_step"]:
        # the plan's window is used up and the call is served from extra usage, billed per token
        h["anthropic-ratelimit-unified-status"] = "rejected"
        h["anthropic-ratelimit-unified-overage-status"] = "allowed"
        h["anthropic-ratelimit-unified-overage-reset"] = str(t + 20 * 86400)
        h.pop("anthropic-ratelimit-unified-overage-disabled-reason", None)
    if win["u7"] >= SCEN.get("surpassed_7d", 9):
        h["anthropic-ratelimit-unified-7d-surpassed-threshold"] = str(SCEN["surpassed_7d"])
    return h


def filler(out_tokens):
    """Text streamed before a tool call: the visible part of the output, as words."""
    cpt = SCEN.get("visible_chars_per_token", 0)
    if not cpt:
        return []
    n = int(out_tokens * (1 - SCEN.get("hidden_share", 0)) * cpt)
    words = ("the boundary reads what leaves and what comes back so a budget can count it " * (n // 70 + 1))
    return [{"type": "text", "text": words[:n]}]


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
        if (self.headers.get("content-encoding") or "") == "gzip":
            raw = gzip.decompress(raw)
        try:
            body = json.loads(raw)
        except Exception:
            body = {}
        if "count_tokens" in self.path:
            return self.reply_json({"input_tokens": tokens_of(body)}, {})
        if "/messages" not in self.path:
            return self.reply_json({}, {})
        with lock:
            tools = [t.get("name") for t in body.get("tools", []) or []]
            main = "Bash" in tools
            asked = body.get("model", "claude-x")
            answered = asked
            blocks, stop = [{"type": "text", "text": "Side answer"}], "end_turn"
            step = None
            if main:
                step = state["step"]; state["step"] += 1
                gap = SCEN.get("gaps", {}).get(str(step))
                if gap:
                    state["offset"] += gap
                sw = SCEN.get("answer_model")
                if sw and step >= sw["from_step"]:
                    answered = sw["model"]
                if step < SCEN.get("steps", 4):
                    blocks = [{"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": "Bash",
                               "input": {"command": SCEN.get("commands", {}).get(str(step)) or f"echo step {step} && seq 1 {SCEN.get('lines', 200)} | head -c {SCEN.get('result_bytes', 4000)}", "description": f"step {step}"}}]
                    stop = "tool_use"
                else:
                    blocks, stop = [{"type": "text", "text": "Done."}], "end_turn"
            out_tokens = SCEN.get("output_tokens", 120) if main else 10
            usage, ttls = usage_for(body, out_tokens)
            wt = spend_window(usage)
            hdrs = unified_headers() if SCEN.get("subscription", True) else {}
            rec = {"t": round(now(), 3), "computer": "silo-main", "path": self.path, "main": main, "step": step,
                   "request": {"model": asked, "stream": bool(body.get("stream")), "n_messages": len(body.get("messages", [])),
                               "n_tools": len(tools), "cache_ttls": ttls, "metadata": body.get("metadata"),
                               "fallbacks": body.get("fallbacks"), "speed": body.get("speed"),
                               "effort": (body.get("output_config") or {}).get("effort"),
                               "session_header": self.headers.get("x-claude-code-session-id"),
                               "anthropic_beta": self.headers.get("anthropic-beta")},
                   "response": {"model": answered, "usage": usage, "stop_reason": stop},
                   "weighted": wt, "headers": hdrs}
            TRACE.write(json.dumps(rec) + "\n"); TRACE.flush()
        time.sleep(SCEN.get("delay_s", 0))
        return self.send_reply(body, answered, blocks, stop, usage, hdrs)

    def reply_json(self, obj, hdrs, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code)
        for k, v in hdrs.items():
            self.send_header(k, v)
        self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def send_reply(self, body, model, blocks, stop, usage, hdrs):
        mid = "msg_" + uuid.uuid4().hex[:24]
        whole = {"id": mid, "type": "message", "role": "assistant", "model": model, "content": blocks,
                 "stop_reason": stop, "stop_sequence": None, "usage": usage}
        if not body.get("stream"):
            return self.reply_json(whole, hdrs)
        self.send_response(200)
        for k, v in hdrs.items():
            self.send_header(k, v)
        self.send_header("content-type", "text/event-stream"); self.send_header("transfer-encoding", "chunked"); self.end_headers()
        start_usage = dict(usage, output_tokens=1)
        ev = [sse("message_start", {"type": "message_start", "message": dict(whole, content=[], stop_reason=None, usage=start_usage)})]
        for i, b in enumerate(blocks):
            if b["type"] == "text":
                ev.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}))
                for k in range(0, max(1, len(b["text"])), 40):   # about one delta per 10 tokens
                    ev.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": b["text"][k:k + 40]}}))
            else:
                ev.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}))
                ev.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": json.dumps(b["input"])}}))
            ev.append(sse("content_block_stop", {"type": "content_block_stop", "index": i}))
        ev.append(sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": usage["output_tokens"]}}))
        ev.append(sse("message_stop", {"type": "message_stop"}))
        for e in ev:
            self.wfile.write(b"%x\r\n%s\r\n" % (len(e), e)); self.wfile.flush()
        self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    srv = S(("127.0.0.1", int(os.environ.get("PORT", "8411"))), H)
    if os.environ.get("CERT"):
        import ssl
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(os.environ["CERT"], os.environ["KEY"])
        srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
    srv.serve_forever()
