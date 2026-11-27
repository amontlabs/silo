"""Stand-ins for Google's and Microsoft's servers, plain HTTP on one port, chosen by Host. Throwaway.

They accept only real credentials: the service account's signed assertion, the real refresh
tokens and client secret, and the access tokens they issued. Every token they issue goes to
issued.jsonl, so the leak count can look for each one later. They never echo a credential back.

    python3 fake_clouds.py <state dir> <port>
"""
import base64, json, os, secrets, sys, time, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs
import jwt
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives import serialization

D, PORT = sys.argv[1], int(sys.argv[2])
CREDS = json.load(open(f"{D}/real_credentials.json"))  # what the clouds know: written by run.py
SA_PUB = serialization.load_pem_public_key(CREDS["google_sa_public_pem"].encode())
ISSUER_KEY = rsa.generate_private_key(public_exponent=65537, key_size=2048)  # signs id tokens and Azure tokens
VALID = {}  # access token -> {"cloud", "aud"}
REFRESH = {"google": {CREDS["google_refresh_token"]}, "azure": {CREDS["azure_refresh_token"]}}
LOCK = threading.Lock()
ISSUED = open(f"{D}/issued.jsonl", "a")
SEEN_HEADERS = open(f"{D}/upstream_requests.jsonl", "a")
TID, OID, APPID = CREDS["azure_tenant"], "6f1c2a34-0000-4000-8000-00000000abcd", CREDS["azure_client_id"]


def issue(kind, value, **kw):
    with LOCK:
        ISSUED.write(json.dumps({"kind": kind, "value": value, **kw}) + "\n"); ISSUED.flush()
    return value


def google_access():
    t = issue("google_access", "ya29.c." + secrets.token_urlsafe(120))
    VALID[t] = {"cloud": "google"}; return t


def google_id_token(aud, email):
    now = int(time.time())
    t = jwt.encode({"iss": "https://accounts.google.com", "aud": aud, "azp": email, "email": email,
                    "email_verified": True, "sub": "1048" + "7" * 17, "iat": now, "exp": now + 3600},
                   ISSUER_KEY, algorithm="RS256", headers={"kid": "fake-google-1"})
    VALID[issue("google_id", t)] = {"cloud": "google-id", "aud": aud}; return t


def azure_access(resource):
    now = int(time.time())
    t = jwt.encode({"aud": resource.rstrip("/"), "iss": f"https://sts.windows.net/{TID}/", "iat": now, "nbf": now,
                    "exp": now + 3599, "tid": TID, "oid": OID, "appid": APPID, "idtyp": "app", "ver": "1.0",
                    "xms_mirid": "/subscriptions/0000/resourcegroups/silo/providers/Microsoft.ManagedIdentity/x"},
                   ISSUER_KEY, algorithm="RS256", headers={"kid": "fake-azure-1", "x5t": "fake-azure-1"})
    VALID[issue("azure_access", t)] = {"cloud": "azure"}; return t


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def reply(self, code, body, ctype="application/json"):
        b = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code); self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def bearer(self):
        a = self.headers.get("Authorization", "")
        return a[7:] if a.startswith("Bearer ") else None

    def note(self):
        SEEN_HEADERS.write(json.dumps({"host": self.headers.get("Host"), "path": self.path,
                                       "authorization": self.headers.get("Authorization")}) + "\n"); SEEN_HEADERS.flush()

    def do_GET(self): self.route("GET", b"")

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        self.route("POST", self.rfile.read(n))

    def route(self, method, body):
        self.note()
        host = (self.headers.get("Host") or "").split(":")[0]
        u = urlparse(self.path); q = {k: v[0] for k, v in parse_qs(u.query).items()}
        form = {k: v[0] for k, v in parse_qs(body.decode()).items()} if body else {}

        # Google ---------------------------------------------------------------------------------
        if host == "oauth2.googleapis.com" and u.path == "/token":
            g = form.get("grant_type")
            if g == "urn:ietf:params:oauth:grant-type:jwt-bearer":
                try:
                    claims = jwt.decode(form["assertion"], SA_PUB, algorithms=["RS256"], audience="https://oauth2.googleapis.com/token")
                except Exception as e:
                    return self.reply(400, {"error": "invalid_grant", "error_description": "Invalid JWT Signature."})
                if "target_audience" in claims:
                    return self.reply(200, {"id_token": google_id_token(claims["target_audience"], claims["iss"])})
                return self.reply(200, {"access_token": google_access(), "expires_in": 3599, "token_type": "Bearer"})
            if g == "refresh_token":
                if form.get("refresh_token") not in REFRESH["google"] or form.get("client_secret") != CREDS["google_client_secret"]:
                    return self.reply(400, {"error": "invalid_grant", "error_description": "Bad Request"})
                return self.reply(200, {"access_token": google_access(), "expires_in": 3599, "token_type": "Bearer",
                                        "scope": "openid https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email",
                                        "id_token": google_id_token(CREDS["google_client_id"], "polarzero@example.com")})
            return self.reply(400, {"error": "unsupported_grant_type"})
        if host == "storage.googleapis.com":
            if VALID.get(self.bearer(), {}).get("cloud") != "google":
                return self.reply(401, {"error": {"code": 401, "message": "Invalid Credentials"}})
            if "/o" in u.path:
                return self.reply(200, {"kind": "storage#objects", "items": [
                    {"kind": "storage#object", "name": "hello.txt", "bucket": "demo-bucket", "size": "5",
                     "updated": "2026-11-25T00:00:00.000Z", "timeCreated": "2026-11-25T00:00:00.000Z",
                     "id": "demo-bucket/hello.txt/1", "generation": "1", "metageneration": "1",
                     "storageClass": "STANDARD", "contentType": "text/plain", "md5Hash": "XUFAKrxLKna5cZ2REBfFkg==",
                     "crc32c": "mnG7TA==", "etag": "CAE="}]})
            return self.reply(200, {"kind": "storage#bucket", "name": "demo-bucket", "id": "demo-bucket",
                                    "location": "EU", "storageClass": "STANDARD", "timeCreated": "2026-11-25T00:00:00.000Z",
                                    "updated": "2026-11-25T00:00:00.000Z", "projectNumber": "123456789012",
                                    "metageneration": "1", "etag": "CAE="})
        if host.endswith(".a.run.app"):
            v = VALID.get(self.bearer(), {})
            if v.get("cloud") != "google-id" or v.get("aud") != f"https://{host}":
                return self.reply(401, b"Unauthorized\n", "text/plain")
            return self.reply(200, b"hello from cloud run\n", "text/plain")

        # Microsoft ------------------------------------------------------------------------------
        if host == "login.microsoftonline.com":
            if u.path.endswith("/discovery/instance"):
                return self.reply(200, {"tenant_discovery_endpoint": f"https://login.microsoftonline.com/{TID}/v2.0/.well-known/openid-configuration",
                                        "api-version": "1.1", "metadata": [{"preferred_network": "login.microsoftonline.com",
                                        "preferred_cache": "login.windows.net", "aliases": ["login.microsoftonline.com", "login.windows.net"]}]})
            if u.path.endswith("/.well-known/openid-configuration"):
                base = f"https://login.microsoftonline.com/{TID}"
                return self.reply(200, {"token_endpoint": f"{base}/oauth2/v2.0/token", "authorization_endpoint": f"{base}/oauth2/v2.0/authorize",
                                        "issuer": f"{base}/v2.0", "device_authorization_endpoint": f"{base}/oauth2/v2.0/devicecode",
                                        "end_session_endpoint": f"{base}/oauth2/v2.0/logout", "jwks_uri": f"{base}/discovery/v2.0/keys",
                                        "response_types_supported": ["code"], "subject_types_supported": ["pairwise"],
                                        "id_token_signing_alg_values_supported": ["RS256"], "scopes_supported": ["openid"]})
            if u.path.endswith("/oauth2/v2.0/token"):
                g = form.get("grant_type"); scope = form.get("scope", "")
                resource = next((s[:-len(".default")] for s in scope.split() if s.endswith(".default")), "https://management.azure.com/")
                if g == "client_credentials":
                    if form.get("client_secret") != CREDS["azure_client_secret"] or form.get("client_id") != APPID:
                        return self.reply(401, {"error": "invalid_client", "error_description": "AADSTS7000215: Invalid client secret provided."})
                    return self.reply(200, {"token_type": "Bearer", "expires_in": 3599, "ext_expires_in": 3599, "access_token": azure_access(resource)})
                if g == "refresh_token":
                    rt = form.get("refresh_token")
                    if rt not in REFRESH["azure"]:
                        return self.reply(400, {"error": "invalid_grant", "error_description": "AADSTS70000"})
                    REFRESH["azure"].discard(rt)  # Microsoft rotates refresh tokens: the old one is spent
                    new = issue("azure_refresh", "0.AXoA" + secrets.token_urlsafe(200)); REFRESH["azure"].add(new)
                    return self.reply(200, {"token_type": "Bearer", "scope": scope, "expires_in": 3599, "ext_expires_in": 3599,
                                            "access_token": azure_access(resource), "refresh_token": new})
                return self.reply(400, {"error": "unsupported_grant_type"})
        if host == "management.azure.com":
            if VALID.get(self.bearer(), {}).get("cloud") != "azure":
                return self.reply(401, {"error": {"code": "InvalidAuthenticationToken", "message": "The access token is invalid."}})
            if u.path.startswith("/subscriptions") and u.path.count("/") <= 1:
                return self.reply(200, {"value": [{"id": "/subscriptions/0000", "subscriptionId": "0000",
                                                   "tenantId": TID, "displayName": "Silo demo", "state": "Enabled",
                                                   "subscriptionPolicies": {}, "authorizationSource": "RoleBased"}],
                                        "count": {"type": "Total", "value": 1}})
            if "/resourcegroups" in u.path.lower():
                return self.reply(200, {"value": [{"id": "/subscriptions/0000/resourceGroups/silo", "name": "silo",
                                                   "type": "Microsoft.Resources/resourceGroups", "location": "westeurope",
                                                   "properties": {"provisioningState": "Succeeded"}}]})
            if u.path.startswith("/tenants"):
                return self.reply(200, {"value": [{"id": f"/tenants/{TID}", "tenantId": TID}]})
            return self.reply(404, {"error": {"code": "NotFound"}})
        if host == "collector.example":  # an outsider's server: keeps whatever it is sent
            open(f"{D}/collector.log", "a").write(json.dumps({"path": self.path, "headers": dict(self.headers),
                                                              "body": body.decode(errors="replace")}) + "\n")
            return self.reply(200, b"thanks\n", "text/plain")
        return self.reply(404, {"error": "no such host in the stand-in", "host": host})


ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
