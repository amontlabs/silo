"""A small HTTP proxy that stands for Silo's proxy on the Mac, for tests 40, 43 and 44.

    python3 -I py/kitproxy.py (--unix PATH | --tcp HOST:PORT) --log FILE
                              [--allow GLOB]... [--allow-public] [--map HOST=ADDR]... [--ready FILE]

It takes CONNECT host:port (HTTPS, git, WebSockets) and plain requests with a full URL, and
writes one JSON line per request to FILE: the time, the method, the host and port asked for, and
what it did (dialled, denied, or failed). It is the only thing that looks a name up or dials out:
- a host matching an --allow GLOB is dialled;
- with --allow-public, any other host is looked up here and dialled only when every address it
  has is public (never the Mac's network, its loopback, link-local or cloud metadata);
- every other host is denied without being looked up, so a denied name never reaches a resolver.
--map HOST=ADDR answers HOST with ADDR without a lookup (the Linux analogues' stand-in origin).
Nothing is read past the request line and headers: TLS stays end to end.
"""
import argparse
import fnmatch
import ipaddress
import json
import os
import socket
import sys
import threading
import time

LOCK = threading.Lock()


def log(path, **kw):
    kw["t"] = round(time.time(), 3)
    with LOCK, open(path, "a") as f:
        f.write(json.dumps(kw, sort_keys=True) + "\n")


def public(addr):
    ip = ipaddress.ip_address(addr)
    return ip.is_global and not ip.is_multicast


def decide(host, port, a):
    """(addresses to dial, reason); no addresses means denied."""
    h = host.strip("[]").lower()
    mapped = dict(m.lower().split("=", 1) for m in a.map)
    for g in a.allow:
        if h in mapped and fnmatch.fnmatch(h, g.lower()):
            return [mapped[h]], "allowed, mapped"
        if fnmatch.fnmatch(h, g.lower()):
            try:
                return [i[4][0] for i in socket.getaddrinfo(h, port, type=socket.SOCK_STREAM)], "allowed"
            except OSError as e:
                return [], "lookup failed: %s" % e
    if not a.allow_public:
        return [], "denied, not looked up"
    try:
        addrs = [i[4][0] for i in socket.getaddrinfo(h, port, type=socket.SOCK_STREAM)]
    except OSError as e:
        return [], "lookup failed: %s" % e
    if not addrs or not all(public(x) for x in addrs):
        return [], "denied: %s is not public" % ",".join(sorted(set(addrs)))
    return addrs, "public"


def read_head(c):
    data = b""
    while b"\r\n\r\n" not in data:
        b = c.recv(65536)
        if not b:
            return None, b""
        data += b
        if len(data) > 1 << 20:
            return None, b""
    head, rest = data.split(b"\r\n\r\n", 1)
    return head.decode("latin-1"), rest


def pump(a, b):
    try:
        while True:
            d = a.recv(65536)
            if not d:
                break
            b.sendall(d)
    except OSError:
        pass
    try:
        b.shutdown(socket.SHUT_WR)
    except OSError:
        pass


def serve(c, a):
    try:
        head, rest = read_head(c)
        if head is None:
            return
        lines = head.split("\r\n")
        method, target, _ver = (lines[0].split(" ") + ["", ""])[:3]
        if method == "CONNECT":
            host, _, port = target.rpartition(":")
            port = int(port or 443)
        else:
            if not target.startswith("http://"):
                c.sendall(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                log(a.log, method=method, host="", port=0, result="not a proxy request", target=target[:200])
                return
            hostport = target[7:].split("/", 1)[0]
            host, _, port = hostport.partition(":")
            port = int(port or 80)
        addrs, why = decide(host, port, a)
        if not addrs:
            c.sendall(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            log(a.log, method=method, host=host, port=port, result="denied", why=why)
            return
        u = None
        for addr in addrs:
            try:
                u = socket.create_connection((addr, port), timeout=15)
                break
            except OSError:
                u = None
        if u is None:
            c.sendall(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            log(a.log, method=method, host=host, port=port, result="failed", why="no address answered")
            return
        u.settimeout(None)
        log(a.log, method=method, host=host, port=port, result="dialled", why=why, addr=addrs[0])
        if method == "CONNECT":
            c.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            if rest:
                u.sendall(rest)
        else:
            path = "/" + target[7:].split("/", 1)[1] if "/" in target[7:] else "/"
            kept = [ln for ln in lines[1:] if not ln.lower().startswith(("proxy-", "connection:"))]
            u.sendall(("%s %s HTTP/1.1\r\n" % (method, path) + "\r\n".join(kept + ["Connection: close"]) + "\r\n\r\n").encode("latin-1") + rest)
        t = threading.Thread(target=pump, args=(u, c), daemon=True)
        t.start()
        pump(c, u)
        t.join(600)
        u.close()
    except OSError as e:
        log(a.log, method="?", host="", port=0, result="error", why=str(e))
    finally:
        c.close()


def main():
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--unix")
    g.add_argument("--tcp")
    ap.add_argument("--log", required=True)
    ap.add_argument("--allow", action="append", default=[])
    ap.add_argument("--allow-public", action="store_true")
    ap.add_argument("--map", action="append", default=[])
    ap.add_argument("--ready")
    a = ap.parse_args()
    if a.unix:
        try:
            os.unlink(a.unix)
        except FileNotFoundError:
            pass
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.bind(a.unix)
        where = a.unix
    else:
        h, _, p = a.tcp.rpartition(":")
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((h, int(p)))
        where = "%s:%d" % s.getsockname()
    s.listen(128)
    if a.ready:
        with open(a.ready + ".tmp", "w") as f:
            f.write(where + "\n")
        os.replace(a.ready + ".tmp", a.ready)
    while True:
        c, _ = s.accept()
        threading.Thread(target=serve, args=(c, a), daemon=True).start()


if __name__ == "__main__":
    sys.exit(main())
