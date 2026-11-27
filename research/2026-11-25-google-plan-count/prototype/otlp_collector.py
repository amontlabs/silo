"""A minimal OTLP/HTTP JSON collector (#407's, taking chunked and gzip bodies, as Gemini CLI sends them, and noting what it cannot parse): writes every metric point and log record it gets to
<OUT>/otlp.jsonl. It stands in for Silo listening for what a program says about itself.

Run: OUT=out PORT=4318 python3 otlp_collector.py
"""
import gzip, http.server, json, os, time

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
F = open(f"{OUT}/otlp.jsonl", "a")


def attrs(lst):
    d = {}
    for a in lst or []:
        v = a.get("value", {})
        d[a["key"]] = next(iter(v.values()), None) if v else None
    return d


class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        if self.headers.get("transfer-encoding", "").lower() == "chunked":
            raw = b""
            while True:
                n = int(self.rfile.readline().split(b";")[0].strip(), 16)
                if n == 0:
                    self.rfile.readline(); break
                raw += self.rfile.read(n); self.rfile.readline()
        else:
            raw = self.rfile.read(int(self.headers.get("content-length", 0)))
        if self.headers.get("content-encoding") == "gzip":
            raw = gzip.decompress(raw)
        try:
            body = json.loads(raw or b"{}")
        except ValueError:
            F.write(json.dumps({"kind": "unparsed", "path": self.path,
                                "type": self.headers.get("content-type")}) + "\n"); F.flush()
            body = {}
        if self.path.endswith("/v1/logs"):
            for rl in body.get("resourceLogs", []):
                for sl in rl.get("scopeLogs", []):
                    for r in sl.get("logRecords", []):
                        F.write(json.dumps({"kind": "log", "t": time.time(),
                                            "body": r.get("body", {}).get("stringValue"),
                                            "attrs": attrs(r.get("attributes"))}) + "\n")
        elif self.path.endswith("/v1/metrics"):
            for rm in body.get("resourceMetrics", []):
                for sm in rm.get("scopeMetrics", []):
                    for m in sm.get("metrics", []):
                        pts = (m.get("sum") or m.get("gauge") or {}).get("dataPoints", [])
                        for p in pts:
                            F.write(json.dumps({"kind": "metric", "name": m["name"],
                                                "value": p.get("asDouble", p.get("asInt")),
                                                "attrs": attrs(p.get("attributes"))}) + "\n")
        F.flush()
        self.send_response(200); self.send_header("content-type", "application/json")
        self.send_header("content-length", "2"); self.end_headers(); self.wfile.write(b"{}")


http.server.ThreadingHTTPServer(("127.0.0.1", int(os.environ.get("PORT", "4318"))), H).serve_forever()
