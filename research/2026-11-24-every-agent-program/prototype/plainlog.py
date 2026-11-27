import http.server, json, sys, time
LOG = open(sys.argv[1], "a")
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass
    def any(self):
        n = int(self.headers.get("content-length", 0) or 0); body = self.rfile.read(n) if n else b""
        LOG.write(json.dumps({"t": time.time(), "m": self.command, "p": self.path, "ct": self.headers.get("content-type"), "stream": (json.loads(body).get("stream") if body[:1] == b"{" else None)}) + "\n"); LOG.flush()
        b = b'{"error":"plain"}'; self.send_response(404); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
    do_GET = do_POST = do_HEAD = any
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[2])), H).serve_forever()
