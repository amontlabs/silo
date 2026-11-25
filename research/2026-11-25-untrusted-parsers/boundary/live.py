"""Sends one raw request to a real provider host through this container's HTTPS proxy, with no
credential, and prints the status line and the error message. A message about the key shows the
host read a header the test placed; 'header is required' shows it did not."""
import socket, ssl, sys, os
host = sys.argv[1]; raw = sys.argv[2].encode().decode('unicode_escape').encode('latin1')
p = os.environ["HTTPS_PROXY"].split("//")[1].rstrip("/"); ph, pp = p.split(":")
s = socket.create_connection((ph, int(pp)))
s.sendall(f"CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n".encode())
r = b""
while b"\r\n\r\n" not in r: r += s.recv(4096)
ctx = ssl.create_default_context(cafile="/root/.ccr/ca-bundle.crt")
t = ctx.wrap_socket(s, server_hostname=host)
t.sendall(raw); t.settimeout(10); out = b""
try:
    while True:
        d = t.recv(65536)
        if not d: break
        out += d
except Exception: pass
first = out.split(b"\r\n", 1)[0].decode(errors="replace")
msg = out.split(b"\r\n\r\n", 1)[-1][:160].decode(errors="replace").replace("\n", " ")
print(f"{first} | {msg}")
