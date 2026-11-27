"""The Mac's side of a closed network: an HTTP proxy that listens on a Unix socket, not on a network.

It stands in for a proxy on the Mac that a computer reaches over vsock only. It takes CONNECT
(HTTPS and any other tunneled TCP) and plain http:// requests, checks the host and port against an
allowlist, and writes one JSON line per connection to LOG. A host on the allowlist is connected to
the address MAP gives it (the experiment's stand-in providers); an allowed host with no address in
MAP is answered 502, since this test reaches nothing on the internet.

Copied from #378 (research/2026-11-25-techniques-from-two-tools/closed-network/proxy.py); the only
change is that ALLOW may be "*", so that every host a program tries is logged.

Environment: SOCK (path of the Unix socket), LOG (log file), ALLOW (comma-separated host or
host:port; a host alone means port 443 and 80; "*" allows every host), MAP (JSON: {"host:port": "ip:port"}).
"""
import json, os, socket, threading, time

SOCK = os.environ["SOCK"]; LOG = open(os.environ["LOG"], "a")
ALLOW = [a.strip() for a in os.environ.get("ALLOW", "").split(",") if a.strip()]
MAP = json.loads(os.environ.get("MAP", "{}"))
LOCK = threading.Lock()


def log(**o):
    o["time"] = round(time.time(), 3)
    with LOCK:
        LOG.write(json.dumps(o) + "\n"); LOG.flush()


def allowed(host, port):
    for a in ALLOW:
        if a == "*":
            return True
        if ":" in a:
            h, p = a.rsplit(":", 1)
            if h == host and int(p) == port:
                return True
        elif a == host and port in (443, 80):
            return True
    return False


def pump(a, b, counter, key):
    try:
        while True:
            d = a.recv(65536)
            if not d:
                break
            counter[key] += len(d)
            b.sendall(d)
    except OSError:
        pass
    finally:
        try:
            b.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle(c):
    head = b""
    while b"\r\n\r\n" not in head and len(head) < 16384:
        d = c.recv(4096)
        if not d:
            c.close(); return
        head += d
    line, _, rest = head.partition(b"\r\n")
    try:
        method, target, _ = line.decode("latin-1").split(" ", 2)
    except ValueError:
        c.close(); return
    if method == "CONNECT":
        host, _, port = target.rpartition(":"); port = int(port)
        extra = head.split(b"\r\n\r\n", 1)[1]
    else:
        # plain http://host[:port]/path
        if not target.startswith("http://"):
            c.sendall(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n"); c.close(); return
        hp = target[7:].split("/", 1)[0]
        host, port = (hp.rsplit(":", 1)[0], int(hp.rsplit(":", 1)[1])) if ":" in hp else (hp, 80)
        extra = None
    if not allowed(host, port):
        log(method=method, host=host, port=port, decision="refused")
        c.sendall(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"); c.close(); return
    dest = MAP.get(f"{host}:{port}")
    if not dest:
        log(method=method, host=host, port=port, decision="allowed, no address in this test")
        c.sendall(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"); c.close(); return
    ip, p = dest.rsplit(":", 1)
    u = socket.create_connection((ip, int(p)))
    counter = {"up": 0, "down": 0}
    if method == "CONNECT":
        c.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        if extra:
            u.sendall(extra); counter["up"] += len(extra)
    else:
        u.sendall(head); counter["up"] += len(head)
    t = threading.Thread(target=pump, args=(c, u, counter, "up")); t.start()
    pump(u, c, counter, "down"); t.join()
    log(method=method, host=host, port=port, decision="allowed", up=counter["up"], down=counter["down"])
    c.close(); u.close()


if os.path.exists(SOCK):
    os.unlink(SOCK)
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.bind(SOCK); s.listen(64)
while True:
    c, _ = s.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
