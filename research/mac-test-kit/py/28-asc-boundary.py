"""The boundary of test 28: ends TLS for App Store Connect's hosts, re-signs the token a program in
the computer signed with a placeholder key, and forwards the request to Apple. Python 3.9, no
packages: ES256 goes through the openssl command (LibreSSL on a Mac).

  28-asc-boundary.py serve --port-file F --cert CHAIN --key KEY --real-key P8 --real-kid ID
        [--real-iss UUID] --ph-key P8 --ph-kid ID [--ph-iss UUID] --log JSONL --mode-file F
        [--hosts h1,h2] [--max-seconds N] [--upstream-ca F] [--upstream-at host=ip:port,...]
  28-asc-boundary.py summarize JSONL FROM_LINE     one JSON object: what the boundary saw
  28-asc-boundary.py selftest                      the boundary against a fake Apple, locally

The real key is read on the Mac and never leaves it; the computer only ever holds the
placeholder. The log keeps no token: only its header and claims, minus the signature.
The mode file holds "plain" or "scoped": scoped adds to each GET token a scope naming that one
request, as Apple documents, so the token Apple gets is good for nothing else.
"""
import base64
import http.client
import json
import os
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

AUD = "appstoreconnect-v1"
MAX_LIFE = 20 * 60
HOSTS = ["api.appstoreconnect.apple.com", "appstoreconnect.apple.com",
         "contentdelivery.itunes.apple.com", "developerservices2.apple.com",
         "api.developer.apple.com"]
HOP = {"connection", "keep-alive", "transfer-encoding", "content-length", "proxy-connection", "upgrade"}


def b64u(b):
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


def unb64u(s):
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


# --- ES256 with the openssl command ------------------------------------------------------------

def _der_int(n):
    b = n.to_bytes(33, "big").lstrip(b"\0") or b"\0"
    if b[0] & 0x80:
        b = b"\0" + b
    return b"\x02" + bytes([len(b)]) + b


def raw_to_der(raw):
    body = _der_int(int.from_bytes(raw[:32], "big")) + _der_int(int.from_bytes(raw[32:], "big"))
    return b"\x30" + bytes([len(body)]) + body


def der_to_raw(der):
    i = 2 if der[1] < 0x80 else 2 + (der[1] & 0x7F)
    out = b""
    for _ in range(2):
        n = der[i + 1]
        out += int.from_bytes(der[i + 2:i + 2 + n], "big").to_bytes(32, "big")
        i += 2 + n
    return out


def es256_sign(key_path, data):
    p = subprocess.run(["openssl", "dgst", "-sha256", "-sign", key_path], input=data,
                       capture_output=True, check=True)
    return der_to_raw(p.stdout)


def es256_verify(pub_path, data, raw):
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(raw_to_der(raw))
    try:
        p = subprocess.run(["openssl", "dgst", "-sha256", "-verify", pub_path, "-signature", f.name],
                           input=data, capture_output=True)
        return p.returncode == 0
    finally:
        os.unlink(f.name)


def pubkey(key_path, out_path):
    subprocess.run(["openssl", "ec", "-in", key_path, "-pubout", "-out", out_path],
                   capture_output=True, check=True)
    return out_path


def jwt_sign(key_path, header, claims):
    h = b64u(json.dumps(header, separators=(",", ":")).encode())
    c = b64u(json.dumps(claims, separators=(",", ":")).encode())
    return "%s.%s.%s" % (h, c, b64u(es256_sign(key_path, ("%s.%s" % (h, c)).encode())))


def jwt_parts(token):
    """(header, claims, signed bytes, raw signature) or None."""
    try:
        h, c, s = token.split(".")
        return json.loads(unb64u(h)), json.loads(unb64u(c)), ("%s.%s" % (h, c)).encode(), unb64u(s)
    except (ValueError, TypeError):
        return None


# --- the boundary ------------------------------------------------------------------------------

class Config:
    pass


def resign(cfg, token, host, method, path, now):
    """Returns (new token or None, facts about the token)."""
    facts = {}
    parts = jwt_parts(token)
    if parts is None:
        facts["auth"] = "not-a-jwt"
        return None, facts
    header, claims, signed, sig = parts
    facts.update(auth="jwt", alg=header.get("alg"), kid_is=("placeholder" if header.get("kid") == cfg.ph_kid else "other"),
                 aud=claims.get("aud"), sub=claims.get("sub"), has_iss="iss" in claims,
                 life=(claims.get("exp", 0) - claims.get("iat", now)), scope_in=claims.get("scope"))
    if host not in cfg.hosts:
        facts["refused"] = "host not allowed for this secret"
        return None, facts
    if header.get("alg") != "ES256" or header.get("kid") != cfg.ph_kid:
        facts["refused"] = "wrong algorithm or key id"
        return None, facts
    if not es256_verify(cfg.ph_pub, signed, sig):
        facts["refused"] = "not signed by the placeholder"
        return None, facts
    if claims.get("aud") != AUD:
        facts["refused"] = "audience %r" % claims.get("aud")
        return None, facts
    if cfg.ph_iss and claims.get("iss") != cfg.ph_iss:
        facts["refused"] = "wrong issuer"
        return None, facts
    iat, exp = claims.get("iat", now), claims.get("exp", 0)
    if exp - iat > MAX_LIFE or exp - now > MAX_LIFE or iat > now + 60:
        facts["refused"] = "lifetime longer than 20 minutes"
        return None, facts
    out = dict(claims, iat=now, exp=min(exp, now + MAX_LIFE))
    if cfg.real_iss:
        out["iss"] = cfg.real_iss
    else:
        out.pop("iss", None)
        out["sub"] = "user"
    scoped = cfg.mode() == "scoped" and method == "GET"
    if scoped:
        out["scope"] = ["GET " + path]
    facts["scoped"] = scoped
    return jwt_sign(cfg.real_key, dict(header, kid=cfg.real_kid), out), facts


class Pinned(http.client.HTTPSConnection):
    """An HTTPS connection to a name, opened to another address (a test's fake Apple)."""

    def __init__(self, name, addr, port, context, timeout=60):
        http.client.HTTPSConnection.__init__(self, name, port, context=context, timeout=timeout)
        self._addr = addr

    def connect(self):
        sock = socket.create_connection((self._addr, self.port), self.timeout)
        self.sock = self._context.wrap_socket(sock, server_hostname=self.host)


def make_handler(cfg):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def _body(self):
            if "chunked" in (self.headers.get("Transfer-Encoding") or "").lower():
                out = b""
                while True:
                    n = int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)
                    if n == 0:
                        while self.rfile.readline() not in (b"\r\n", b"\n", b""):
                            pass
                        return out
                    out += self.rfile.read(n)
                    self.rfile.readline()
            n = int(self.headers.get("Content-Length") or 0)
            return self.rfile.read(n) if n else b""

        def _any(self):
            body = self._body()
            host = (self.headers.get("Host") or "").split(":")[0] or cfg.sni.get(id(self.connection), "")
            entry = {"t": round(time.time(), 3), "host": host, "sni": cfg.sni.get(id(self.connection)),
                     "method": self.command, "path": self.path.split("?")[0],
                     "user_agent": (self.headers.get("User-Agent") or "")[:80]}
            auth = self.headers.get("Authorization") or ""
            headers = {k: v for k, v in self.headers.items() if k.lower() not in HOP and k.lower() != "host"}
            if auth.startswith("Bearer "):
                new, facts = resign(cfg, auth[7:], host, self.command, self.path, int(time.time()))
                entry.update(facts)
                if new is None:
                    entry["status"] = 403
                    cfg.write(entry)
                    msg = json.dumps({"refused_by_silo_kit": facts.get("refused", "not re-signed")}).encode()
                    self.send_response(403)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(msg)))
                    self.end_headers()
                    self.wfile.write(msg)
                    return
                entry["resigned"] = True
                for k in list(headers):
                    if k.lower() == "authorization":
                        del headers[k]
                headers["Authorization"] = "Bearer " + new
            else:
                entry["auth"] = "basic" if auth.startswith("Basic ") else ("other" if auth else "none")
            try:
                addr, port = cfg.upstream_at.get(host, (host, 443))
                conn = Pinned(host, addr, port, cfg.upstream_ctx)
                conn.request(self.command, self.path, body=body or None, headers=headers)
                r = conn.getresponse()
                data = r.read()
                status, rheaders = r.status, r.getheaders()
                conn.close()
            except Exception as e:  # the upstream failed: say so to the program and in the log
                entry.update(status=502, upstream_error=str(e)[:160])
                cfg.write(entry)
                self.send_response(502)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            entry["status"] = status
            cfg.write(entry)
            self.send_response(status)
            for k, v in rheaders:
                if k.lower() not in HOP:
                    self.send_header(k, v)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = _any

    return Handler


class TLSServer(ThreadingHTTPServer):
    """Each connection's handshake runs in its own thread; a refused handshake is logged."""
    daemon_threads = True

    def __init__(self, addr, handler, cfg):
        ThreadingHTTPServer.__init__(self, addr, handler)
        self.cfg = cfg

    def finish_request(self, request, client_address):
        try:
            tls = self.cfg.ctx.wrap_socket(request, server_side=True)
        except (ssl.SSLError, OSError) as e:
            self.cfg.write({"t": round(time.time(), 3), "handshake_refused": str(e)[:160],
                            "sni": self.cfg.last_sni})
            return
        try:
            self.RequestHandlerClass(tls, client_address, self)
        finally:
            self.cfg.sni.pop(id(tls), None)


def build(args):
    cfg = Config()
    cfg.real_key, cfg.real_kid, cfg.real_iss = args["real-key"], args["real-kid"], args.get("real-iss", "")
    cfg.ph_kid, cfg.ph_iss = args["ph-kid"], args.get("ph-iss", "")
    cfg.ph_pub = pubkey(args["ph-key"], args["log"] + ".ph.pub.pem")
    cfg.hosts = set((args.get("hosts") or ",".join(HOSTS)).split(","))
    cfg.sni, cfg.last_sni = {}, None
    lock = threading.Lock()
    log_path, mode_file = args["log"], args.get("mode-file")

    def write(e):
        with lock:
            with open(log_path, "a") as f:
                f.write(json.dumps(e) + "\n")

    def mode():
        try:
            return open(mode_file).read().strip() if mode_file else "plain"
        except OSError:
            return "plain"

    cfg.write, cfg.mode = write, mode
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(args["cert"], args["key"])

    def on_sni(sock, name, _ctx):
        cfg.sni[id(sock)] = name
        cfg.last_sni = name

    ctx.sni_callback = on_sni
    cfg.ctx = ctx
    ca = args.get("upstream-ca") or ("/etc/ssl/cert.pem" if os.path.exists("/etc/ssl/cert.pem") else None)
    cfg.upstream_ctx = ssl.create_default_context(cafile=ca)
    cfg.upstream_at = {}
    for item in filter(None, (args.get("upstream-at") or "").split(",")):
        name, target = item.split("=")
        a, p = target.rsplit(":", 1)
        cfg.upstream_at[name] = (a, int(p))
    return cfg


def parse(argv):
    out, i = {}, 0
    while i < len(argv):
        out[argv[i].lstrip("-")] = argv[i + 1]
        i += 2
    return out


def serve(argv):
    args = parse(argv)
    cfg = build(args)
    srv = TLSServer(("127.0.0.1", 0), make_handler(cfg), cfg)
    with open(args["port-file"], "w") as f:
        f.write(str(srv.server_address[1]))
    limit = float(args.get("max-seconds") or 0)
    if limit:
        threading.Timer(limit, srv.shutdown).start()
    srv.serve_forever()


def summarize(path, start):
    """What the boundary saw from line START on: hosts, audiences, lifetimes, statuses."""
    seen = []
    try:
        for i, line in enumerate(open(path)):
            if i >= start and line.strip():
                seen.append(json.loads(line))
    except OSError:
        pass
    req = [e for e in seen if "method" in e]
    s = {
        "requests": len(req),
        "handshakes_refused": len([e for e in seen if "handshake_refused" in e]),
        "hosts": sorted(set(e["host"] for e in req)),
        "audiences": sorted(set(str(e.get("aud")) for e in req if e.get("auth") == "jwt")),
        "lifetimes": sorted(set(e.get("life") for e in req if e.get("auth") == "jwt")),
        "resigned": len([e for e in req if e.get("resigned")]),
        "resigned_ok": len([e for e in req if e.get("resigned") and 200 <= e.get("status", 0) < 300]),
        "scoped": len([e for e in req if e.get("scoped")]),
        "scoped_refused": len([e for e in req if e.get("scoped") and e.get("status") == 401]),
        "refused": sorted(set(e["refused"] for e in req if e.get("refused"))),
        "statuses": sorted(set(e.get("status") for e in req)),
        "other_auth": sorted(set(e["auth"] for e in req if e.get("auth") not in ("jwt", None))),
    }
    print(json.dumps(s))


# --- selftest: the boundary against a fake Apple, on 127.0.0.1 --------------------------------

def selftest():
    d = tempfile.mkdtemp(prefix="silokit-28-selftest-")

    def sh(*a):
        subprocess.run(a, cwd=d, capture_output=True, check=True)

    for name in ("real", "ph", "own"):
        sh("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", name + ".ec")
        sh("openssl", "pkcs8", "-topk8", "-nocrypt", "-in", name + ".ec", "-out", name + ".p8")
    sans = ",".join("DNS:" + h for h in HOSTS + ["paste.example"])
    open(os.path.join(d, "ca.cnf"), "w").write("[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nCN=Kit CA\n"
                                                "[v3]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\n")
    open(os.path.join(d, "leaf.cnf"), "w").write("[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nCN=%s\n"
                                                  "[v3]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
                                                  "extendedKeyUsage=serverAuth\nsubjectAltName=%s\n" % (HOSTS[0], sans))
    sh("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.crt",
       "-days", "2", "-config", "ca.cnf", "-extensions", "v3")
    sh("openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", "leaf.key", "-out", "leaf.csr",
       "-config", "leaf.cnf")
    sh("openssl", "x509", "-req", "-in", "leaf.csr", "-CA", "ca.crt", "-CAkey", "ca.key", "-CAcreateserial",
       "-out", "leaf.crt", "-days", "2", "-extfile", "leaf.cnf", "-extensions", "v3")
    p = lambda n: os.path.join(d, n)
    real_pub = pubkey(p("real.p8"), p("real.pub.pem"))
    REAL_KID, REAL_ISS, PH_KID, PH_ISS = "REALKEY123", "69a6de70-real-issuer", "SILOKIT001", "00000000-silo-placeholder"

    class FakeApple(BaseHTTPRequestHandler):
        """Stands for Apple: trusts only the real key; honours a scope."""
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def _any(self):
            n = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(n) if n else b""
            parts = jwt_parts((self.headers.get("Authorization") or "")[7:])
            ok = False
            if parts:
                h, c, signed, sig = parts
                ok = (es256_verify(real_pub, signed, sig) and h.get("kid") == REAL_KID and c.get("iss") == REAL_ISS
                      and c.get("aud") == AUD and ("%s %s" % (self.command, self.path)) in c.get("scope", ["%s %s" % (self.command, self.path)]))
            msg = json.dumps({"ok": ok, "got_bytes": len(body)}).encode()
            self.send_response(200 if ok else 401)
            self.send_header("Content-Length", str(len(msg)))
            self.end_headers()
            self.wfile.write(msg)

        do_GET = do_POST = _any

    actx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    actx.load_cert_chain(p("leaf.crt"), p("leaf.key"))
    apple = ThreadingHTTPServer(("127.0.0.1", 0), FakeApple)
    apple.socket = actx.wrap_socket(apple.socket, server_side=True)
    threading.Thread(target=apple.serve_forever, daemon=True).start()
    ap = apple.server_address[1]

    log, mode = p("boundary.jsonl"), p("mode")
    open(mode, "w").write("plain")
    args = {"cert": p("leaf.crt"), "key": p("leaf.key"), "real-key": p("real.p8"), "real-kid": REAL_KID,
            "real-iss": REAL_ISS, "ph-key": p("ph.p8"), "ph-kid": PH_KID, "ph-iss": PH_ISS, "log": log,
            "mode-file": mode, "upstream-ca": p("ca.crt"),
            "upstream-at": ",".join("%s=127.0.0.1:%d" % (h, ap) for h in HOSTS)}
    cfg = build(args)
    bsrv = TLSServer(("127.0.0.1", 0), make_handler(cfg), cfg)
    threading.Thread(target=bsrv.serve_forever, daemon=True).start()
    bp = bsrv.server_address[1]

    trusting = ssl.create_default_context(cafile=p("ca.crt"))
    stranger = ssl.create_default_context()  # the system's authorities only: refuses the kit's CA

    def tool_token(key=None, kid=PH_KID, iss=PH_ISS, life=1200, aud=AUD):
        now = int(time.time())
        return jwt_sign(key or p("ph.p8"), {"alg": "ES256", "kid": kid, "typ": "JWT"},
                        {"iss": iss, "iat": now, "exp": now + life, "aud": aud})

    def call(port, token, host=HOSTS[0], method="GET", path="/v1/apps", body=None, ctx=trusting):
        try:
            c = Pinned(HOSTS[0], "127.0.0.1", port, ctx, timeout=20)
            c.request(method, path, body=body, headers={"Authorization": "Bearer " + token, "Host": host})
            r = c.getresponse()
            data = r.read()
            c.close()
            return r.status, data
        except (ssl.SSLError, OSError):
            return "tls-refused", b""

    cases = []
    cases.append(("placeholder token through the boundary", call(bp, tool_token())[0], 200))
    open(mode, "w").write("scoped")
    cases.append(("same, with the GET scoped to its request", call(bp, tool_token(), path="/v1/apps?limit=1")[0], 200))
    st, data = call(bp, tool_token(), method="POST", path="/notary/v2/submissions", body=b"x" * 5000)
    cases.append(("a POST and its body forwarded", st if b'"got_bytes": 5000' in data else "body lost", 200))
    open(mode, "w").write("plain")
    cases.append(("placeholder token sent straight to Apple", call(ap, tool_token())[0], 401))
    cases.append(("placeholder token to another host", call(bp, tool_token(), host="paste.example")[0], 403))
    cases.append(("token made with a key of the agent's own", call(bp, tool_token(key=p("own.p8")))[0], 403))
    cases.append(("token asking for a day", call(bp, tool_token(life=86400))[0], 403))
    cases.append(("token for another audience", call(bp, tool_token(aud="something-else"))[0], 403))
    cases.append(("a program that does not trust the kit's authority", call(bp, tool_token(), ctx=stranger)[0], "tls-refused"))
    time.sleep(0.3)
    lines = [json.loads(x) for x in open(log)]
    cases.append(("the refused handshake is in the log", any("handshake_refused" in e for e in lines), True))
    cases.append(("no token is in the log", not any(".ey" in json.dumps(e) for e in lines), True))
    bad = 0
    for name, got, want in cases:
        bad += got != want
        print("%s %s (want %s)  %s" % ("ok " if got == want else "BAD", got, want, name))
    bsrv.shutdown()
    apple.shutdown()
    subprocess.run(["rm", "-rf", d])
    return 1 if bad else 0


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    cmd = sys.argv[1]
    if cmd == "serve":
        serve(sys.argv[2:])
    elif cmd == "summarize":
        summarize(sys.argv[2], int(sys.argv[3]))
    elif cmd == "selftest":
        return selftest()
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
