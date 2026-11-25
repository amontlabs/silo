"""A stand-in for Silo's boundary, for cloud credentials and exchanged tokens. Throwaway code.

Two servers:

1. The metadata server, at 169.254.169.254:80 (and metadata.google.internal, which the computer's
   names resolve to it). It answers as Google's (Metadata-Flavor: Google) and as Azure's IMDS
   (Metadata: true). It makes tokens from the credentials the Mac holds: it signs the service
   account's assertion itself, or sends Azure's client secret, to the clouds' token endpoints.

2. A TLS endpoint for the cloud hosts, with a certificate from Silo's own authority. It forwards
   each request upstream and:
   - swaps a placeholder for its real value in `Authorization: Bearer` and in the form fields of a
     token request (refresh_token, client_secret), only on the hosts that value is for;
   - in "intercept" mode, rewrites the answer of a known token endpoint: each access, id and
     refresh token in it becomes a placeholder, and the real one stays here;
   - scrubs any real value it holds out of every answer, as a last line;
   - logs every byte it sends into the computer, for the leak count.

Modes (config "mode"):
   metadata: "placeholder" | "real"   what the metadata server hands out
   exchange: "intercept" | "pass"     whether a token endpoint's answer is rewritten
   jwt:      "jwt" | "opaque"         the shape of a placeholder for a token that is a JWT

    python3 boundary.py <config.json>
"""
import base64, json, os, re, secrets, socket, ssl, sys, threading, time, datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs, urlencode
import http.client
import jwt as pyjwt
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec

CFG = json.load(open(sys.argv[1]))
MODE = CFG["mode"]
MAC = CFG["mac"]  # the credentials the Mac holds; the computer never gets these
UP = ("127.0.0.1", CFG["upstream_port"])
LOCK = threading.Lock()
GUEST_LOG = open(CFG["guest_bound_log"], "ab")
RECORD = open(CFG["record"], "a")


def fact(**kw):
    kw["t"] = round(time.time(), 3)
    with LOCK: RECORD.write(json.dumps(kw) + "\n"); RECORD.flush()


# placeholders -------------------------------------------------------------------------------

PH = {}    # placeholder -> {"real", "hosts", "kind"}
REAL = {}  # real -> placeholder


def host_matches(host, patterns):
    return any(host == p or (p.startswith("*.") and host.endswith(p[1:])) for p in patterns)


def make_placeholder(real, kind, hosts, keep=None):
    with LOCK:
        if real in REAL: return REAL[real]
        if keep:  # a rotated refresh token keeps the computer's placeholder
            ph = keep
        elif real.count(".") == 2 and real.startswith("eyJ") and MODE["jwt"] == "jwt":
            h, p, s = real.split(".")  # same header and claims, a signature that is not the real one
            ph = f"{h}.{p}.{base64.urlsafe_b64encode(secrets.token_bytes(len(s) * 3 // 4)).decode().rstrip('=')}"
        else:
            prefix = re.match(r"^(ya29\.c\.|1//|0\.A[A-Za-z0-9]{2}|eyJ)?", real).group(0)
            body = secrets.token_urlsafe(len(real))[: len(real) - len(prefix) - 7]
            ph = prefix + "siloPH_" + body
        PH[ph] = {"real": real, "hosts": hosts, "kind": kind}
        REAL[real] = ph
        return ph


for p in CFG["placeholders"]:  # the ones the computer was given at setup, for secrets it holds as files
    PH[p["placeholder"]] = {"real": MAC[p["real_key"]], "hosts": p["hosts"], "kind": p["kind"]}
    REAL[MAC[p["real_key"]]] = p["placeholder"]

TOKEN_HOSTS = {"google_access": ["*.googleapis.com"], "azure_access": ["management.azure.com", "*.azure.com"],
               "google_refresh": ["oauth2.googleapis.com"], "azure_refresh": ["login.microsoftonline.com"]}


def scrub(b):
    """Last line: no real value the boundary holds goes into the computer."""
    n = 0
    with LOCK: pairs = list(REAL.items())
    for real, ph in pairs:
        r = real.encode()
        if r in b: n += b.count(r); b = b.replace(r, ph.encode())
    return b, n


def to_guest(where, data):
    with LOCK:
        GUEST_LOG.write(f"\n--- {where} {time.time():.3f}\n".encode() + data); GUEST_LOG.flush()


# upstream ---------------------------------------------------------------------------------------

def upstream(method, host, path, headers, body):
    c = http.client.HTTPConnection(*UP, timeout=20)
    h = {k: v for k, v in headers if k.lower() not in ("host", "content-length", "connection", "accept-encoding")}
    h["Host"] = host; h["Content-Length"] = str(len(body)); h["Accept-Encoding"] = "identity"
    c.request(method, path, body=body, headers=h)
    r = c.getresponse(); data = r.read()
    return r.status, r.reason, [(k, v) for k, v in r.getheaders() if k.lower() not in ("content-length", "transfer-encoding", "connection")], data


# minting from the Mac's credentials ---------------------------------------------------------------

CACHE = {}


def google_token(scopes=None, audience=None):
    key = ("google", audience or " ".join(scopes or []))
    c = CACHE.get(key)
    if c and c["exp"] > time.time() + 300: return c["value"], int(c["exp"] - time.time())
    now = int(time.time())
    claims = {"iss": MAC["google_sa_email"], "aud": "https://oauth2.googleapis.com/token", "iat": now, "exp": now + 3600}
    if audience: claims["target_audience"] = audience
    else: claims["scope"] = " ".join(scopes or ["https://www.googleapis.com/auth/cloud-platform"])
    assertion = pyjwt.encode(claims, MAC["google_sa_private_pem"], algorithm="RS256", headers={"kid": MAC["google_sa_key_id"]})
    st, _, _, data = upstream("POST", "oauth2.googleapis.com", "/token",
                              [("Content-Type", "application/x-www-form-urlencoded")],
                              urlencode({"grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer", "assertion": assertion}).encode())
    j = json.loads(data)
    v = j["id_token"] if audience else j["access_token"]
    CACHE[key] = {"value": v, "exp": now + 3599}
    fact(kind="metadata.minted", cloud="google", what="id_token" if audience else "access_token")
    return v, 3599


def azure_token(resource):
    key = ("azure", resource)
    c = CACHE.get(key)
    if c and c["exp"] > time.time() + 300: return c["value"], int(c["exp"] - time.time())
    st, _, _, data = upstream("POST", "login.microsoftonline.com", f"/{MAC['azure_tenant']}/oauth2/v2.0/token",
                              [("Content-Type", "application/x-www-form-urlencoded")],
                              urlencode({"grant_type": "client_credentials", "client_id": MAC["azure_client_id"],
                                         "client_secret": MAC["azure_client_secret"],
                                         "scope": resource.rstrip("/") + "/.default"}).encode())
    v = json.loads(data)["access_token"]
    CACHE[key] = {"value": v, "exp": time.time() + 3599}
    fact(kind="metadata.minted", cloud="azure", resource=resource)
    return v, 3599


def hand_out(real, kind, hosts):
    return make_placeholder(real, kind, hosts) if MODE["metadata"] == "placeholder" else real


# 1. the metadata server -------------------------------------------------------------------------

class Metadata(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def send(self, code, body, ctype="application/json", flavor=True):
        b = body if isinstance(body, bytes) else (json.dumps(body).encode() if not isinstance(body, str) else body.encode())
        self.send_response(code)
        if flavor: self.send_header("Metadata-Flavor", "Google")
        self.send_header("Content-Type", ctype); self.send_header("Content-Length", str(len(b))); self.end_headers()
        self.wfile.write(b)
        to_guest(f"metadata {self.path}", b)

    def do_GET(self):
        u = urlparse(self.path); q = {k: v[0] for k, v in parse_qs(u.query).items()}; p = u.path
        fact(kind="metadata.request", path=p, flavor=self.headers.get("Metadata-Flavor"), azure=self.headers.get("Metadata"))
        if self.headers.get("X-Forwarded-For"):  # as Google's server does: a forwarded request is refused
            return self.send(403, "Forbidden", "text/plain")
        # Azure IMDS
        if p.startswith("/metadata/"):
            if self.headers.get("Metadata", "").lower() != "true":
                return self.send(400, {"error": "invalid_request", "error_description": "Required metadata header not specified"}, flavor=False)
            if p == "/metadata/identity/oauth2/token":
                res = q.get("resource", "https://management.azure.com/")
                real, ttl = azure_token(res)
                tok = hand_out(real, "azure_access", TOKEN_HOSTS["azure_access"])
                now = int(time.time())
                return self.send(200, {"access_token": tok, "client_id": MAC["azure_client_id"], "expires_in": str(ttl),
                                       "expires_on": str(now + ttl), "ext_expires_in": str(ttl), "not_before": str(now),
                                       "resource": res, "token_type": "Bearer"}, flavor=False)
            if p == "/metadata/instance":
                return self.send(200, {"compute": {"subscriptionId": "0000", "resourceGroupName": "silo", "location": "westeurope",
                                                   "azEnvironment": "AzurePublicCloud"}}, flavor=False)
            return self.send(404, {"error": "not found"}, flavor=False)
        # Google
        if p in ("", "/"):
            return self.send(200, "computeMetadata/\n", "application/text")
        if self.headers.get("Metadata-Flavor") != "Google":
            return self.send(403, "Missing Metadata-Flavor:Google header.", "text/html")
        base = "/computeMetadata/v1/"
        if not p.startswith(base): return self.send(404, "Not Found", "text/html")
        rest = p[len(base):]
        email = MAC["google_sa_email"]
        sa = re.match(r"instance/service-accounts/(default|[^/]+)/?(.*)$", rest)
        if rest == "project/project-id": return self.send(200, MAC["google_project"], "application/text")
        if rest == "project/numeric-project-id": return self.send(200, "123456789012", "application/text")
        if rest == "universe/universe-domain": return self.send(200, "googleapis.com", "application/text")
        if rest == "instance/zone": return self.send(200, "projects/123456789012/zones/europe-west9-a", "application/text")
        if rest == "instance/id": return self.send(200, "4242424242424242424", "application/text")
        if rest in ("instance/service-accounts/", "instance/service-accounts") or (sa and sa.group(2) == "" and q.get("recursive")):
            if sa and q.get("recursive"):
                return self.send(200, {"aliases": ["default"], "email": email,
                                       "scopes": ["https://www.googleapis.com/auth/cloud-platform"]})
            return self.send(200, "default/\n" + email + "/\n", "application/text")
        if sa:
            what = sa.group(2)
            if what == "email": return self.send(200, email, "application/text")
            if what == "scopes": return self.send(200, "https://www.googleapis.com/auth/cloud-platform\n", "application/text")
            if what == "aliases": return self.send(200, "default\n", "application/text")
            if what == "token":
                scopes = q["scopes"].split(",") if "scopes" in q else None
                real, ttl = google_token(scopes)
                return self.send(200, {"access_token": hand_out(real, "google_access", TOKEN_HOSTS["google_access"]),
                                       "expires_in": ttl, "token_type": "Bearer"})
            if what == "identity":
                aud = q.get("audience")
                if not aud: return self.send(400, "non-empty audience parameter required", "text/html")
                real, _ = google_token(audience=aud)
                host = urlparse(aud).hostname or aud
                return self.send(200, hand_out(real, "google_id", [host]), "text/html")
        return self.send(404, "Not Found", "text/html")


# 2. the TLS endpoint ----------------------------------------------------------------------------

CA_KEY = serialization.load_pem_private_key(open(CFG["ca_key"], "rb").read(), None)
CA_CERT = x509.load_pem_x509_certificate(open(CFG["ca_cert"], "rb").read())
CTX = {}


def ctx_for(name):
    if name in CTX: return CTX[name]
    k = ec.generate_private_key(ec.SECP256R1())
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (x509.CertificateBuilder().subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, name)]))
            .issuer_name(CA_CERT.subject).public_key(k.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(hours=1)).not_valid_after(now + datetime.timedelta(days=3))
            .add_extension(x509.SubjectAlternativeName([x509.DNSName(name)]), False)
            .add_extension(x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.SERVER_AUTH]), False)
            .add_extension(x509.AuthorityKeyIdentifier.from_issuer_public_key(CA_KEY.public_key()), False)
            .sign(CA_KEY, hashes.SHA256()))
    d = CFG["state_dir"]
    open(f"{d}/{name}.pem", "wb").write(cert.public_bytes(serialization.Encoding.PEM) + CA_CERT.public_bytes(serialization.Encoding.PEM))
    open(f"{d}/{name}.key", "wb").write(k.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    c = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); c.load_cert_chain(f"{d}/{name}.pem", f"{d}/{name}.key")
    c.set_alpn_protocols(["http/1.1"])
    CTX[name] = c; return c


TOKEN_ENDPOINTS = [("oauth2.googleapis.com", re.compile(r"^/token$"), "google"),
                   ("login.microsoftonline.com", re.compile(r"^/[^/]+/oauth2/(v2\.0/)?token$"), "azure")]


class Front(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def do_GET(self): self.handle_one("GET")
    def do_POST(self): self.handle_one("POST")
    def do_PUT(self): self.handle_one("PUT")
    def do_DELETE(self): self.handle_one("DELETE")
    def do_PATCH(self): self.handle_one("PATCH")

    def handle_one(self, method):
        host = (self.headers.get("Host") or "").split(":")[0]
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        headers = list(self.headers.items())
        swapped = []
        # placeholders in Authorization, on the hosts they are for
        out = []
        for k, v in headers:
            if k.lower() == "authorization" and v.startswith("Bearer "):
                ph = v[7:]; e = PH.get(ph)
                if e and host_matches(host, e["hosts"]):
                    v = "Bearer " + e["real"]; swapped.append(e["kind"])
                elif e:
                    fact(kind="boundary.placeholder_elsewhere", host=host, secret_kind=e["kind"])
            out.append((k, v))
        headers = out
        ep = next((c for h, rx, c in TOKEN_ENDPOINTS if h == host and rx.match(urlparse(self.path).path)), None)
        if ep and body:
            form = parse_qs(body.decode(), keep_blank_values=True)
            changed = False
            for f in ("refresh_token", "client_secret", "assertion"):
                if f in form and form[f][0] in PH and host_matches(host, PH[form[f][0]]["hosts"]):
                    swapped.append(PH[form[f][0]]["kind"]); form[f] = [PH[form[f][0]]["real"]]; changed = True
            incoming_refresh_ph = parse_qs(body.decode()).get("refresh_token", [None])[0]
            if changed: body = urlencode({k: v[0] for k, v in form.items()}).encode()
        st, reason, rh, data = upstream(method, host, self.path, headers, body)
        rewritten = []
        if ep and MODE["exchange"] == "intercept" and st == 200:
            try:
                j = json.loads(data)
                for f, kind in (("access_token", f"{ep}_access"), ("id_token", f"{ep}_id"), ("refresh_token", f"{ep}_refresh")):
                    if f in j:
                        hosts = TOKEN_HOSTS.get(kind) or ([MAC["google_client_id"]] if kind == "google_id" else [])
                        keep = incoming_refresh_ph if (f == "refresh_token" and incoming_refresh_ph in PH) else None
                        if keep: PH[keep]["real"] = j[f]; REAL[j[f]] = keep
                        j[f] = make_placeholder(j[f], kind, hosts, keep=keep); rewritten.append(f)
                data = json.dumps(j).encode()
            except ValueError:
                pass
        data, n = scrub(data)
        fact(kind="boundary.request", host=host, path=urlparse(self.path).path, status=st, swapped=swapped,
             answer_rewritten=rewritten, scrubbed=n)
        self.send_response(st, reason)
        for k, v in rh: self.send_header(k, v)
        self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)
        to_guest(f"{host} {self.path[:80]}", data)


class TLSServer(ThreadingHTTPServer):
    def get_request(self):
        s, a = self.socket.accept()
        base = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        base.sni_callback = lambda sock, name, _: setattr(sock, "context", ctx_for(name or "localhost"))
        base.load_cert_chain(*(lambda c: (f"{CFG['state_dir']}/localhost.pem", f"{CFG['state_dir']}/localhost.key"))(ctx_for("localhost")))
        return base.wrap_socket(s, server_side=True), a


def serve(srv): threading.Thread(target=srv.serve_forever, daemon=True).start()


serve(ThreadingHTTPServer((CFG["metadata_ip"], 80), Metadata))
serve(TLSServer(("127.0.0.1", 443), Front))
fact(kind="boundary.start", mode=MODE)
print("ready", flush=True)
threading.Event().wait()
