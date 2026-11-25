"""A stand-in for a helper that owns its computer's network link: it sits between the computer's
network device (silovm's --net vfkit:DOWN) and gvproxy (UP), passes every frame on, and when
gvproxy restarts, connects again and says "VFKT" again, so gvproxy learns its peer anew.

    36-relay.py --down DOWN_SOCKET --up GVPROXY_SOCKET --dir D

DOWN is a unixgram socket this relay binds (silovm connects to it); its own socket towards
gvproxy is D/relay-up.sock. Writes D/relay.json once it listens, and one JSON line per reconnect
to D/relay.log. Runs until killed.
"""
import argparse
import errno
import json
import os
import select
import socket
import time


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--down", required=True)
    ap.add_argument("--up", required=True)
    ap.add_argument("--dir", required=True)
    a = ap.parse_args()
    log = open(os.path.join(a.dir, "relay.log"), "a")
    for p in (a.down,):
        try:
            os.unlink(p)
        except OSError:
            pass
    down = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    down.bind(a.down)
    for s in (down,):
        s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 4 << 20)
    up_path = os.path.join(a.dir, "relay-up.sock")
    state = {"up": None, "peer": None, "reconnects": 0, "dropped": 0}

    def connect_up():
        if state["up"] is not None:
            state["up"].close()
            state["up"] = None
        try:
            os.unlink(up_path)
        except OSError:
            pass
        u = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        u.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
        u.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 4 << 20)
        u.bind(up_path)
        try:
            u.connect(a.up)
            u.send(b"VFKT")
        except OSError:
            u.close()
            return False
        state["up"] = u
        state["reconnects"] += 1
        log.write(json.dumps({"t": round(time.time(), 3), "event": "connected", "n": state["reconnects"]}) + "\n")
        log.flush()
        return True

    while not connect_up():
        time.sleep(0.1)
    with open(os.path.join(a.dir, "relay.json"), "w") as f:
        json.dump({"down": a.down, "up": a.up, "pid": os.getpid()}, f)
    last_try = 0.0
    while True:
        socks = [down] + ([state["up"]] if state["up"] is not None else [])
        r, _, _ = select.select(socks, [], [], 0.1)
        if state["up"] is None and time.time() - last_try > 0.1:
            last_try = time.time()
            connect_up()
        for s in r:
            if s is down:
                try:
                    frame, peer = down.recvfrom(65536 + 64)
                except OSError:
                    continue
                state["peer"] = peer or state["peer"]
                if frame == b"VFKT":
                    continue
                if state["up"] is None:
                    state["dropped"] += 1
                    continue
                try:
                    state["up"].send(frame)
                except OSError as e:
                    state["dropped"] += 1
                    if e.errno in (errno.ECONNREFUSED, errno.ENOENT, errno.ENOTCONN, errno.EPIPE):
                        log.write(json.dumps({"t": round(time.time(), 3), "event": "up-lost", "errno": e.errno}) + "\n")
                        log.flush()
                        state["up"].close()
                        state["up"] = None
            else:
                try:
                    frame = s.recv(65536 + 64)
                except OSError as e:
                    log.write(json.dumps({"t": round(time.time(), 3), "event": "up-lost", "errno": e.errno}) + "\n")
                    log.flush()
                    s.close()
                    state["up"] = None
                    continue
                if state["peer"]:
                    try:
                        down.sendto(frame, state["peer"])
                    except OSError:
                        state["dropped"] += 1


if __name__ == "__main__":
    main()
