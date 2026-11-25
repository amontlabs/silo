"""Sends exact bytes to the boundary as a program inside the computer would, and prints what comes back."""
import socket, ssl, sys, time
port, ca, sni, path = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
data = open(path, "rb").read()
cx = ssl.create_default_context(cafile=ca); cx.set_alpn_protocols(["http/1.1"]); cx.verify_flags &= ~ssl.VERIFY_X509_STRICT
s = cx.wrap_socket(socket.create_connection(("127.0.0.1", port)), server_hostname=sni)
s.sendall(data); s.settimeout(1.5); got = b""
try:
    while True:
        d = s.recv(65536)
        if not d: break
        got += d
except socket.timeout: pass
print(got.decode("latin1"))
