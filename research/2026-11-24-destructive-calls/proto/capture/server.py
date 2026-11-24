#!/usr/bin/env python3
"""Fake API server for the CLI capture experiment.

Listens on 443 with one certificate for every API host (pointed at 127.0.0.1 in /etc/hosts).
Logs each request as one JSON line. Answers from canned rules so the CLI reaches its
destructive step. In hold mode, a request matching --hold waits --hold-secs, then gets a
refusal in the style --hold-style, the way Silo's boundary would answer at the end of a hold.

Never logs a credential: Authorization and other secret-looking headers are replaced.
"""
import argparse, base64, gzip, json, os, re, select, socket, ssl, subprocess, threading, time, zlib
from urllib.parse import urlsplit, parse_qs

SECRET_HEADERS = re.compile(r"(authorization|cookie|token|secret|api-?key|auth-key|password|session)", re.I)
SENTENCE = "Held by Silo and not answered: deleting volume X needs your yes."

lock = threading.Lock()
conn_counter = [0]


def now():
    return round(time.time(), 3)


class Log:
    def __init__(self, path):
        self.f = open(path, "a", buffering=1)

    def write(self, rec):
        with lock:
            self.f.write(json.dumps(rec, ensure_ascii=False) + "\n")


def redact_headers(headers):
    out = []
    for k, v in headers:
        if SECRET_HEADERS.search(k):
            v = "<redacted>"
        out.append([k, v])
    return out


def decode_body(raw, headers):
    enc = (hget(headers, "content-encoding") or "").lower()
    note = None
    try:
        if enc == "gzip":
            raw = gzip.decompress(raw); note = "gzip"
        elif enc == "deflate":
            raw = zlib.decompress(raw); note = "deflate"
        elif enc == "br":
            note = "br (not decoded)"
    except Exception as e:  # noqa
        note = f"{enc} (decode failed: {e})"
    return raw, note


def hget(headers, name):
    for k, v in headers:
        if k.lower() == name.lower():
            return v
    return None


def graphql_info(body_text):
    try:
        d = json.loads(body_text)
    except Exception:
        return None
    items = d if isinstance(d, list) else [d]
    out = []
    for it in items:
        if isinstance(it, dict) and "query" in it:
            q = it.get("query") or ""
            m = re.search(r"\b(query|mutation|subscription)\b\s*(\w+)?", q)
            # first root field after the first "{"
            root = None
            brace = q.find("{")
            if brace >= 0:
                m2 = re.match(r"\s*(\w+)", q[brace + 1:])
                root = m2.group(1) if m2 else None
            out.append({"operationName": it.get("operationName"),
                        "kind": m.group(1) if m else "query",
                        "nameInQuery": m.group(2) if m else None,
                        "firstRootField": root,
                        "variables": it.get("variables")})
    return out or None


def aws_info(headers, body_text, query):
    info = {}
    t = hget(headers, "x-amz-target")
    if t:
        info["X-Amz-Target"] = t
    if body_text and "Action=" in body_text:
        qs = parse_qs(body_text)
        if "Action" in qs:
            info["Action"] = qs["Action"][0]
    if "Action" in query:
        info["Action"] = query["Action"][0]
    auth = hget(headers, "authorization") or ""
    m = re.search(r"Credential=[^/]+/\d+/([^/]+)/([^/]+)/", auth)
    if m:
        info["sigv4_region"], info["sigv4_service"] = m.group(1), m.group(2)
    return info or None


class Rules:
    """canned/<cli>.json: list of {"host": re, "method": re, "path": re over path?query, "op": re, "status": int,
    "headers": {}, "body": any|str, "body_file": str}. First match wins."""

    def __init__(self, path):
        self.rules = []
        self.dir = os.path.dirname(path or "")
        if path and os.path.exists(path):
            self.rules = json.load(open(path))

    def find(self, host, method, path, ops):
        opnames = [((o.get("operationName") or o.get("nameInQuery") or "") + " " + (o.get("firstRootField") or "")) for o in (ops or [])]
        for r in self.rules:
            if not re.search(r.get("host", ".*"), host or ""):
                continue
            if not re.search(r.get("method", ".*"), method):
                continue
            if not re.search(r.get("path", ".*"), path):
                continue
            if "op" in r and not any(re.search(r["op"], n) for n in opnames):
                continue
            return r
        return None


class Server:
    def __init__(self, args):
        self.args = args
        self.log = Log(args.log)
        self.rules = Rules(args.canned)
        self.hold_re = re.compile(args.hold) if args.hold else None

    def serve(self):
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(self.args.cert, self.args.key)
        ctx.set_alpn_protocols(self.args.alpn.split(","))
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((self.args.bind, self.args.port))
        s.listen(128)
        print(f"listening on {self.args.port} hold={self.args.hold!r} secs={self.args.hold_secs} style={self.args.hold_style}", flush=True)
        while True:
            c, addr = s.accept()
            threading.Thread(target=self.handle_tls, args=(ctx, c), daemon=True).start()

    def handle_tls(self, ctx, c):
        with lock:
            conn_counter[0] += 1
            cid = conn_counter[0]
        try:
            tc = ctx.wrap_socket(c, server_side=True)
        except Exception as e:
            self.log.write({"t": now(), "conn": cid, "event": "tls_fail", "error": str(e)})
            c.close()
            return
        alpn = tc.selected_alpn_protocol()
        self.log.write({"t": now(), "conn": cid, "event": "tls", "alpn": alpn, "tls": tc.version()})
        if alpn == "h2":
            self.log.write({"t": now(), "conn": cid, "event": "h2_chosen_closing"})
            tc.close()
            return
        try:
            self.handle_http(tc, cid)
        except Exception as e:
            self.log.write({"t": now(), "conn": cid, "event": "conn_error", "error": repr(e)})
        finally:
            try:
                tc.close()
            except Exception:
                pass

    def read_request(self, f):
        line = f.readline(65536)
        if not line:
            return None
        line = line.decode("latin-1").rstrip("\r\n")
        if not line:
            line = f.readline(65536).decode("latin-1").rstrip("\r\n")
            if not line:
                return None
        method, target, version = line.split(" ", 2)
        headers = []
        while True:
            h = f.readline(65536).decode("latin-1")
            if h in ("\r\n", "\n", ""):
                break
            k, _, v = h.partition(":")
            headers.append((k.strip(), v.strip()))
        body = b""
        te = (hget(headers, "transfer-encoding") or "").lower()
        if "chunked" in te:
            while True:
                size = int(f.readline().strip().split(b";")[0], 16)
                if size == 0:
                    while f.readline() not in (b"\r\n", b"\n", b""):
                        pass
                    break
                body += f.read(size)
                f.readline()
        else:
            n = int(hget(headers, "content-length") or 0)
            if n:
                body = f.read(n)
        return method, target, version, headers, body, te

    def handle_http(self, tc, cid):
        f = tc.makefile("rb")
        while True:
            req = self.read_request(f)
            if req is None:
                self.log.write({"t": now(), "conn": cid, "event": "client_closed"})
                return
            method, target, version, headers, raw, te = req
            host = (hget(headers, "host") or "").split(":")[0]
            u = urlsplit(target)
            path = u.path
            query = parse_qs(u.query)
            body, enc_note = decode_body(raw, headers)
            ctype = (hget(headers, "content-type") or "")
            rec = {"t": now(), "conn": cid, "event": "request", "method": method, "host": host,
                   "target": target, "http": version, "headers": redact_headers(headers),
                   "body_len": len(raw)}
            if enc_note:
                rec["body_encoding"] = enc_note
            if te:
                rec["transfer_encoding"] = te
            text = None
            if "git-" in ctype or path.endswith("git-receive-pack") or path.endswith("git-upload-pack"):
                rec["git_pktlines"] = pktlines(body)
            else:
                try:
                    text = body.decode("utf-8")
                    rec["body"] = text[:20000]
                except Exception:
                    rec["body_b64_head"] = base64.b64encode(body[:2000]).decode()
            ops = graphql_info(text) if text else None
            if ops:
                rec["graphql"] = ops
            ai = aws_info(headers, text, query)
            if ai:
                rec["aws"] = ai
            if hget(headers, "upgrade"):
                rec["upgrade"] = hget(headers, "upgrade")
            self.log.write(rec)

            key = f"{method} {host} {path} " + " ".join(((o.get('operationName') or o.get('nameInQuery') or '') + ' ' + (o.get('firstRootField') or '')) for o in (ops or []))
            if self.hold_re and self.hold_re.search(key):
                self.hold(tc, cid, key, body)
                return
            if host == "github.com" and (".git/" in path):
                status, rh, rb = self.git_backend(method, path, u.query, headers, body)
            else:
                status, rh, rb = self.canned(host, method, target, ops, text)
            self.send(tc, status, rh, rb)
            self.log.write({"t": now(), "conn": cid, "event": "response", "status": status, "len": len(rb), "for": key.strip()})

    def send(self, tc, status, rh, rb, close=False):
        reason = {200: "OK", 201: "Created", 202: "Accepted", 204: "No Content", 403: "Forbidden", 404: "Not Found"}.get(status, "OK")
        out = f"HTTP/1.1 {status} {reason}\r\n"
        hs = dict(rh)
        hs.setdefault("Content-Type", "application/json")
        if status != 204:
            hs["Content-Length"] = str(len(rb))
        if close:
            hs["Connection"] = "close"
        for k, v in hs.items():
            out += f"{k}: {v}\r\n"
        tc.sendall(out.encode() + b"\r\n" + (rb if status != 204 else b""))

    def hold(self, tc, cid, key, body=b""):
        start = time.time()
        self.log.write({"t": now(), "conn": cid, "event": "hold_start", "for": key.strip(), "secs": self.args.hold_secs})
        closed = False
        while time.time() - start < self.args.hold_secs:
            r, _, _ = select.select([tc], [], [], 0.5)
            if r:
                try:
                    tc.setblocking(False)
                    d = tc.recv(1)
                    tc.setblocking(True)
                    if d == b"":
                        closed = True
                        break
                except (ssl.SSLWantReadError, BlockingIOError):
                    tc.setblocking(True)
                except Exception:
                    closed = True
                    break
        waited = round(time.time() - start, 2)
        if closed:
            self.log.write({"t": now(), "conn": cid, "event": "hold_client_gave_up", "after_s": waited, "for": key.strip()})
            return
        style = self.args.hold_style
        if style == "json403":
            st, h, b = 403, {"Content-Type": "application/json"}, json.dumps({"error": SENTENCE}).encode()
        elif style == "text403":
            st, h, b = 403, {"Content-Type": "text/plain; charset=utf-8"}, (SENTENCE + "\n").encode()
        elif style == "gql200":
            st, h, b = 200, {"Content-Type": "application/json"}, json.dumps({"data": None, "errors": [{"message": SENTENCE, "extensions": {"code": "HELD_BY_SILO"}}]}).encode()
        elif style == "xml403":
            st, h, b = 403, {"Content-Type": "application/xml"}, (f"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>AccessDenied</Code><Message>{SENTENCE}</Message><RequestId>silo</RequestId></Error>").encode()
        elif style == "awsjson403":
            st, h, b = 403, {"Content-Type": "application/x-amz-json-1.0", "x-amzn-ErrorType": "AccessDeniedException"}, json.dumps({"__type": "AccessDeniedException", "message": SENTENCE}).encode()
        elif style == "custom":
            st, h, b = self.args.hold_status, {"Content-Type": self.args.hold_ctype}, self.args.hold_body.replace("SENTENCE", SENTENCE).encode()
        elif style == "gitng":
            st, h, b = 200, {"Content-Type": "application/x-git-receive-pack-result", "Cache-Control": "no-cache"}, git_refusal(body)
        else:
            st, h, b = 403, {}, b""
        try:
            self.send(tc, st, h, b, close=True)
            self.log.write({"t": now(), "conn": cid, "event": "hold_refused", "after_s": waited, "style": style, "for": key.strip()})
        except Exception as e:
            self.log.write({"t": now(), "conn": cid, "event": "hold_send_failed", "after_s": waited, "error": repr(e)})

    def canned(self, host, method, path, ops, text):
        r = self.rules.find(host, method, path, ops)
        if r is None:
            if ops:
                return 200, {}, json.dumps({"data": {}}).encode()
            return 200, {}, b"{}"
        body = r.get("body", {})
        if "body_file" in r:
            body = open(os.path.join(self.rules.dir, r["body_file"]), "rb").read()
        elif not isinstance(body, str):
            body = json.dumps(body)
        if isinstance(body, str):
            body = body.encode()
        return r.get("status", 200), r.get("headers", {}), body

    def git_backend(self, method, path, query, headers, body):
        # https://github.com/o/r.git/... -> GIT_PROJECT_ROOT/o/r.git
        env = dict(os.environ)
        env.update({"GIT_PROJECT_ROOT": self.args.git_root, "GIT_HTTP_EXPORT_ALL": "1",
                    "PATH_INFO": path, "QUERY_STRING": query or "", "REQUEST_METHOD": method,
                    "CONTENT_TYPE": hget(headers, "content-type") or "",
                    "REMOTE_USER": "tester", "REMOTE_ADDR": "127.0.0.1",
                    "CONTENT_LENGTH": str(len(body)), "GIT_HTTP_MAX_REQUEST_BUFFER": "100M"})
        p = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True)
        out = p.stdout
        head, _, rb = out.partition(b"\r\n\r\n")
        if not _:
            head, _, rb = out.partition(b"\n\n")
        status, rh = 200, {}
        for line in head.decode("latin-1").splitlines():
            k, _, v = line.partition(":")
            if k.lower() == "status":
                status = int(v.strip().split()[0])
            elif k:
                rh[k.strip()] = v.strip()
        return status, rh, rb


def pkt(data):
    return b"%04x" % (len(data) + 4) + data


def git_refusal(body):
    """A receive-pack answer in git's own words: every ref update refused ("ng"), with the
    sentence on side-band 2 so git prints it as "remote: ..." and as the reason per ref."""
    refs = []
    for line in pktlines(body):
        if isinstance(line, str) and line.count(" ") >= 2 and len(line) > 81:
            refs.append(line.split("\\0")[0].split(" ")[2])
    inner = pkt(b"unpack ok\n") + b"".join(pkt(f"ng {r} {SENTENCE}\n".encode()) for r in refs) + b"0000"
    return pkt(b"\x02" + f"{SENTENCE}\n".encode()) + pkt(b"\x01" + inner) + b"0000"


def pktlines(body):
    out, i = [], 0
    while i + 4 <= len(body):
        try:
            n = int(body[i:i + 4], 16)
        except ValueError:
            out.append({"raw_rest_len": len(body) - i, "starts": body[i:i + 4].decode("latin-1")})
            break
        if n == 0:
            out.append("0000 (flush)")
            i += 4
            if body[i:i + 4] == b"PACK":
                out.append({"PACK_bytes": len(body) - i})
                break
            continue
        line = body[i + 4:i + n]
        out.append(line.decode("utf-8", "replace").replace("\x00", "\\0"))
        i += n
    return out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=443)
    ap.add_argument("--bind", default="127.0.0.1")
    ap.add_argument("--cert", default="/opt/clis/pki/chain.crt")
    ap.add_argument("--key", default="/opt/clis/pki/leaf.key")
    ap.add_argument("--log", required=True)
    ap.add_argument("--canned")
    ap.add_argument("--hold", help="regex over 'METHOD host path opName rootField'")
    ap.add_argument("--hold-secs", type=float, default=5)
    ap.add_argument("--hold-style", default="json403", choices=["json403", "text403", "gql200", "xml403", "awsjson403", "empty403", "custom", "gitng"])
    ap.add_argument("--hold-status", type=int, default=403)
    ap.add_argument("--hold-ctype", default="application/json")
    ap.add_argument("--hold-body", default="", help="for --hold-style custom; the word SENTENCE is replaced by the sentence")
    ap.add_argument("--alpn", default="http/1.1", help="comma list offered; with h2 first, an h2 pick is logged and the connection closed")
    ap.add_argument("--git-root", default="/opt/clis/git")
    Server(ap.parse_args()).serve()
