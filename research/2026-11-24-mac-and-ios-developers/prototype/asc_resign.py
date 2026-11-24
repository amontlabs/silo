#!/usr/bin/env python3
"""Re-signing an App Store Connect token at the boundary: a prototype on Linux.

App Store Connect, notarytool, altool, Transporter, fastlane and `xcodebuild
-authenticationKeyPath` all authenticate the same way: the program reads a .p8
key (P-256) and signs a short JWT (ES256) inside the computer. The key itself is
never sent, so a placeholder swapped into the request, as `secret-placeholder`
answers for an API key, cannot work.

What this shows: the computer holds a placeholder .p8, a real P-256 key that is
worthless anywhere else. Programs sign with it as usual. The boundary, which
already ends the encrypted connection to the hosts that get a secret, checks the
token was signed by the placeholder, checks its claims, and signs the same
claims again with the real key, under the real key id and issuer. The real key
never enters the computer, and the agent never sees a token signed by it.

Run: python3 asc_resign.py   (needs `cryptography`; three local HTTP servers,
no network). It prints one line per case and exits 1 if any case is wrong.
"""
import base64, json, threading, time, urllib.request, urllib.error
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric.utils import (
    decode_dss_signature, encode_dss_signature)
from cryptography.exceptions import InvalidSignature

AUD = "appstoreconnect-v1"
MAX_LIFE = 20 * 60          # Apple refuses longer tokens for most keys
ALLOWED_HOSTS = {"api.appstoreconnect.apple.com"}


def b64u(b): return base64.urlsafe_b64encode(b).rstrip(b"=").decode()
def unb64u(s): return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def sign(key, header, claims):
    h = b64u(json.dumps(header, separators=(",", ":")).encode())
    c = b64u(json.dumps(claims, separators=(",", ":")).encode())
    der = key.sign(f"{h}.{c}".encode(), ec.ECDSA(hashes.SHA256()))
    r, s = decode_dss_signature(der)
    return f"{h}.{c}.{b64u(r.to_bytes(32, 'big') + s.to_bytes(32, 'big'))}"


def verify(pub, token):
    h, c, s = token.split(".")
    raw = unb64u(s)
    der = encode_dss_signature(int.from_bytes(raw[:32], "big"), int.from_bytes(raw[32:], "big"))
    pub.verify(der, f"{h}.{c}".encode(), ec.ECDSA(hashes.SHA256()))
    return json.loads(unb64u(h)), json.loads(unb64u(c))


# The real key lives outside the computer. The placeholder is what the computer gets.
REAL = dict(key=ec.generate_private_key(ec.SECP256R1()), kid="REALKEY123", iss="69a6de70-real-issuer")
PH = dict(key=ec.generate_private_key(ec.SECP256R1()), kid="SILOPH0001", iss="00000000-silo-placeholder")


def resign(token, host, method, path, now):
    """The boundary's whole job. Returns (new token, None) or (None, reason)."""
    if host not in ALLOWED_HOSTS:
        return None, "host not allowed for this secret"
    try:
        header, claims = verify(PH["key"].public_key(), token)
    except (InvalidSignature, ValueError, IndexError):
        return None, "not signed by the placeholder"
    if header.get("alg") != "ES256" or header.get("kid") != PH["kid"]:
        return None, "wrong algorithm or key id"
    if claims.get("iss") != PH["iss"] or claims.get("aud") != AUD:
        return None, "wrong issuer or audience"
    iat, exp = claims.get("iat", now), claims.get("exp", 0)
    if exp - iat > MAX_LIFE or exp - now > MAX_LIFE or iat > now + 60:
        return None, "lifetime longer than 20 minutes"
    out = dict(claims, iss=REAL["iss"], iat=now, exp=min(exp, now + MAX_LIFE))
    # Narrowing: Apple documents `scope` for GET only ("The HTTP GET method",
    # path, optional query). A GET token Apple gets is good for this one call.
    # Other methods go unscoped, so the boundary's own rule on method and path
    # is what limits them (see the README).
    if method == "GET":
        out["scope"] = [f"GET {path}"]
    return sign(REAL["key"], dict(header, kid=REAL["kid"]), out), None


class FakeASC(BaseHTTPRequestHandler):
    """Stands for Apple: trusts only the real key."""
    def do_GET(self):
        auth = self.headers.get("Authorization", "")
        try:
            header, claims = verify(REAL["key"].public_key(), auth.removeprefix("Bearer "))
            ok = header["kid"] == REAL["kid"] and claims["iss"] == REAL["iss"] \
                and f"GET {self.path}" in claims.get("scope", [f"GET {self.path}"])
        except Exception:
            ok = False
        self.send_response(200 if ok else 401)
        self.end_headers()
        self.wfile.write(b'{"data":[]}' if ok else b'{"errors":[{"status":"401"}]}')
    def log_message(self, *a): pass


def make_boundary(asc_port, log):
    class Boundary(BaseHTTPRequestHandler):
        """Stands for Silo's network stack after it has ended TLS to the host."""
        def do_GET(self):
            host = self.headers.get("X-Host", "api.appstoreconnect.apple.com")
            tok = self.headers.get("Authorization", "").removeprefix("Bearer ")
            new, why = resign(tok, host, "GET", self.path, int(time.time()))
            log.append(why or "re-signed")
            if not new:
                self.send_response(403); self.end_headers()
                self.wfile.write(json.dumps({"refused": why}).encode()); return
            req = urllib.request.Request(f"http://127.0.0.1:{asc_port}{self.path}",
                                         headers={"Authorization": f"Bearer {new}"})
            try:
                r = urllib.request.urlopen(req); code, body = r.status, r.read()
            except urllib.error.HTTPError as e:
                code, body = e.code, e.read()
            self.send_response(code); self.end_headers(); self.wfile.write(body)
        def log_message(self, *a): pass
    return Boundary


def serve(handler):
    s = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=s.serve_forever, daemon=True).start()
    return s


def call(port, token, host=None):
    h = {"Authorization": f"Bearer {token}"}
    if host: h["X-Host"] = host
    try:
        return urllib.request.urlopen(urllib.request.Request(
            f"http://127.0.0.1:{port}/v1/apps", headers=h)).status
    except urllib.error.HTTPError as e:
        return e.code


def tool_token(key, kid, iss, life=1200, aud=AUD):
    """What notarytool, altool and fastlane put on the wire."""
    now = int(time.time())
    return sign(key, {"alg": "ES256", "kid": kid, "typ": "JWT"},
                {"iss": iss, "iat": now, "exp": now + life, "aud": aud})


def main():
    asc = serve(FakeASC)
    log = []
    bnd = serve(make_boundary(asc.server_port, log))
    a, b = asc.server_port, bnd.server_port
    ph = lambda **kw: tool_token(PH["key"], PH["kid"], PH["iss"], **kw)
    cases = [
        ("program in the computer, through the boundary", call(b, ph()), 200),
        ("placeholder token sent straight to Apple", call(a, ph()), 401),
        ("placeholder token to another host", call(b, ph(), host="paste.example"), 403),
        ("token the agent made with a key of its own", call(b, tool_token(
            ec.generate_private_key(ec.SECP256R1()), PH["kid"], PH["iss"])), 403),
        ("token asking for a day", call(b, ph(life=86400)), 403),
        ("token for another audience", call(b, ph(aud="something-else")), 403),
    ]
    bad = 0
    for name, got, want in cases:
        bad += got != want
        print(f"{'ok ' if got == want else 'BAD'} {got} (want {want})  {name}")
    print("boundary log:", log)
    asc.shutdown(); bnd.shutdown()
    raise SystemExit(1 if bad else 0)


if __name__ == "__main__":
    main()
