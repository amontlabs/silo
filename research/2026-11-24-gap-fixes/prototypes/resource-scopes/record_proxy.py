"""A recording HTTPS proxy that never reaches the internet.

It terminates TLS with a CA it makes itself, writes every request it sees to a JSON-lines file,
and answers from fakes: Git smart HTTP is served by the local `git http-backend` over bare
repositories in a folder, and api.github.com, uploads.github.com, the npm registry, PyPI's upload
host and S3 get canned answers just good enough for `gh`, `npm`, `twine` and boto3 to send their
real requests. Used to build the corpus of real client traffic for the classifier.

    python3 record_proxy.py --port 8899 --state DIR --out corpus.jsonl

DIR receives ca.pem (give it to the clients) and holds the bare repositories under DIR/git/<owner>/<repo>.git.
"""

import argparse
import base64
import datetime
import gzip
import json
import os
import socket
import ssl
import subprocess
import threading

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID

LOCK = threading.Lock()


def make_ca(state):
    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "resource-scopes test CA")])
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
        self.sock, self.buf = sock, b""

    def read_until(self, marker):
        while marker not in self.buf:
            chunk = self.sock.recv(65536)
            if not chunk:
                return None
            self.buf += chunk
        head, self.buf = self.buf.split(marker, 1)
        return head

    def read_n(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
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


def git_backend(state, method, path, query, headers, body):
    h = {k.lower(): v for k, v in headers}
    if h.get("content-encoding") == "gzip":
        body = gzip.decompress(body)
    env = {
        "GIT_PROJECT_ROOT": os.path.join(state, "git"), "GIT_HTTP_EXPORT_ALL": "1",
        "PATH_INFO": path, "QUERY_STRING": query, "REQUEST_METHOD": method,
        "CONTENT_TYPE": h.get("content-type", ""), "CONTENT_LENGTH": str(len(body)),
        "REMOTE_USER": "victim", "REMOTE_ADDR": "127.0.0.1", "PATH": os.environ["PATH"],
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


GRAPHQL_FAKES = {
    "UserCurrent": {"viewer": {"login": "victim", "id": "U_kgDOAAAAAQ"}},
    "RepositoryInfo": {"repository": {"id": "R_kgDOAAAAAg", "name": "app", "owner": {"login": "victim"},
                                       "hasIssuesEnabled": True, "viewerPermission": "ADMIN",
                                       "defaultBranchRef": {"name": "main"}, "isPrivate": True}},
}


def fake_answer(host, method, path, headers, body):
    js = [("Content-Type", "application/json")]
    if host == "api.github.com" or host == "uploads.github.com":
        extra = js + [("X-OAuth-Scopes", "repo, read:org, gist, workflow")]
        if path.startswith("/graphql"):
            try:
                q = json.loads(body or b"{}").get("query", "")
            except ValueError:
                q = ""
            name = q.split("(")[0].split("{")[0].replace("query", "").replace("mutation", "").strip()
            data = dict(GRAPHQL_FAKES["UserCurrent"], **GRAPHQL_FAKES["RepositoryInfo"])
            if q.lstrip().startswith("mutation"):
                data = {"createRepository": {"repository": {"id": "R_x", "url": "https://github.com/victim/x",
                                                            "name": "x", "owner": {"login": "victim"}}}}
            return 200, extra, json.dumps({"data": data}).encode()
        if path == "/user":
            return 200, extra, json.dumps({"login": "victim", "id": 1, "node_id": "U_kgDOAAAAAQ"}).encode()
        if method == "POST" and path == "/gists":
            return 201, extra, json.dumps({"id": "abc", "html_url": "https://gist.github.com/abc",
                                          "files": {}, "owner": {"login": "victim"}}).encode()
        if method == "GET" and "/releases/tags/" in path:
            return 200, extra, json.dumps({"id": 7, "upload_url": "https://uploads.github.com/repos/victim/app/releases/7/assets{?name,label}",
                                          "assets": [], "tag_name": "v1"}).encode()
        if path.startswith("/repos/victim/app") and method == "GET" and path.count("/") == 3:
            return 200, extra, json.dumps({"id": 2, "node_id": "R_kgDOAAAAAg", "full_name": "victim/app",
                                          "name": "app", "owner": {"login": "victim"}, "private": True,
                                          "default_branch": "main"}).encode()
        if "/secrets/public-key" in path:
            return 200, extra, json.dumps({"key_id": "1", "key": base64.b64encode(b"\x01" * 32).decode()}).encode()
        if method == "GET":
            return 200, extra, b"[]" if path.endswith("s") else b"{}"
        return 201, extra, json.dumps({"id": 9, "number": 1, "html_url": "https://github.com/victim/app/issues/1",
                                       "url": "https://api.github.com/x"}).encode()
    if host == "registry.npmjs.org":
        if method == "PUT":
            return 200, js, b'{"ok":true}'
        return 404, js, b'{"error":"not found"}'
    if host == "upload.pypi.org":
        return 200, [("Content-Type", "text/plain")], b"OK"
    if host.endswith("amazonaws.com"):
        return 200, [("ETag", '"x"'), ("Content-Type", "application/xml")], b""
    return 502, [("Content-Type", "text/plain")], b"not faked"


def add_no_thin(adv):
    """Adds the no-thin capability to a receive-pack advertisement, as the boundary would."""
    out, pos, done = b"", 0, False
    while pos < len(adv):
        n = int(adv[pos:pos + 4], 16)
        if n == 0:
            out += b"0000"; pos += 4
            continue
        line = adv[pos + 4:pos + n]; pos += n
        if not done and b"\0" in line:
            line = line.replace(b"\0", b"\0no-thin ", 1); done = True
        out += b"%04x" % (len(line) + 4) + line
    return out


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
    except ssl.SSLError:
        return
    conn = Conn(tls)
    while True:
        try:
            req = conn.read_request()
        except (ssl.SSLError, ConnectionError, ValueError):
            break
        if req is None:
            break
        method, target, headers, body = req
        path, _, query = target.partition("?")
        rec = {"method": method, "host": host,
               "target": target, "headers": headers,
               "body_b64": base64.b64encode(body).decode() if len(body) < 200000 else None,
               "body_len": len(body)}
        with LOCK, open(args.out, "a") as f:
            f.write(json.dumps(rec) + "\n")
        bare = path.split("/")
        if host == "github.com" and (path.endswith("/info/refs") or path.endswith("git-upload-pack")
                                     or path.endswith("git-receive-pack")):
            status, rh, payload = git_backend(args.state, method, path if ".git/" in path else
                                              path.replace("/info/", ".git/info/", 1).replace("/git-", ".git/git-", 1),
                                              query, headers, body)
            if args.no_thin and "service=git-receive-pack" in query and method == "GET":
                payload = add_no_thin(payload)
        else:
            status, rh, payload = fake_answer(host, method, path, headers, body)
        head = f"HTTP/1.1 {status} X\r\n" + "".join(f"{k}: {v}\r\n" for k, v in rh)
        head += f"Content-Length: {len(payload)}\r\n\r\n"
        try:
            tls.sendall(head.encode() + payload)
        except (ssl.SSLError, ConnectionError):
            break
    tls.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8899)
    ap.add_argument("--state", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-thin", action="store_true", help="add no-thin to receive-pack advertisements")
    args = ap.parse_args()
    os.makedirs(os.path.join(args.state, "git"), exist_ok=True)
    ca_key, ca_cert = make_ca(args.state)
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", args.port))
    srv.listen(64)
    print("ready", flush=True)
    while True:
        c, _ = srv.accept()
        threading.Thread(target=handle, args=(c, args, ca_key, ca_cert), daemon=True).start()


if __name__ == "__main__":
    main()
