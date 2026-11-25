"""The Mac's side of test 36: what a computer's connections through its network stack meet.

    36-server.py --dir D [--port P] [--duplex-port Q]

Listens on 0.0.0.0 (so a computer reaches it at the Mac's own address) and writes D/server.json
with its ports once it listens. #347's checks 6 to 9, as a stack sees them:

  POST /echo        sends the body back as it came, with X-Sha256 of what it received (6)
  GET /stream       a streamed reply: one event at once, the next 2 s later, then the end (7)
  GET /slow/N       a reply that starts, stays silent N seconds, then ends (8)
  GET /ok           "ok", for the probe that watches a restart of the stack (10)
  duplex port       a line-by-line echo over one long TCP connection, both ways at once (9)

Each request is logged to D/server.log as one JSON line: what it was, when it ended, and how
(complete, or the error the connection ended on). Runs until killed.
"""
import argparse
import hashlib
import json
import os
import socket
import threading
import time

LOCK = threading.Lock()
LOG = None


def log(**kw):
    kw["t"] = round(time.time(), 3)
    with LOCK:
        LOG.write(json.dumps(kw) + "\n")
        LOG.flush()


def read_request(c):
    buf = b""
    while b"\r\n\r\n" not in buf:
        d = c.recv(65536)
        if not d:
            return None, None, b""
        buf += d
        if len(buf) > 65536 * 4:
            return None, None, b""
    head, rest = buf.split(b"\r\n\r\n", 1)
    lines = head.decode("latin-1").split("\r\n")
    method, path = lines[0].split(" ")[:2]
    hdrs = {}
    for ln in lines[1:]:
        k, _, v = ln.partition(":")
        hdrs[k.strip().lower()] = v.strip()
    n = int(hdrs.get("content-length", "0") or 0)
    while len(rest) < n:
        d = c.recv(min(1 << 20, n - len(rest)))
        if not d:
            break
        rest += d
    return method, path, rest


def chunk(b):
    return b"%x\r\n" % len(b) + b + b"\r\n"


def handle(c, peer):
    path = "?"
    try:
        c.settimeout(900)
        method, path, body = read_request(c)
        if method is None:
            return
        if method == "POST" and path == "/echo":
            sha = hashlib.sha256(body).hexdigest()
            c.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nX-Sha256: %s\r\n"
                      b"Content-Length: %d\r\nConnection: close\r\n\r\n" % (sha.encode(), len(body)) + body)
            log(req="echo", bytes=len(body), sha=sha[:16], how="complete")
        elif path == "/stream":
            c.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n"
                      b"Connection: close\r\n\r\n" + chunk(b"data: first\n\n"))
            time.sleep(2)
            c.sendall(chunk(b"data: second\n\n") + b"0\r\n\r\n")
            log(req="stream", how="complete")
        elif path.startswith("/slow/"):
            n = float(path.split("/")[2])
            c.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n"
                      b"Connection: close\r\n\r\n" + chunk(b"start\n"))
            log(req="slow", idle=n, how="started")
            time.sleep(n)
            c.sendall(chunk(b"end\n") + b"0\r\n\r\n")
            log(req="slow", idle=n, how="complete")
        else:
            c.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n")
    except OSError as e:
        log(req=path, how="error: %s" % e.__class__.__name__ + (" %s" % e.errno if e.errno else ""))
    finally:
        c.close()


def duplex(c, peer):
    n = 0
    try:
        c.settimeout(120)
        f = c.makefile("rb")
        for line in f:
            c.sendall(line)
            n += 1
        log(req="duplex", lines=n, how="complete")
    except OSError as e:
        log(req="duplex", lines=n, how="error: %s" % e.__class__.__name__)
    finally:
        c.close()


def serve(s, fn):
    while True:
        try:
            c, peer = s.accept()
        except OSError:
            return
        threading.Thread(target=fn, args=(c, peer), daemon=True).start()


def main():
    global LOG
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--duplex-port", type=int, default=0)
    a = ap.parse_args()
    LOG = open(os.path.join(a.dir, "server.log"), "a")
    socks = []
    for port in (a.port, a.duplex_port):
        s = socket.socket()
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind(("0.0.0.0", port))
        s.listen(256)
        socks.append(s)
    threading.Thread(target=serve, args=(socks[1], duplex), daemon=True).start()
    with open(os.path.join(a.dir, "server.json.tmp"), "w") as f:
        json.dump({"port": socks[0].getsockname()[1], "duplex_port": socks[1].getsockname()[1]}, f)
    os.rename(os.path.join(a.dir, "server.json.tmp"), os.path.join(a.dir, "server.json"))
    serve(socks[0], handle)


if __name__ == "__main__":
    main()
