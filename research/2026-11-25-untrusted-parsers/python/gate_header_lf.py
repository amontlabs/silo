"""#384 local-models/prototype/gate.py: a bare LF inside a header value. The gate keeps the
value whole and writes it on; Go's reader (Ollama's) splits it into a second header."""
import json, os, socket, subprocess, sys, time
W = "/tmp/claude-0/gate"; os.makedirs(W, exist_ok=True)
json.dump({"18080": {"computer": "c1", "server": "ollama", "to": 18081, "models": None}}, open(W + "/grants.json", "w"))
up = subprocess.Popen([os.environ.get("ECHO", "go/echo"), "18081"])  # go build -o go/echo go/echo.go
g = subprocess.Popen([sys.executable, sys.argv[1]], env=dict(os.environ, GRANTS=W + "/grants.json", LOG=W + "/log.jsonl"), stdout=subprocess.PIPE)
g.stdout.readline(); time.sleep(0.3)
s = socket.create_connection(("127.0.0.1", 18080))
s.sendall(b"GET /api/tags HTTP/1.1\r\nhost: models.silo.internal\r\nx-pad: a\norigin: https://evil.example\r\n\r\n")
out = b""
while (d := s.recv(65536)): out += d
print(out.split(b"\r\n\r\n", 1)[1].decode())
g.kill(); up.kill()
