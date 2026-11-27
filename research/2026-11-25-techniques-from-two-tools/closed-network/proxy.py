"""The Mac's side of a closed network: an HTTP proxy that listens on a Unix socket, not on a network.

It stands in for a proxy on the Mac that a computer reaches over vsock only. It takes CONNECT
(HTTPS and any other tunneled TCP) and plain http:// requests, checks the host and port against an
allowlist, and writes one JSON line per connection to LOG. A host on the allowlist is connected to
the address MAP gives it (the experiment's stand-in providers); an allowed host with no address in
MAP is answered 502, since this test reaches nothing on the internet.

Environment: SOCK (path of the Unix socket), LOG (log file), ALLOW (comma-separated host or
host:port; a host alone means port 443 and 80), MAP (JSON: {"host:port": "ip:port"}).
"""
import json, os, re, socket, threading, time

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
        if ":" in a:
            h, p = a.rsplit(":", 1)
            if h == host and int(p) == port:
                return True
        elif a == host and port in (443, 80):
            return True
    return False


TOKEN = rb"[!#$%&'*+.^_`|~0-9A-Za-z-]+"
HOP = {b"connection", b"keep-alive", b"proxy-connection", b"proxy-authorization", b"te", b"upgrade"}


def parse_head(head):
    """(request line, [(name, value)]) of a head ending in CRLF CRLF, read strictly: CRLF line ends
    only, no folded line, a token and a colon on each header line, no NUL. None when it is not,
    since a host that reads it another way would see other requests or other headers."""
    text = head[:-4]
    bare = text.replace(b"\r\n", b"")
    if b"\n" in bare or b"\r" in bare or b"\0" in text:
        return None
    lines = text.split(b"\r\n")
    hs = []
    for line in lines[1:]:
        m = re.fullmatch(TOKEN + rb":[ \t]*(.*?)[ \t]*", line)
        if not m:
            return None
        hs.append((line.split(b":", 1)[0].lower(), m[1]))
    return lines[0].decode("latin-1"), hs


def refuse(c, code, why, **o):
    log(decision="refused", why=why, **o)
    c.sendall(b"HTTP/1.1 %d X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n" % code); c.close()


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
    if b"\r\n\r\n" not in head:
        c.close(); return
    head, extra = head.split(b"\r\n\r\n", 1)
    head += b"\r\n\r\n"
    parsed = parse_head(head)
    if not parsed:
        return refuse(c, 400, "head")
    line, hs = parsed
    try:
        method, target, _ = line.split(" ", 2)
    except ValueError:
        c.close(); return
    if method == "CONNECT":
        m = re.fullmatch(r"([a-z0-9.-]+):(\d{1,5})", target.lower())
        if not m:
            return refuse(c, 400, "target", method=method)
        host, port = m[1], int(m[2])
    else:
        # Plain http://host[:port]/path: one request per connection, checked whole. The Host
        # header must name the URL's host, the body is passed by its length and nothing after it,
        # and the host is asked to close, so no later request on the connection reaches it.
        m = re.fullmatch(r"http://([a-z0-9.-]+)(?::(\d{1,5}))?(/[^\s]*)?", target, re.I)
        if not m:
            return refuse(c, 400, "target", method=method)
        host, port = m[1].lower(), int(m[2] or 80)
        hosts = [v for k, v in hs if k == b"host"]
        named = lambda v: re.fullmatch(r"([a-z0-9.-]+)(?::(\d{1,5}))?", v.decode("latin-1").lower())
        if len(hosts) != 1 or not named(hosts[0]) or (named(hosts[0])[1], int(named(hosts[0])[2] or 80)) != (host, port):
            return refuse(c, 400, "host-header", method=method, host=host, port=port)
        if any(k == b"transfer-encoding" for k, _ in hs):
            return refuse(c, 501, "transfer-encoding", method=method, host=host, port=port)
        lengths = [v for k, v in hs if k == b"content-length"]
        if len(lengths) > 1 or (lengths and not re.fullmatch(rb"\d{1,12}", lengths[0])):
            return refuse(c, 400, "content-length", method=method, host=host, port=port)
        body_len = int(lengths[0]) if lengths else 0
    if not allowed(host, port):
        return refuse(c, 403, "not-allowed", method=method, host=host, port=port)
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
        t = threading.Thread(target=pump, args=(c, u, counter, "up")); t.start()
        pump(u, c, counter, "down"); t.join()
    else:
        out = (line + "\r\n").encode("latin-1") + b"".join(
            k + b": " + v + b"\r\n" for k, v in hs if k not in HOP) + b"Connection: close\r\n\r\n"
        body = extra[:body_len]
        while len(body) < body_len:
            d = c.recv(min(65536, body_len - len(body)))
            if not d:
                break
            body += d
        u.sendall(out + body); counter["up"] += len(out) + len(body)
        pump(u, c, counter, "down")           # bytes the program sends after the body go nowhere
    log(method=method, host=host, port=port, decision="allowed", up=counter["up"], down=counter["down"])
    c.close(); u.close()


if os.path.exists(SOCK):
    os.unlink(SOCK)
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.bind(SOCK); s.listen(64)
while True:
    c, _ = s.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
