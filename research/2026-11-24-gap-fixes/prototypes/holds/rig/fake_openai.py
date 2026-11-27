"""A fake OpenAI Responses API, with the same scenarios and probe as fake_anthropic.py.
Changed for the holds experiments: a call whose input has "session_id": "$LAST" gets the session
ID that Codex gave in the latest result ("Process running with session ID N")."""
import json, os, sys, time, uuid, http.server, threading
sys.path.insert(0, os.path.dirname(__file__))
from probe import Probe, FifoProbe
OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
SCEN = json.load(open(os.environ["SCENARIO"]))
LOGF = open(f"{OUT}/log.txt", "a"); REQF = open(f"{OUT}/requests.jsonl", "a"); lock = threading.Lock()
def log(s):
    with lock: LOGF.write(f"{time.time():.3f} {s}\n"); LOGF.flush()
probe = (FifoProbe(os.environ["PROBE_FIFO"], SCEN.get("allow", []), log) if os.environ.get("PROBE_FIFO") else Probe(SCEN.get("allow", []), log)) if SCEN.get("probe", True) else None
state = {"step": 0, "pending": set()}
def sse(data):
    return f"event: {data['type']}\ndata: {json.dumps(data)}\n\n".encode()
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass
    def do_GET(self):
        log(f"GET {self.path} upgrade={self.headers.get('upgrade')} ext={self.headers.get('sec-websocket-extensions')}")
        if (self.headers.get('upgrade') or '').lower() == 'websocket' and os.environ.get('WS'):
            return self.ws_serve()
        b = b'{"models":[]}' if 'models' in self.path else b''
        self.send_response(200 if 'models' in self.path else 404); self.send_header("content-type","application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_POST(self):
        n = int(self.headers.get("content-length", 0)); raw = self.rfile.read(n)
        import gzip
        if (self.headers.get("content-encoding") or "") == "gzip": raw = gzip.decompress(raw)
        try: body = json.loads(raw)
        except Exception: body = {}
        with lock: REQF.write(json.dumps({"t": time.time(), "path": self.path, "headers": dict(self.headers), "body": body}) + "\n"); REQF.flush()
        if "/responses" not in self.path:
            log(f"POST {self.path}"); return self.reply_json({})
        items = self.step(body)
        rid = "resp_" + uuid.uuid4().hex[:20]
        evs = [sse({"type": "response.created", "sequence_number": 0, "response": {"id": rid, "object": "response", "status": "in_progress", "output": []}})]
        for k, it in enumerate(items):
            evs.append(sse({"type": "response.output_item.added", "output_index": k, "item": it}))
            evs.append(sse({"type": "response.output_item.done", "output_index": k, "item": it}))
        evs.append(sse({"type": "response.completed", "response": {"id": rid, "object": "response", "status": "completed", "output": items, "usage": {"input_tokens": 50, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 20, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 70}}}))
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("connection", "close"); self.end_headers()
        for e in evs: self.wfile.write(e); self.wfile.flush()
        self.close_connection = True
    def step(self, body):
        inp = body.get("input", [])
        res = [i for i in inp if isinstance(i, dict) and i.get("type") in ("function_call_output", "custom_tool_call_output")]
        new = [r for r in res if r["call_id"] not in state.setdefault("seen", set())]
        for r in new:
            state["seen"].add(r["call_id"])
            log(f"RESULT for {r['call_id']}: {json.dumps(r.get('output'))[:500]}")
            import re as _re
            m = _re.search(r"session ID (\d+)", json.dumps(r.get("output")))
            if m: state["last_session"] = int(m.group(1))
            if r["call_id"] in state["pending"]:
                state["pending"].discard(r["call_id"])
                if probe and not state["pending"] and probe.armed: probe.disarm("result arrived for " + r["call_id"])
        tools = [t.get("name") or t.get("type") for t in body.get("tools", [])]
        log(f"responses request tools={tools} input_items={len(inp)} new_results={len(new)}")
        items = []
        if not new and state["step"] > 0:
            log("new user turn, history items %d" % len(inp))
            items.append(self.msg("Resumed and seen."))
        elif state["step"] < len(SCEN["steps"]):
            st = SCEN["steps"][state["step"]]; state["step"] += 1; deny = []
            for c in st["calls"]:
                cid = "call_" + uuid.uuid4().hex[:20]
                if c.get("custom"):
                    items.append({"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:20], "call_id": cid, "name": c["name"], "input": c["input"], "status": "completed"})
                else:
                    inp_ = dict(c["input"])
                    if inp_.get("session_id") == "$LAST": inp_["session_id"] = state.get("last_session", 0)
                    items.append({"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": cid, "name": c["name"], "arguments": json.dumps(inp_), "status": "completed"})
                if c.get("deny"): deny.append(cid)
            if deny and probe:
                state["pending"] = set(deny); probe.arm("deny " + ",".join(deny))
            log(f"STEP {state['step']} send calls {[(i['name'], i['call_id']) for i in items]}")
        else:
            log("final answer"); items.append(self.msg("Done."))
        return items

    def ws_serve(self):
        import base64, hashlib, struct as st
        key = self.headers['sec-websocket-key']
        acc = base64.b64encode(hashlib.sha1((key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
        import zlib
        defl = bool(os.environ.get('WS_DEFLATE')) and 'permessage-deflate' in (self.headers.get('sec-websocket-extensions') or '')
        cobj = zlib.compressobj(6, zlib.DEFLATED, -15); dobj = zlib.decompressobj(-15)
        self.send_response(101); self.send_header('upgrade', 'websocket'); self.send_header('connection', 'Upgrade'); self.send_header('sec-websocket-accept', acc)
        if defl: self.send_header('sec-websocket-extensions', 'permessage-deflate')
        self.end_headers(); self.wfile.flush()
        log(f"ws deflate={defl}")
        rf, wf = self.rfile, self.wfile
        def recv():
            msg = b''; op0 = None; comp = False
            while True:
                h = rf.read(2)
                if len(h) < 2: return None, None
                fin, op = h[0] & 0x80, h[0] & 0x0f; ln = h[1] & 0x7f; masked = h[1] & 0x80; rsv1 = h[0] & 0x40
                if op != 0: comp = bool(rsv1)
                if ln == 126: ln = st.unpack('>H', rf.read(2))[0]
                elif ln == 127: ln = st.unpack('>Q', rf.read(8))[0]
                mk = rf.read(4) if masked else b'\0\0\0\0'
                data = bytes(b ^ mk[i % 4] for i, b in enumerate(rf.read(ln)))
                if op == 9:  # ping
                    send(data, 10); continue
                if op == 8: return 8, data
                if op0 is None: op0 = op
                msg += data
                if fin:
                    if defl and comp: msg = dobj.decompress(msg + b'\x00\x00\xff\xff')
                    return op0, msg
        def send(data, op=1):
            if isinstance(data, str): data = data.encode()
            rsv = 0
            if defl and op == 1:
                data = (cobj.compress(data) + cobj.flush(zlib.Z_SYNC_FLUSH))[:-4]; rsv = 0x40
            n = len(data)
            hd = bytes([0x80 | rsv | op]) + (bytes([n]) if n < 126 else bytes([126]) + st.pack('>H', n) if n < 65536 else bytes([127]) + st.pack('>Q', n))
            wf.write(hd + data); wf.flush()
        while True:
            op, data = recv()
            if op is None or op == 8: log('ws closed'); return
            try: body = json.loads(data)
            except Exception: log(f'ws non-json {data[:100]!r}'); continue
            with lock: REQF.write(json.dumps({"t": time.time(), "path": "ws", "headers": {}, "body": body}) + "\n"); REQF.flush()
            log(f"ws message type={body.get('type')} keys={list(body)[:12]}")
            items = [] if body.get("generate") is False else self.step(body)
            rid = "resp_" + uuid.uuid4().hex[:20]
            send(json.dumps({"type": "response.created", "response": {"id": rid, "status": "in_progress", "output": []}}))
            for k, it in enumerate(items):
                send(json.dumps({"type": "response.output_item.added", "output_index": k, "item": it}))
                send(json.dumps({"type": "response.output_item.done", "output_index": k, "item": it}))
            send(json.dumps({"type": "response.completed", "response": {"id": rid, "status": "completed", "output": items, "usage": {"input_tokens": 50, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 20, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 70}}}))

    def msg(self, t):
        return {"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": t, "annotations": []}]}
    def reply_json(self, o):
        b = json.dumps(o).encode(); self.send_response(200); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
class S(http.server.ThreadingHTTPServer): daemon_threads = True
if __name__ == "__main__":
    port = int(os.environ["PORT"]); srv = S((os.environ.get("BIND", "127.0.0.1"), port), H); pf = os.environ.get("ROOTPIDFILE")
    def watch():
        while probe and pf:
            try: probe.root_pid = int(open(pf).read().strip())
            except Exception: pass
            time.sleep(0.05)
    threading.Thread(target=watch, daemon=True).start(); log(f"listening {port}"); import ssl
    if os.environ.get("TLS_CERT"):
        cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); cx.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"])
        srv.socket = cx.wrap_socket(srv.socket, server_side=True)
    srv.serve_forever()
