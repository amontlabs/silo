"""A fake Anthropic Messages API. The main conversation (a request whose tools include Bash) gets a
scripted reply with tool calls; when the reply carries a call to deny, the probe is armed before
the reply is sent and disarmed when a request carries a result for that call. Every request is
logged whole to requests.jsonl."""
import json, os, sys, time, uuid, http.server, threading
sys.path.insert(0, os.path.dirname(__file__))
from probe import Probe, FifoProbe

OUT = os.environ.get("OUT", "out")
os.makedirs(OUT, exist_ok=True)
SCEN = json.load(open(os.environ["SCENARIO"]))
LOGF = open(f"{OUT}/log.txt", "a")
REQF = open(f"{OUT}/requests.jsonl", "a")
lock = threading.Lock()


def log(s):
    with lock:
        LOGF.write(f"{time.time():.3f} {s}\n")
        LOGF.flush()


probe = (FifoProbe(os.environ["PROBE_FIFO"], SCEN.get("allow", []), log) if os.environ.get("PROBE_FIFO") else Probe(SCEN.get("allow", []), log)) if SCEN.get("probe", True) else None
if probe:
    probe.root_pid = None
state = {"step": 0, "pending": set()}


def sse(ev, data):
    return f"event: {ev}\ndata: {json.dumps(data)}\n\n".encode()


def reply_events(model, blocks, stop):
    mid = "msg_" + uuid.uuid4().hex[:24]
    out = [sse("message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "stop_sequence": None, "usage": {"input_tokens": 50, "output_tokens": 1, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}}})]
    nd = int(os.environ.get("TEXT_DELTAS", "0"))
    if nd and not any(b["type"] == "text" for b in blocks):
        blocks = [{"type": "text", "text": "word " * nd}] + blocks
    for i, b in enumerate(blocks):
        if b["type"] == "text":
            out.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}))
            parts = [b["text"]] if not nd else [b["text"][k*5:(k+1)*5] for k in range(max(1, len(b["text"])//5))]
            for p in parts:
                out.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": p}}))
        else:
            out.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}))
            js = json.dumps(b["input"]); step = int(os.environ.get("JSON_STEP", "0")) or len(js) or 1
            for k in range(0, len(js), step):
                out.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": js[k:k+step]}}))
        out.append(sse("content_block_stop", {"type": "content_block_stop", "index": i}))
    out.append(sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": 20}}))
    out.append(sse("message_stop", {"type": "message_stop"}))
    return out, {"id": mid, "type": "message", "role": "assistant", "model": model, "content": blocks, "stop_reason": stop, "stop_sequence": None, "usage": {"input_tokens": 50, "output_tokens": 20}}


def results_in(body):
    msgs = body.get("messages", [])
    if not msgs or msgs[-1].get("role") != "user" or isinstance(msgs[-1].get("content"), str):
        return []
    return [c for c in msgs[-1]["content"] if c.get("type") == "tool_result"]


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_GET(self):
        log(f"GET {self.path}")
        self.send_response(404); self.send_header("content-length", "0"); self.end_headers()

    def do_HEAD(self):
        self.do_GET()

    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        raw = self.rfile.read(n)
        import gzip
        if (self.headers.get("content-encoding") or "") == "gzip": raw = gzip.decompress(raw)
        try:
            body = json.loads(raw)
        except Exception:
            body = {}
        with lock:
            REQF.write(json.dumps({"t": time.time(), "path": self.path, "headers": dict(self.headers), "body": body}) + "\n"); REQF.flush()
        if "count_tokens" in self.path:
            return self.json({"input_tokens": 100})
        if "/messages" not in self.path:
            return self.json({})
        model = body.get("model", "claude-x")
        tools = [t.get("name") for t in body.get("tools", [])]
        main = "Bash" in tools or "shell" in tools
        if not main:
            log(f"side request model={model} tools={len(tools)}")
            return self.send_reply(body, [{"type": "text", "text": "Side answer"}], "end_turn")
        res = results_in(body)
        for r in res:
            log(f"RESULT for {r['tool_use_id']}: is_error={r.get('is_error')} content={json.dumps(r.get('content'))[:400]}")
            if r["tool_use_id"] in state["pending"]:
                state["pending"].discard(r["tool_use_id"])
                if probe and not state["pending"] and probe.armed:
                    probe.disarm("result arrived for " + r["tool_use_id"])
        steps = SCEN["steps"]
        if not res and state["step"] > 0 and not SCEN.get("restart_ok"):
            # a new user turn, e.g. a resumed conversation
            log("new user turn with history of %d messages" % len(body.get("messages", [])))
            return self.send_reply(body, [{"type": "text", "text": "Resumed and seen."}], "end_turn")
        if state["step"] < len(steps):
            st = steps[state["step"]]; state["step"] += 1
            blocks = []
            deny_ids = []
            for c in st["calls"]:
                tid = "toolu_" + uuid.uuid4().hex[:20]
                blocks.append({"type": "tool_use", "id": tid, "name": c["name"], "input": c["input"]})
                if c.get("deny"):
                    deny_ids.append(tid)
            if st.get("text"):
                blocks.insert(0, {"type": "text", "text": st["text"]})
            if deny_ids and probe:
                state["pending"] = set(deny_ids)
                probe.arm("deny " + ",".join(deny_ids))
            log(f"STEP {state['step']} send calls {[(b['name'], b.get('id')) for b in blocks if b['type']=='tool_use']}")
            return self.send_reply(body, blocks, "tool_use")
        log("final answer")
        return self.send_reply(body, [{"type": "text", "text": "Done."}], "end_turn")

    def json(self, obj, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def send_reply(self, body, blocks, stop):
        evs, whole = reply_events(body.get("model", "claude-x"), blocks, stop)
        if not body.get("stream"):
            return self.json(whole)
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("cache-control", "no-cache"); self.send_header("connection", "close"); self.end_headers()
        dl = float(os.environ.get("DELAY_MS", "0")) / 1000
        for e in evs:
            self.wfile.write(e); self.wfile.flush()
            if dl: time.sleep(dl)
        self.close_connection = True


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8080"))
    srv = S((os.environ.get("BIND", "127.0.0.1"), port), H)
    pidfile = os.environ.get("ROOTPIDFILE")

    def watch():
        while probe and pidfile:
            try:
                probe.root_pid = int(open(pidfile).read().strip())
            except Exception:
                pass
            time.sleep(0.05)
    threading.Thread(target=watch, daemon=True).start()
    log(f"listening {port}")
    import ssl
    if os.environ.get("TLS_CERT"):
        cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); cx.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"])
        srv.socket = cx.wrap_socket(srv.socket, server_side=True)
    srv.serve_forever()
