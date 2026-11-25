"""A stand-in for api.anthropic.com on plain HTTP that logs, for each request, the path and which
credential header carried what, then answers 401. usage: headers.py <port> <log>"""
import http.server, json, sys
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def any(self):
        n = int(self.headers.get("content-length") or 0)
        if n: self.rfile.read(n)
        rec = {"path": self.path.split("?")[0], "x-api-key": self.headers.get("x-api-key"),
               "authorization": self.headers.get("authorization"), "beta": self.headers.get("anthropic-beta")}
        open(sys.argv[2], "a").write(json.dumps(rec) + "\n")
        b = b'{"type":"error","error":{"type":"authentication_error","message":"stand-in"}}'
        self.send_response(401); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
    do_GET = do_POST = do_HEAD = any
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
