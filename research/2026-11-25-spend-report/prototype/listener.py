"""Silo's listener for a program's own spend report: OTLP over HTTP with JSON, the format Silo
sets when it sets up Claude Code. Extends #407's otlp_collector.py with what the boundary knows of
each export: the computer it came from (the address it arrived on) and when it arrived.

Every log record and metric point goes to <OUT>/reports.jsonl, one per line, as told.

Run: OUT=out PORT=4318 python3 listener.py
"""
import http.server, json, os, time

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
F = open(f"{OUT}/reports.jsonl", "a")


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
        got = round(time.time(), 4)
        peer = self.client_address[0]
        raw = self.rfile.read(int(self.headers.get("content-length", 0)))
        try:
            body = json.loads(raw or b"{}")
        except ValueError:                       # protobuf or anything else: kept as a fact, unread
            F.write(json.dumps({"kind": "unreadable", "computer": peer, "got": got, "path": self.path,
                                "content_type": self.headers.get("content-type"), "bytes": len(raw)}) + "\n")
            body = {}
        if self.path.endswith("/v1/logs"):
            for rl in body.get("resourceLogs", []):
                res = attrs(rl.get("resource", {}).get("attributes"))
                for sl in rl.get("scopeLogs", []):
                    for r in sl.get("logRecords", []):
                        F.write(json.dumps({"kind": "log", "computer": peer, "got": got,
                                            "t": int(r.get("timeUnixNano") or 0) / 1e9,
                                            "body": r.get("body", {}).get("stringValue"),
                                            "attrs": attrs(r.get("attributes")), "resource": res}) + "\n")
        elif self.path.endswith("/v1/metrics"):
            for rm in body.get("resourceMetrics", []):
                for sm in rm.get("scopeMetrics", []):
                    for m in sm.get("metrics", []):
                        pts = (m.get("sum") or m.get("gauge") or {}).get("dataPoints", [])
                        for p in pts:
                            F.write(json.dumps({"kind": "metric", "computer": peer, "got": got, "name": m["name"],
                                                "value": p.get("asDouble", p.get("asInt")),
                                                "attrs": attrs(p.get("attributes"))}) + "\n")
        F.flush()
        self.send_response(200); self.send_header("content-type", "application/json")
        self.send_header("content-length", "2"); self.end_headers(); self.wfile.write(b"{}")


http.server.ThreadingHTTPServer(("127.0.0.1", int(os.environ.get("PORT", "4318"))), H).serve_forever()
