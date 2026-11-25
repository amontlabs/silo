"""Answers every POST with 400 and keeps its JSON body as body-<model>.json in the current folder.
Used by codex_tools.sh to see which tools Codex offers each model."""
import http.server, json, sys, gzip
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def do_GET(self):
        self.send_response(404); self.end_headers()
    def do_POST(self):
        n=int(self.headers.get('content-length') or 0); raw=self.rfile.read(n)
        if (self.headers.get('content-encoding') or '')=='zstd':
            import zstandard; raw=zstandard.ZstdDecompressor().decompress(raw,max_output_size=1<<28)
        try: b=json.loads(raw)
        except Exception: b={}
        open(f'body-{b.get("model")}.json','w').write(json.dumps(b,indent=1))
        self.send_response(400); self.send_header('content-type','application/json'); m=b'{"error":{"message":"stop"}}'
        self.send_header('content-length',str(len(m))); self.end_headers(); self.wfile.write(m)
http.server.ThreadingHTTPServer(('127.0.0.1',18555),H).serve_forever()
