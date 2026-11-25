import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        open(sys.argv[2], "a").write(self.path + "\n"); self.send_response(200); self.send_header("access-control-allow-origin", "*"); self.end_headers()
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
