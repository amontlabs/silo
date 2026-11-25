"""Asks the agent to sign a userauth request for a made-up session, with no session-bind first."""
import os, socket, struct, sys
s = lambda b: struct.pack(">I", len(b)) + b
c = socket.socket(socket.AF_UNIX); c.connect(sys.argv[1])
def call(m):
    c.sendall(s(m)); n = struct.unpack(">I", c.recv(4))[0]; d = b""
    while len(d) < n: d += c.recv(n - len(d))
    return d
ids = call(bytes([11])); key = ids[9:9 + struct.unpack(">I", ids[5:9])[0]]
data = s(os.urandom(32)) + bytes([50]) + s(b"deploy") + s(b"ssh-connection") + s(b"publickey") + b"\x01" + s(b"ssh-ed25519") + s(key)
r = call(bytes([13]) + s(key) + s(data) + struct.pack(">I", 0))
print("signed" if r[0] == 14 else "refused")
