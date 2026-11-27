#!/usr/bin/env python3
"""A stand-in for the boundary's connection log: an HTTP proxy that records, for each connection,
the host, the port, when it opened and closed, and the bytes each way. It forwards CONNECT and
plain HTTP requests to the proxy named by UPSTREAM (host:port), or straight out without one.

  hostlog.py PORT LOG.jsonl            the phase written on each line is read from LOG.phase

The phase file lets a run label its steps (setup, start, night) without restarting the proxy.
"""
import json, os, socket, sys, threading, time

PORT, LOG = int(sys.argv[1]), sys.argv[2]
UP = os.environ.get("UPSTREAM", "")
lock = threading.Lock()


def phase():
    try:
        return open(LOG + ".phase").read().strip()
    except OSError:
        return "?"


def write(rec):
    with lock, open(LOG, "a") as f:
        f.write(json.dumps(rec) + "\n")


def pump(a, b, count, key):
    try:
        while True:
            d = a.recv(65536)
            if not d:
                break
            count[key] += len(d)
            b.sendall(d)
    except OSError:
        pass
    finally:
        for s in (a, b):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def read_head(s):
    buf = b""
    while b"\r\n\r\n" not in buf:
        d = s.recv(4096)
        if not d:
            return None, b""
        buf += d
        if len(buf) > 65536:
            return None, b""
    head, rest = buf.split(b"\r\n\r\n", 1)
    return head.decode("latin-1"), rest


def dial(host, port):
    if UP:
        uh, up = UP.rsplit(":", 1)
        s = socket.create_connection((uh, int(up)), timeout=30)
        s.sendall(("CONNECT %s:%d HTTP/1.1\r\nHost: %s:%d\r\n\r\n" % (host, port, host, port)).encode())
        head, rest = read_head(s)
        if head is None or " 200" not in head.split("\r\n")[0]:
            s.close()
            raise OSError("upstream refused: %r" % (head or "")[:80])
        s.settimeout(None)
        return s, rest
    s = socket.create_connection((host, port), timeout=30)
    s.settimeout(None)
    return s, b""


def handle(c):
    t0 = time.time()
    rec = {"phase": phase(), "t0": round(t0, 3)}
    count = {"up": 0, "down": 0}
    try:
        head, rest = read_head(c)
        if head is None:
            return
        line = head.split("\r\n")[0]
        method, target, _ = line.split(" ", 2)
        if method == "CONNECT":
            host, port = target.rsplit(":", 1)
            port = int(port)
            first = b""
            reply = b"HTTP/1.1 200 Connection established\r\n\r\n"
        else:  # plain HTTP: absolute URI
            u = target.split("://", 1)[1]
            hp = u.split("/", 1)[0]
            host, port = (hp.rsplit(":", 1)[0], int(hp.rsplit(":", 1)[1])) if ":" in hp else (hp, 80)
            first = (head + "\r\n\r\n").encode("latin-1") + rest
            rest = b""
            reply = b""
        rec.update({"host": host.lower().strip("[]"), "port": port, "method": method})
        try:
            s, early = dial(host, port)
        except OSError as e:
            rec["error"] = str(e)[:120]
            c.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            return
        if reply:
            c.sendall(reply)
        if early:
            c.sendall(early)
            count["down"] += len(early)
        if rest:
            s.sendall(rest)
            count["up"] += len(rest)
        if first:
            if UP:  # an upstream proxy takes the absolute URI as it is, over the tunnel to itself
                pass
            s.sendall(first)
            count["up"] += len(first)
        t = threading.Thread(target=pump, args=(s, c, count, "down"), daemon=True)
        t.start()
        pump(c, s, count, "up")
        t.join()
    except Exception as e:  # noqa: BLE001
        rec["error"] = str(e)[:120]
    finally:
        rec.update({"t1": round(time.time(), 3), "up": count["up"], "down": count["down"]})
        if "host" in rec:
            write(rec)
        c.close()


srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", PORT))
srv.listen(256)
while True:
    conn, _ = srv.accept()
    threading.Thread(target=handle, args=(conn,), daemon=True).start()
