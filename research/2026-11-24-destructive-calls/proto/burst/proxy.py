"""Logging proxy in front of moto, standing where Silo's boundary would stand.

    python3 proxy.py --listen 4566 --upstream 127.0.0.1:5000 --log trace.jsonl --holds holds.json

Every request is logged as one JSON line when it ends: when it started and ended (seconds since
the proxy started), which client connection carried it, the AWS service (from the SigV4 credential
scope), the operation (X-Amz-Target, the form field Action, or for S3 and Lambda the method and
path), the SDK's invocation id and attempt, a hash of method+path+body, and the status.

`holds.json` is read on every request, so a run can change it without a restart:

    {"rules": [{"op": "rds:DeleteDBInstance", "seconds": 60, "then": "403", "times": 1}]}

`op` is a regular expression matched against "service:Operation". `then` is "403" (refuse in the
service's own error shape, with Silo's sentence) or "pass" (a yes after `seconds`). `times` limits
how many requests the rule holds (absent: all). Credentials are never logged: the Authorization
header is reduced to its credential scope's service and region.
"""
import argparse
import hashlib
import http.client
import itertools
import json
import re
import socketserver
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlsplit

T0 = time.monotonic()
SENTENCE = "Held by Silo and not answered: this request needs your yes."
lock = threading.Lock()
conn_ids = itertools.count(1)
req_ids = itertools.count(1)
hold_used = {}


def now():
    return round(time.monotonic() - T0, 4)


def scope(headers):
    a = headers.get("Authorization", "")
    m = re.search(r"Credential=[^/]+/\d{8}/([^/]+)/([^/]+)/aws4_request", a)
    return (m.group(1), m.group(2)) if m else ("?", "?")


def operation(service, method, path, query, headers, body):
    tgt = headers.get("X-Amz-Target")
    if tgt:
        return tgt.split(".")[-1]
    ctype = headers.get("Content-Type", "")
    if "x-www-form-urlencoded" in ctype and body:
        act = parse_qs(body.decode("utf-8", "replace")).get("Action")
        if act:
            return act[0]
    if service == "s3":
        parts = [p for p in path.split("/") if p]
        q = parse_qs(query, keep_blank_values=True)
        if method == "DELETE":
            return "DeleteObject" if len(parts) > 1 else "DeleteBucket" if not q else "DeleteBucket" + "".join(k.capitalize() for k in q)
        if method == "POST" and "delete" in q:
            return "DeleteObjects"
        if method == "PUT":
            return "PutObject" if len(parts) > 1 else ("PutBucket" + "".join(k.capitalize() for k in q) if q else "CreateBucket")
        if method in ("GET", "HEAD"):
            return ("HeadObject" if method == "HEAD" else "GetObject") if len(parts) > 1 else "ListOrGetBucket" + "".join(k.capitalize() for k in q if k not in ("list-type", "prefix", "encoding-type", "max-keys", "continuation-token", "delimiter"))
        return method + " " + path
    return method + " " + re.sub(r"/[^/]+$", "/{name}", path) if method == "DELETE" else method + " " + path


def refusal(service, headers, op):
    """403 in the service's own error shape (details/cli-capture.md, section 3)."""
    if headers.get("X-Amz-Target"):
        ctype = headers.get("Content-Type") or "application/x-amz-json-1.0"
        body = json.dumps({"__type": "AccessDeniedException", "message": SENTENCE}).encode()
        return ctype, body, {}
    if service == "s3":
        body = f"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>AccessDenied</Code><Message>{SENTENCE}</Message><RequestId>silo</RequestId></Error>".encode()
        return "application/xml", body, {}
    if service == "ec2":
        body = f"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Errors><Error><Code>UnauthorizedOperation</Code><Message>{SENTENCE}</Message></Error></Errors><RequestID>silo</RequestID></Response>".encode()
        return "text/xml", body, {}
    if service in ("lambda",):
        return "application/json", json.dumps({"Type": "User", "message": SENTENCE}).encode(), {"x-amzn-ErrorType": "AccessDeniedException"}
    body = f"<ErrorResponse><Error><Type>Sender</Type><Code>AccessDenied</Code><Message>{SENTENCE}</Message></Error><RequestId>silo</RequestId></ErrorResponse>".encode()
    return "text/xml", body, {}


def hold_rule(args, key):
    try:
        rules = json.load(open(args.holds)).get("rules", []) if args.holds else []
    except (OSError, ValueError):
        rules = []
    for i, r in enumerate(rules):
        if re.fullmatch(r["op"], key):
            with lock:
                n = hold_used.get((args.holds, i, r["op"]), 0)
                if "times" in r and n >= r["times"]:
                    continue
                hold_used[(args.holds, i, r["op"])] = n + 1
            return r
    return None


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        super().setup()
        self.conn_id = next(conn_ids)
        self.upstream = None

    def log_message(self, *a):
        pass

    def read_body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            out = b""
            while True:
                size = int(self.rfile.readline().split(b";")[0], 16)
                if size == 0:
                    while self.rfile.readline() not in (b"\r\n", b"\n", b""):
                        pass
                    return out
                out += self.rfile.read(size)
                self.rfile.readline()
        n = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(n) if n else b""

    def handle_any(self):
        rid = next(req_ids)
        start = now()
        body = self.read_body()
        u = urlsplit(self.path)
        region, service = scope(self.headers)
        op = operation(service, self.command, u.path, u.query, self.headers, body)
        key = f"{service}:{op}"
        rec = {
            "id": rid, "start": start, "conn": self.conn_id, "client_port": self.client_address[1],
            "service": service, "region": region, "op": key, "method": self.command,
            "path": u.path, "query": u.query,
            "invocation_id": self.headers.get("amz-sdk-invocation-id"),
            "attempt": self.headers.get("amz-sdk-request"),
            "ua": (self.headers.get("User-Agent") or "")[:300],
            "identity": hashlib.sha256(self.command.encode() + b" " + self.path.encode() + b"\n" + body).hexdigest()[:16],
            "host": f"{service}.{region}.amazonaws.com",
            "body_len": len(body),
            "body": body[:2000].decode("utf-8", "replace") if body and (service != "s3" or self.command == "POST" or u.query) else None,
        }
        rule = hold_rule(self.server.args, key)
        eng = self.server.engine
        try:
            if eng and eng.holds(key):
                decision, ask = eng.hold(rec)
                rec["ask"], rec["decision"], rec["held_until"] = ask, decision, now()
                if decision != "yes":
                    rule = {"seconds": 0, "then": "403"}
            if rule:
                rec.setdefault("held", rule["seconds"])
                deadline = time.monotonic() + rule["seconds"]
                while time.monotonic() < deadline:
                    time.sleep(0.2)
                rec["held_until"] = now()
                if rule.get("then", "403") == "403":
                    ctype, rbody, extra = refusal(service, self.headers, op)
                    self.send_response(403)
                    self.send_header("Content-Type", ctype)
                    self.send_header("Content-Length", str(len(rbody)))
                    for k, v in extra.items():
                        self.send_header(k, v)
                    self.end_headers()
                    self.wfile.write(rbody)
                    rec["status"] = 403
                    rec["refused"] = True
                    return
            status, rheaders, rbody = self.forward(body)
            self.send_response(status)
            for k, v in rheaders:
                if k.lower() not in ("transfer-encoding", "connection", "content-length", "server", "date"):
                    self.send_header(k, v)
            if self.command == "HEAD":
                self.send_header("Content-Length", dict((k.lower(), v) for k, v in rheaders).get("content-length", "0"))
            else:
                self.send_header("Content-Length", str(len(rbody)))
            self.end_headers()
            self.wfile.write(rbody)
            rec["status"] = status
        except (BrokenPipeError, ConnectionResetError) as e:
            rec["client_gone"] = type(e).__name__
            self.close_connection = True
        finally:
            rec["end"] = now()
            with lock:
                self.server.log.write(json.dumps(rec) + "\n")
                self.server.log.flush()

    def forward(self, body):
        for attempt in (1, 2):
            try:
                if self.upstream is None:
                    self.upstream = http.client.HTTPConnection(*self.server.up, timeout=120)
                hdrs = {k: v for k, v in self.headers.items() if k.lower() not in ("transfer-encoding", "content-length", "connection")}
                hdrs["Content-Length"] = str(len(body))
                self.upstream.request(self.command, self.path, body=body, headers=hdrs)
                r = self.upstream.getresponse()
                return r.status, r.getheaders(), r.read()
            except (http.client.HTTPException, OSError):
                self.upstream = None
                if attempt == 2:
                    raise

    do_GET = do_PUT = do_POST = do_DELETE = do_HEAD = do_PATCH = handle_any


class Server(socketserver.ThreadingMixIn, HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--listen", type=int, default=4566)
    ap.add_argument("--upstream", default="127.0.0.1:5000")
    ap.add_argument("--log", required=True)
    ap.add_argument("--holds")
    ap.add_argument("--policy")
    ap.add_argument("--asks")
    args = ap.parse_args()
    srv = Server(("127.0.0.1", args.listen), Handler)
    srv.args = args
    srv.engine = None
    if args.policy:
        import asks
        srv.engine = asks.Engine(json.load(open(args.policy)), args.asks or args.log + ".asks", now)
    h, p = args.upstream.split(":")
    srv.up = (h, int(p))
    srv.log = open(args.log, "a")
    print(f"proxy on {args.listen} -> {args.upstream}, log {args.log}", file=sys.stderr, flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
