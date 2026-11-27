"""The Mac's end of a network card that leads nowhere (#378), for tests 40 and 43.

    python3 -I py/deadend.py --sock PATH --counts FILE [--router 10.254.0.1] [--no-arp]

Listens on a Unix datagram socket the way gvproxy's -listen-vfkit does, so silovm's
--net vfkit:PATH (and the Linux analogue's fake-guest-net.py) talk to it unchanged: the first
datagram from a peer is "VFKT", then each datagram is one Ethernet frame. It answers an ARP
request for the router's address with a made-up MAC (#378: a macOS 27 computer stays "offline"
without that one answer), and drops every other frame. With --no-arp it answers nothing.

It never sends a frame anywhere but back to the computer, and never opens another socket. Every
second, and when stopped, it writes FILE: a JSON object of the frames it saw, by kind, with the
destinations of the IP ones (what root in the computer tried to reach), and the ARP answers given.
"""
import argparse
import json
import os
import signal
import socket
import struct
import sys
import time

ROUTER_MAC = bytes.fromhex("5a94ef000001")


def ip4(b):
    return socket.inet_ntoa(b)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sock", required=True)
    ap.add_argument("--counts", required=True)
    ap.add_argument("--router", default="10.254.0.1")
    ap.add_argument("--no-arp", action="store_true")
    a = ap.parse_args()
    router = socket.inet_aton(a.router)

    try:
        os.unlink(a.sock)
    except FileNotFoundError:
        pass
    s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    s.bind(a.sock)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
    s.settimeout(1.0)
    counts = {"frames": 0, "arp_answered": 0, "arp_other": 0, "dropped": {}, "ip_destinations": {},
              "peers": 0, "no_arp": a.no_arp, "router": a.router, "started": time.time()}
    stop = []

    def write_counts():
        tmp = a.counts + ".tmp"
        with open(tmp, "w") as f:
            json.dump(counts, f, sort_keys=True)
        os.replace(tmp, a.counts)

    signal.signal(signal.SIGTERM, lambda *_: stop.append(1))
    signal.signal(signal.SIGINT, lambda *_: stop.append(1))
    last = 0.0
    while not stop:
        if time.time() - last > 1:
            write_counts()
            last = time.time()
        try:
            frame, peer = s.recvfrom(65536)
        except socket.timeout:
            continue
        except InterruptedError:
            continue
        if frame == b"VFKT":
            counts["peers"] += 1
            continue
        if len(frame) < 14:
            continue
        counts["frames"] += 1
        dst, src, etype = frame[0:6], frame[6:12], struct.unpack("!H", frame[12:14])[0]
        body = frame[14:]
        if etype == 0x0806 and len(body) >= 28:
            op = struct.unpack("!H", body[6:8])[0]
            sha, spa, tpa = body[8:14], body[14:18], body[24:28]
            if op == 1 and tpa == router and not a.no_arp:
                reply = struct.pack("!HHBBH", 1, 0x0800, 6, 4, 2) + ROUTER_MAC + router + sha + spa
                s.sendto(sha + ROUTER_MAC + struct.pack("!H", 0x0806) + reply, peer)
                counts["arp_answered"] += 1
            else:
                counts["arp_other"] += 1
            continue
        if etype == 0x0800 and len(body) >= 20:
            proto = body[9]
            name = {1: "icmp", 6: "tcp", 17: "udp"}.get(proto, "ip-proto-%d" % proto)
            d = ip4(body[16:20])
            port = ""
            ihl = (body[0] & 15) * 4
            if proto in (6, 17) and len(body) >= ihl + 4:
                port = ":%d" % struct.unpack("!H", body[ihl + 2:ihl + 4])[0]
            key = "%s %s%s" % (name, d, port)
            counts["ip_destinations"][key] = counts["ip_destinations"].get(key, 0) + 1
        else:
            name = {0x86DD: "ipv6"}.get(etype, "ethertype-%04x" % etype)
        counts["dropped"][name] = counts["dropped"].get(name, 0) + 1
    write_counts()
    s.close()
    try:
        os.unlink(a.sock)
    except OSError:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
