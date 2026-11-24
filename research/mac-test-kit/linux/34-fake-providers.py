"""Fake providers for the Linux run of test 34, adapted from the stand-ins of the research of
24 November 2026 (fake_anthropic.py, fake_openai.py): one TLS server that answers as
api.anthropic.com or chatgpt.com by the Host header. The fake agents ask for a reply that is not a
normal one with the header x-silokit-fake: 429, 529, cut (a stream that stops half way), midstream
(an error event inside a 200 stream) or nows (a refused WebSocket).

  34-fake-providers.py PORT_FILE CERT KEY
"""
import base64
import gzip
import hashlib
import json
import struct
import sys
import time
import uuid
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ssl

PLAN = {"anthropic-ratelimit-unified-status": "allowed", "anthropic-ratelimit-unified-5h-utilization": "0.12",
        "anthropic-ratelimit-unified-7d-utilization": "0.34", "anthropic-ratelimit-unified-reset": "1764000000",
        "anthropic-ratelimit-unified-representative-claim": "five_hour"}
CODEX = {"x-codex-primary-used-percent": "7", "x-codex-secondary-used-percent": "21",
         "x-codex-primary-window-minutes": "300", "x-codex-secondary-window-minutes": "10080"}


def sse(ev, data):
    return ("event: %s\ndata: %s\n\n" % (ev, json.dumps(data))).encode()


def claude_events(model):
    mid = "msg_" + uuid.uuid4().hex[:20]
    return [sse("message_start", {"type": "message_start", "message": {"id": mid, "model": model, "usage": {"input_tokens": 50, "output_tokens": 1, "cache_read_input_tokens": 900}}}),
            sse("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            sse("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ok"}}),
            sse("content_block_stop", {"type": "content_block_stop", "index": 0}),
            sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 20}}),
            sse("message_stop", {"type": "message_stop"})]


def codex_events(model):
    rid = "resp_" + uuid.uuid4().hex[:20]
    item = {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "ok"}]}
    return rid, [{"type": "response.created", "response": {"id": rid, "status": "in_progress"}},
                 {"type": "response.output_item.done", "output_index": 0, "item": item},
                 {"type": "response.completed", "response": {"id": rid, "model": model, "status": "completed", "output": [item],
                                                             "usage": {"input_tokens": 50, "input_tokens_details": {"cached_tokens": 10}, "output_tokens": 20}}}]


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def body(self):
        n = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(n) if n else b""
        if (self.headers.get("content-encoding") or "") == "gzip":
            raw = gzip.decompress(raw)
        try:
            return json.loads(raw)
        except ValueError:
            return {}

    def json_reply(self, code, obj, extra=None):
        b = json.dumps(obj).encode()
        self.send_response(code)
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        host = (self.headers.get("host") or "").split(":")[0]
        if (self.headers.get("upgrade") or "").lower() == "websocket":
            if self.headers.get("x-silokit-fake") == "nows":
                return self.json_reply(426, {"error": {"type": "upgrade_required"}})
            return self.ws()
        if host == "api.anthropic.com" and self.path.startswith("/api/claude_code/"):
            return self.json_reply(200, {})
        self.json_reply(404, {"error": {"type": "not_found"}})

    def do_POST(self):
        host = (self.headers.get("host") or "").split(":")[0]
        body = self.body()
        fake = self.headers.get("x-silokit-fake") or ""
        model = body.get("model", "model-x")
        if host == "api.anthropic.com":
            if fake == "429":
                return self.json_reply(429, {"type": "error", "error": {"type": "rate_limit_error", "message": "limit"}},
                                       dict(PLAN, **{"anthropic-ratelimit-unified-status": "rejected", "retry-after": "1"}))
            if fake == "529":
                return self.json_reply(529, {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}},
                                       {"x-should-retry": "true"})
            evs = claude_events(model)
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("request-id", "req_" + uuid.uuid4().hex[:16])
            for k, v in PLAN.items():
                self.send_header(k, v)
            if fake in ("cut", "midstream"):
                # No length and no chunks: the stream ends when the connection does.
                self.send_header("connection", "close")
                self.end_headers()
                self.wfile.write(evs[0] + evs[1])
                if fake == "midstream":
                    self.wfile.write(sse("error", {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}))
                self.wfile.flush()
                self.close_connection = True
                return
            self.send_header("transfer-encoding", "chunked")
            self.end_headers()
            for e in evs:
                self.wfile.write(b"%x\r\n%s\r\n" % (len(e), e))
                self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
            return
        if fake == "429":
            return self.json_reply(429, {"error": {"type": "usage_limit_reached", "message": "limit"}}, dict(CODEX, **{"retry-after": "1"}))
        _, evs = codex_events(model)
        data = b"".join(sse(e["type"], e) for e in evs)
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        for k, v in CODEX.items():
            self.send_header(k, v)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def ws(self):
        key = self.headers["sec-websocket-key"]
        acc = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        defl = "permessage-deflate" in (self.headers.get("sec-websocket-extensions") or "")
        cobj, dobj = zlib.compressobj(6, zlib.DEFLATED, -15), zlib.decompressobj(-15)
        self.send_response(101)
        self.send_header("upgrade", "websocket")
        self.send_header("connection", "Upgrade")
        self.send_header("sec-websocket-accept", acc)
        for k, v in CODEX.items():
            self.send_header(k, v)
        if defl:
            self.send_header("sec-websocket-extensions", "permessage-deflate")
        self.end_headers()
        self.wfile.flush()
        rf, wf = self.rfile, self.wfile

        def send(data, op=1):
            data = data.encode() if isinstance(data, str) else data
            rsv = 0
            if defl and op == 1:
                data, rsv = (cobj.compress(data) + cobj.flush(zlib.Z_SYNC_FLUSH))[:-4], 0x40
            n = len(data)
            hd = bytes([0x80 | rsv | op]) + (bytes([n]) if n < 126 else bytes([126]) + struct.pack(">H", n) if n < 65536 else bytes([127]) + struct.pack(">Q", n))
            wf.write(hd + data)
            wf.flush()

        def recv():
            msg, op0, comp = b"", None, False
            while True:
                h = rf.read(2)
                if len(h) < 2:
                    return None, None
                fin, op, ln, rsv1 = h[0] & 0x80, h[0] & 0x0F, h[1] & 0x7F, h[0] & 0x40
                if op != 0:
                    comp = bool(rsv1)
                if ln == 126:
                    ln = struct.unpack(">H", rf.read(2))[0]
                elif ln == 127:
                    ln = struct.unpack(">Q", rf.read(8))[0]
                mk = rf.read(4) if h[1] & 0x80 else b"\0\0\0\0"
                data = bytes(b ^ mk[i % 4] for i, b in enumerate(rf.read(ln)))
                if op == 8:
                    return 8, data
                if op0 is None:
                    op0 = op
                msg += data
                if fin:
                    if defl and comp:
                        msg = dobj.decompress(msg + b"\x00\x00\xff\xff")
                    return op0, msg

        while True:
            op, data = recv()
            if op is None or op == 8:
                if op == 8:
                    send(b"", 8)
                return
            body = json.loads(data)
            send(json.dumps({"type": "codex.rate_limits", "rate_limits": {"primary": {"used_percent": 7.0, "window_minutes": 300}}}))
            rid, evs = codex_events(body.get("model", "model-x"))
            if body.get("generate") is False:
                evs = [evs[0], {"type": "response.completed", "response": {"id": rid, "status": "completed", "output": []}}]
            for e in evs:
                send(json.dumps(e))
                time.sleep(0.01)


class S(ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    srv = S(("127.0.0.1", 0), H)
    cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    cx.load_cert_chain(sys.argv[2], sys.argv[3])
    srv.socket = cx.wrap_socket(srv.socket, server_side=True)
    open(sys.argv[1], "w").write(str(srv.server_address[1]))
    srv.serve_forever()
