"""#379 cloud/boundary.py: which name decides the swap of a placeholder for the real token, the
TLS server name or the Host header. Runs inside `unshare -n` (its own loopback)."""
import json, os, socket, ssl, subprocess, sys, time, threading, datetime, secrets
from http.server import BaseHTTPRequestHandler, HTTPServer
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
W = "/tmp/claude-0/cloud"; os.makedirs(W + "/state", exist_ok=True)
SRC = sys.argv[1]
import fcntl, struct
_s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); _i = struct.pack("16sh", b"lo", 0)
_f = struct.unpack("16sh", fcntl.ioctl(_s, 0x8913, _i))[1]; fcntl.ioctl(_s, 0x8914, struct.pack("16sh", b"lo", _f | 1))
k = ec.generate_private_key(ec.SECP256R1()); now = datetime.datetime.now(datetime.timezone.utc)
n = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "test ca")])
ca = (x509.CertificateBuilder().subject_name(n).issuer_name(n).public_key(k.public_key()).serial_number(1)
      .not_valid_before(now - datetime.timedelta(hours=1)).not_valid_after(now + datetime.timedelta(days=1))
      .add_extension(x509.BasicConstraints(ca=True, path_length=None), True)
      .add_extension(x509.KeyUsage(True, False, False, False, False, True, True, False, False), True)
      .add_extension(x509.SubjectKeyIdentifier.from_public_key(k.public_key()), False).sign(k, hashes.SHA256()))
open(W + "/ca.pem", "wb").write(ca.public_bytes(serialization.Encoding.PEM))
open(W + "/ca.key", "wb").write(k.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
REAL = "ya29.c.REAL-" + secrets.token_hex(8)
cfg = {"mode": {"metadata": "placeholder", "exchange": "intercept", "jwt": "jwt"}, "upstream_port": 9000,
       "mac": {"google_access_real": REAL}, "placeholders": [{"placeholder": "ya29.c.siloPH_test", "real_key": "google_access_real",
       "hosts": ["*.googleapis.com"], "kind": "google_access"}], "guest_bound_log": W + "/guest.log", "record": W + "/record.jsonl",
       "ca_key": W + "/ca.key", "ca_cert": W + "/ca.pem", "state_dir": W + "/state", "metadata_ip": "127.0.0.1", "tls_port": 443}
json.dump(cfg, open(W + "/cfg.json", "w"))
seen = []
class Up(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        seen.append((self.headers.get("Host"), self.headers.get("Authorization")))
        self.send_response(200); self.send_header("Content-Length", "2"); self.end_headers(); self.wfile.write(b"ok")
threading.Thread(target=HTTPServer(("127.0.0.1", 9000), Up).serve_forever, daemon=True).start()
b = subprocess.Popen([sys.executable, SRC, W + "/cfg.json"], stdout=subprocess.PIPE, stderr=open(W + "/b.err", "w"))
b.stdout.readline()
def get(sni, host):
    ctx = ssl.create_default_context(cafile=W + "/ca.pem")
    s = ctx.wrap_socket(socket.create_connection(("127.0.0.1", 443)), server_hostname=sni)
    s.sendall(f"GET /x HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer ya29.c.siloPH_test\r\nConnection: close\r\n\r\n".encode())
    s.recv(4096); s.close()
for sni, host in [("storage.googleapis.com", "storage.googleapis.com"), ("collector.example", "storage.googleapis.com"),
                  ("storage.googleapis.com", "collector.example")]:
    get(sni, host)
    h, a = seen[-1]
    print(f"TLS name {sni:24} Host {host:24} -> upstream got {'the REAL token' if REAL in a else 'the placeholder'}")
b.kill()
