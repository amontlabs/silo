"""A stand-in for Silo's engine on the Mac, to see what a computer can reach of it.

    07-standin.py --dir D [--loopback-port P1] [--any-port P2]

Listens three ways, as an engine might:
  loopback  TCP on 127.0.0.1 (what a careless engine, or any local tool of yours, does)
  any       TCP on 0.0.0.0 (the control: a service open to every network of the Mac)
  unix      a Unix socket D/engine.sock, mode 0600 (what Silo's engine does)
Ports 0 pick free ports. Writes D/standin.json with the ports once it listens. Every connection
gets "HTTP/1.0 200" and the body "silo-kit-standin NAME", so a curl from inside the computer
shows which listener it reached. Each connection is logged to D/standin.log as one JSON line
with the listener and the peer's address: through gvproxy a computer's connection arrives from
the Mac's own address, like a local program's. Runs until killed.
"""
import argparse
import json
import os
import socket
import threading
import time


def serve(sock, name, log, lock):
    while True:
        try:
            c, peer = sock.accept()
        except OSError:
            return
        threading.Thread(target=answer, args=(c, peer, name, log, lock), daemon=True).start()


def answer(c, peer, name, log, lock):
    try:
        c.settimeout(3)
        try:
            c.recv(2048)
        except OSError:
            pass
        body = ("silo-kit-standin %s\n" % name).encode()
        c.sendall(b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: %d\r\n"
                  b"Connection: close\r\n\r\n" % len(body) + body)
    except OSError:
        pass
    finally:
        c.close()
    with lock:
        log.write(json.dumps({"t": round(time.time(), 3), "listener": name,
                              "peer": peer if isinstance(peer, str) else list(peer)}) + "\n")
        log.flush()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--loopback-port", type=int, default=0)
    ap.add_argument("--any-port", type=int, default=0)
    a = ap.parse_args()
    lock = threading.Lock()
    log = open(os.path.join(a.dir, "standin.log"), "a")

    socks = []
    for name, addr, port in (("loopback", "127.0.0.1", a.loopback_port), ("any", "0.0.0.0", a.any_port)):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((addr, port))
        s.listen(128)
        socks.append((name, s))
    path = os.path.join(a.dir, "engine.sock")
    try:
        os.unlink(path)
    except OSError:
        pass
    u = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    old = os.umask(0o177)
    u.bind(path)
    os.umask(old)
    u.listen(16)
    socks.append(("unix", u))

    info = {"loopback_port": socks[0][1].getsockname()[1], "any_port": socks[1][1].getsockname()[1],
            "unix": path, "pid": os.getpid()}
    with open(os.path.join(a.dir, "standin.json.tmp"), "w") as f:
        json.dump(info, f)
    os.rename(os.path.join(a.dir, "standin.json.tmp"), os.path.join(a.dir, "standin.json"))
    threads = [threading.Thread(target=serve, args=(s, n, log, lock), daemon=True) for n, s in socks]
    for t in threads:
        t.start()
    while True:
        time.sleep(3600)


if __name__ == "__main__":
    main()
