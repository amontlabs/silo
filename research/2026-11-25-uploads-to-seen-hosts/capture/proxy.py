"""A recording HTTPS proxy that never reaches the internet.

It ends TLS with an authority it makes itself, writes every request it sees to a JSON-lines file
with the bytes each way, and answers from fakes good enough for the real clients to send their
real uploads:

- github.com: Git smart HTTP, served by the local `git http-backend` over bare repositories
  in STATE/git/<owner>/<repo>.git. It asks for a credential (401) before a push, as GitHub does.
- api.github.com, uploads.github.com: gists, issues, comments, releases and their assets.
- ghcr.io: the OCI distribution API with GitHub's token exchange.
- storage.googleapis.com: the JSON API (simple, multipart and resumable uploads) and the XML API.
- oauth2.googleapis.com/tokeninfo and api.github.com/user answer who a token belongs to, from
  the TOKENS table below, so a rule that asks the host "whose credential is this" can be tested.

Started from #350's `research/2026-11-24-gap-fixes/prototypes/resource-scopes/record_proxy.py`.

    python3 proxy.py --port 8899 --state DIR --out capture.jsonl
"""

import argparse
import base64
import datetime
import gzip
import hashlib
import json
import os
import socket
import ssl
import subprocess
import threading
import uuid

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID

LOCK = threading.Lock()

# Who each test credential belongs to, as the host would answer.
TOKENS = {
    "ghp_polarzero_test": "polarzero",
    "ghp_mallory_test": "mallory",
    "ya29.polarzero_test": "polarzero@example.com",
    "ya29.mallory_test": "mallory@example.net",
}


def make_ca(state):
    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "uploads-to-seen-hosts test CA")])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
            .public_key(key.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(days=1))
            .not_valid_after(now + datetime.timedelta(days=2))
            .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
            .add_extension(x509.SubjectKeyIdentifier.from_public_key(key.public_key()), critical=False)
            .add_extension(x509.KeyUsage(True, False, False, False, False, True, True, False, False), critical=True)
            .sign(key, hashes.SHA256()))
    with open(os.path.join(state, "ca.pem"), "wb") as f:
        f.write(cert.public_bytes(serialization.Encoding.PEM))
    return key, cert


def leaf_context(host, ca_key, ca_cert, state, cache={}):
    if host in cache:
        return cache[host]
    key = ec.generate_private_key(ec.SECP256R1())
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (x509.CertificateBuilder()
            .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, host)]))
            .issuer_name(ca_cert.subject).public_key(key.public_key())
            .serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(days=1))
            .not_valid_after(now + datetime.timedelta(days=1))
            .add_extension(x509.SubjectAlternativeName([x509.DNSName(host)]), critical=False)
            .add_extension(x509.AuthorityKeyIdentifier.from_issuer_public_key(ca_key.public_key()), critical=False)
            .add_extension(x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
            .sign(ca_key, hashes.SHA256()))
    path = os.path.join(state, f"leaf-{host}.pem")
    with open(path, "wb") as f:
        f.write(cert.public_bytes(serialization.Encoding.PEM))
        f.write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                  serialization.NoEncryption()))
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(path)
    ctx.set_alpn_protocols(["http/1.1"])
    cache[host] = ctx
    return ctx


class Conn:
    def __init__(self, sock):
        self.sock, self.buf, self.read_total = sock, b"", 0

    def _recv(self):
        chunk = self.sock.recv(65536)
        self.read_total += len(chunk)
        return chunk

    def read_until(self, marker):
        while marker not in self.buf:
            chunk = self._recv()
            if not chunk:
                return None
            self.buf += chunk
        head, self.buf = self.buf.split(marker, 1)
        return head

    def read_n(self, n):
        while len(self.buf) < n:
            chunk = self._recv()
            if not chunk:
                break
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def read_request(self):
        head = self.read_until(b"\r\n\r\n")
        if head is None:
            return None
        lines = head.decode("latin-1").split("\r\n")
        method, target, _ = lines[0].split(" ", 2)
        headers = []
        for line in lines[1:]:
            k, v = line.split(":", 1)
            headers.append((k.strip(), v.strip()))
        h = {k.lower(): v for k, v in headers}
        body = b""
        if h.get("expect", "").lower() == "100-continue":
            self.sock.sendall(b"HTTP/1.1 100 Continue\r\n\r\n")
        if h.get("transfer-encoding", "").lower() == "chunked":
            while True:
                size = int(self.read_until(b"\r\n").split(b";")[0], 16)
                if size == 0:
                    self.read_until(b"\r\n")
                    break
                body += self.read_n(size)
                self.read_n(2)
        elif "content-length" in h:
            body = self.read_n(int(h["content-length"]))
        return method, target, headers, body


def bearer_or_basic(headers):
    """The token a request carries, from Bearer, token or Basic (password, else user)."""
    for k, v in headers:
        if k.lower() != "authorization":
            continue
        scheme, _, cred = v.partition(" ")
        if scheme.lower() in ("bearer", "token"):
            return cred
        if scheme.lower() == "basic":
            user, _, pw = base64.b64decode(cred).decode("latin-1").partition(":")
            return pw or user
    return None


def git_backend(state, method, path, query, headers, body):
    h = {k.lower(): v for k, v in headers}
    if h.get("content-encoding") == "gzip":
        body = gzip.decompress(body)
    env = {
        "GIT_PROJECT_ROOT": os.path.join(state, "git"), "GIT_HTTP_EXPORT_ALL": "1",
        "PATH_INFO": path, "QUERY_STRING": query, "REQUEST_METHOD": method,
        "CONTENT_TYPE": h.get("content-type", ""), "CONTENT_LENGTH": str(len(body)),
        "REMOTE_USER": "someone", "REMOTE_ADDR": "127.0.0.1", "PATH": os.environ["PATH"],
        "GIT_PROTOCOL": h.get("git-protocol", ""),
    }
    out = subprocess.run(["/usr/lib/git-core/git-http-backend"], input=body, env=env,
                         capture_output=True).stdout
    head, _, payload = out.partition(b"\r\n\r\n")
    status, rh = 200, []
    for line in head.decode("latin-1").split("\r\n"):
        if not line:
            continue
        k, v = line.split(":", 1)
        if k.lower() == "status":
            status = int(v.strip().split()[0])
        else:
            rh.append((k, v.strip()))
    return status, rh, payload


UPLOADS = {}  # OCI and GCS resumable sessions


def github_api(host, method, path, query, headers, body):
    js = [("Content-Type", "application/json"), ("X-OAuth-Scopes", "repo, gist, write:packages")]
    tok = bearer_or_basic(headers)
    login = TOKENS.get(tok or "", "unknown")
    parts = path.strip("/").split("/")
    if path == "/user":
        if tok is None:
            return 401, js, b'{"message":"Requires authentication"}'
        return 200, js, json.dumps({"login": login, "id": abs(hash(login)) % 10**8}).encode()
    if path.startswith("/graphql"):
        doc = json.loads(body or b"{}")
        q, v = doc.get("query", ""), doc.get("variables") or {}
        o, r = v.get("owner", login), v.get("repo") or v.get("name") or "app"
        # Node ids that say what they belong to, so a test can play GitHub's answer to nodes(ids:).
        issue = {"__typename": "Issue", "id": f"I_{o}_{r}_1", "url": f"https://github.com/{o}/{r}/issues/1"}
        if "createIssue" in q:
            data = {"createIssue": {"issue": {"url": issue["url"]}}}
        elif "addComment" in q:
            data = {"addComment": {"commentEdge": {"node": {"url": issue["url"] + "#c"}}}}
        elif "release(" in q:
            data = {"repository": {"release": {"databaseId": 7, "isDraft": False}}}
        elif "issueOrPullRequest" in q:
            data = {"repository": {"hasIssuesEnabled": True, "issue": issue}}
        elif q.strip().startswith("query { viewer") or "UserCurrent" in q:
            data = {"viewer": {"login": login}}
        else:
            data = {"repository": {"id": f"R_{o}_{r}", "name": r, "owner": {"login": o},
                                   "hasIssuesEnabled": True, "viewerPermission": "ADMIN",
                                   "defaultBranchRef": {"name": "main"}, "isPrivate": False}}
        return 200, js, json.dumps({"data": data}).encode()
    if method == "POST" and path == "/gists":
        return 201, js, json.dumps({"id": "g1", "html_url": "https://gist.github.com/g1", "files": {},
                                    "owner": {"login": login}}).encode()
    if method == "GET" and "/releases/tags/" in path:
        o, r = parts[1], parts[2]
        return 200, js, json.dumps({"id": 7, "tag_name": parts[-1],
                                    "upload_url": f"https://uploads.github.com/repos/{o}/{r}/releases/7/assets{{?name,label}}",
                                    "assets": []}).encode()
    if host == "uploads.github.com" and method == "POST":
        return 201, js, json.dumps({"id": 11, "name": query, "state": "uploaded",
                                    "browser_download_url": "https://github.com/x"}).encode()
    if len(parts) == 3 and parts[0] == "repos" and method == "GET":
        return 200, js, json.dumps({"id": 2, "node_id": "R_x", "full_name": f"{parts[1]}/{parts[2]}",
                                    "name": parts[2], "owner": {"login": parts[1]}, "private": False,
                                    "default_branch": "main", "has_issues": True}).encode()
    if method == "GET":
        return 200, js, b"[]" if path.endswith("s") else b"{}"
    return 201, js, json.dumps({"id": 9, "number": 1, "html_url": "https://github.com/x/y/issues/1",
                                "url": "https://api.github.com/x"}).encode()


def ghcr(method, path, query, headers, body):
    js = [("Content-Type", "application/json"), ("Docker-Distribution-API-Version", "registry/2.0")]
    if path == "/token":
        tok = bearer_or_basic(headers)
        if tok is None and body:  # docker's OAuth form: password=...
            form = dict(p.split("=", 1) for p in body.decode().split("&") if "=" in p)
            tok = form.get("password") or form.get("refresh_token")
        # GHCR answers with an opaque token; this fake makes it carry who asked and for what.
        opaque = base64.b64encode(json.dumps({"who": TOKENS.get(tok or "", "anonymous"),
                                              "q": query}).encode()).decode()
        return 200, js, json.dumps({"token": opaque, "access_token": opaque}).encode()
    has_bearer = any(k.lower() == "authorization" and v.lower().startswith("bearer") for k, v in headers)
    if not has_bearer:
        name = "/".join(path.split("/")[2:-2]) or "x/y"
        return 401, js + [("WWW-Authenticate",
                           f'Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:{name}:pull,push"')], \
            b'{"errors":[{"code":"UNAUTHORIZED"}]}'
    if path == "/v2/":
        return 200, js, b"{}"
    if "/blobs/uploads/" in path:
        if method == "POST":
            u = str(uuid.uuid4())
            name = path.split("/v2/")[1].split("/blobs/")[0]
            return 202, js + [("Location", f"/v2/{name}/blobs/uploads/{u}"), ("Range", "0-0"),
                              ("Docker-Upload-UUID", u), ("Content-Length", "0")], b""
        if method == "PATCH":
            return 202, js + [("Location", path), ("Range", f"0-{max(len(body) - 1, 0)}")], b""
        if method == "PUT":
            d = dict(p.split("=", 1) for p in query.split("&") if "=" in p).get("digest", "")
            return 201, js + [("Location", path.split("/uploads/")[0] + "/" + d),
                              ("Docker-Content-Digest", d.replace("%3A", ":"))], b""
    if "/blobs/" in path and method in ("HEAD", "GET"):
        return 404, js, b'{"errors":[{"code":"BLOB_UNKNOWN"}]}'
    if "/manifests/" in path:
        if method in ("HEAD", "GET"):
            return 404, js, b'{"errors":[{"code":"MANIFEST_UNKNOWN"}]}'
        d = "sha256:" + hashlib.sha256(body).hexdigest()
        return 201, js + [("Docker-Content-Digest", d), ("Location", path)], b""
    return 404, js, b"{}"


def media_of(headers, body):
    """The object's bytes: the last part of a multipart upload, else the whole body."""
    ct = dict((k.lower(), v) for k, v in headers).get("content-type", "")
    if "boundary=" not in ct:
        return body
    b = ct.split("boundary=")[1].strip('"').encode()
    parts = [p for p in body.split(b"--" + b) if p.strip() and not p.startswith(b"--")]
    if not parts:
        return body
    last = parts[-1]
    sep = b"\r\n\r\n" if b"\r\n\r\n" in last else b"\n\n"
    if sep not in last:
        return body
    data = last.split(sep, 1)[1]
    return data[:-2] if data.endswith(b"\r\n") else data[:-1] if data.endswith(b"\n") else data


def gcs(host, method, path, query, headers, body):
    js = [("Content-Type", "application/json")]
    media = media_of(headers, body)
    q = dict(p.split("=", 1) for p in query.split("&") if "=" in p)
    if host == "oauth2.googleapis.com" and path == "/tokeninfo":
        tok = q.get("access_token")
        if tok not in TOKENS:
            return 400, js, b'{"error":"invalid_token"}'
        return 200, js, json.dumps({"email": TOKENS[tok], "scope": "https://www.googleapis.com/auth/devstorage.read_write"}).encode()
    obj = {"kind": "storage#object", "name": q.get("name", "o"), "bucket": "b", "generation": "1",
           "metageneration": "1", "size": str(len(media)), "md5Hash": base64.b64encode(hashlib.md5(media).digest()).decode(), "contentType": "application/octet-stream", "etag": "x",
           "timeCreated": "2026-11-25T00:00:00Z", "updated": "2026-11-25T00:00:00Z"}
    if path.startswith("/resumable/"):
        path = path[len("/resumable"):]
    if path.startswith("/upload/storage/v1/b/"):
        bucket = path.split("/")[5]
        obj["bucket"] = bucket
        if q.get("uploadType") == "resumable" and "upload_id" not in q:
            uid = uuid.uuid4().hex
            meta = json.loads(body or b"{}") if body.strip().startswith(b"{") else {}
            UPLOADS[uid] = meta.get("name", q.get("name", "o"))
            return 200, js + [("Location", f"https://{host}/upload/storage/v1/b/{bucket}/o?uploadType=resumable&upload_id={uid}"),
                              ("X-GUploader-UploadID", uid)], b""
        if "upload_id" in q:
            obj["name"] = UPLOADS.get(q["upload_id"], "o")
        if q.get("uploadType") == "multipart":
            for line in body.split(b"\r\n"):
                if line.startswith(b"{") and b'"name"' in line:
                    obj["name"] = json.loads(line).get("name", "o")
                    break
        return 200, js, json.dumps(obj).encode()
    if path.startswith("/storage/v1/b/"):
        parts = path.split("/")
        if len(parts) == 5:  # bucket metadata
            return 200, js, json.dumps({"kind": "storage#bucket", "name": parts[4], "projectNumber": "1",
                                        "location": "US"}).encode()
        if method == "GET" and len(parts) >= 7:
            return 404, js, b'{"error":{"code":404,"message":"No such object"}}'
        if method == "GET":
            return 200, js, b'{"kind":"storage#objects","items":[]}'
        return 200, js, json.dumps(obj).encode()
    # XML API: path-style /bucket/object, or virtual-host style bucket.storage.googleapis.com
    if method in ("PUT", "POST"):
        return 200, [("ETag", '"x"'), ("x-goog-generation", "1")], b""
    return 404, [("Content-Type", "application/xml")], b"<Error><Code>NoSuchKey</Code></Error>"


def answer(state, host, method, path, query, headers, body, no_thin):
    if host == "github.com" or host == "gist.github.com":
        if not any(k.lower() == "authorization" for k, v in headers) and (
                "git-receive-pack" in path or "git-receive-pack" in query):
            return 401, [("WWW-Authenticate", 'Basic realm="GitHub"')], b""
        p = path if ".git/" in path else path.replace("/info/", ".git/info/", 1).replace("/git-", ".git/git-", 1)
        if host == "gist.github.com":
            p = "/gists" + p
        status, rh, payload = git_backend(state, method, p, query, headers, body)
        return status, rh, payload
    if host in ("api.github.com", "uploads.github.com"):
        return github_api(host, method, path, query, headers, body)
    if host == "ghcr.io":
        return ghcr(method, path, query, headers, body)
    if host.endswith("googleapis.com"):
        return gcs(host, method, path, query, headers, body)
    return 502, [("Content-Type", "text/plain")], b"not faked"


def handle(client, args, ca_key, ca_cert):
    conn = Conn(client)
    req = conn.read_request()
    if req is None or req[0] != "CONNECT":
        client.close()
        return
    host = req[1].split(":")[0]
    client.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
    try:
        tls = leaf_context(host, ca_key, ca_cert, args.state).wrap_socket(client, server_side=True)
    except (ssl.SSLError, OSError):
        return
    conn = Conn(tls)
    cid = uuid.uuid4().hex[:8]
    while True:
        try:
            req = conn.read_request()
        except (ssl.SSLError, ConnectionError, ValueError):
            break
        if req is None:
            break
        method, target, headers, body = req
        path, _, query = target.partition("?")
        try:
            status, rh, payload = answer(args.state, host, method, path, query, headers, body, args.no_thin)
        except Exception as e:  # the request is still recorded; the client gets a 500
            status, rh, payload = 500, [("Content-Type", "text/plain")], repr(e).encode()
        if method == "HEAD":
            head = f"HTTP/1.1 {status} X\r\n" + "".join(f"{k}: {v}\r\n" for k, v in rh)
            if not any(k.lower() == "content-length" for k, _ in rh):
                head += "Content-Length: 0\r\n"
            out = head.encode() + b"\r\n"
        else:
            rh = [(k, v) for k, v in rh if k.lower() != "content-length"]
            head = f"HTTP/1.1 {status} X\r\n" + "".join(f"{k}: {v}\r\n" for k, v in rh)
            out = head.encode() + f"Content-Length: {len(payload)}\r\n\r\n".encode() + payload
        rec = {"case": os.environ.get("CASE_FILE") and open(os.environ["CASE_FILE"]).read().strip(),
               "conn": cid, "method": method, "host": host, "target": target, "headers": headers,
               "body_b64": base64.b64encode(body).decode() if len(body) < 300000 else None,
               "body_len": len(body), "bytes_up": len(f"{method} {target} HTTP/1.1\r\n") + sum(len(k) + len(v) + 4 for k, v in headers) + 2 + len(body),
               "status": status, "bytes_down": len(out),
               "answer": payload.decode("latin-1") if host == "ghcr.io" and path == "/token" else None}
        with LOCK, open(args.out, "a") as f:
            f.write(json.dumps(rec) + "\n")
        try:
            tls.sendall(out)
        except (ssl.SSLError, ConnectionError):
            break
    tls.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8899)
    ap.add_argument("--state", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-thin", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.join(args.state, "git"), exist_ok=True)
    ca_key, ca_cert = make_ca(args.state)
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", args.port))
    srv.listen(64)
    print("ready", flush=True)
    while True:
        c, _ = srv.accept()
        threading.Thread(target=handle, args=(c, args, ca_key, ca_cert), daemon=True).start()


if __name__ == "__main__":
    main()
