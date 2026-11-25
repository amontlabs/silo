"""A stand-in for a computer's network card, from #390's linux/fake-guest-net.py, changed so that
the card is handed its socket, as Apple's VZFileHandleNetworkDeviceAttachment is handed a file
handle by the process that creates the virtual machine (Silo's helper).

    unshare -n python3 -I guest-nic.py --fd N --mac MAC --ready FILE --pump NICPUMP

Run inside a new network namespace (unshare -n, as root). It does what the framework and the
guest's kernel do together:
  - takes the datagram socket N it inherited (already connected by the helper) and only reads and
    writes it: like the framework, it never connects it again;
  - asks gvproxy's DHCP server for an address with raw DHCP frames (DISCOVER, OFFER, REQUEST,
    ACK) and keeps what it answered: address, mask, router, DNS, MTU (option 26);
  - creates tap0 with that MAC, address, MTU and a default route, brings lo and tap0 up;
  - writes the DHCP answer as JSON to FILE, then becomes nicpump, which moves frames between
    tap0 and the socket until it is killed.
Commands then run "inside the computer" with nsenter -t PID -n. Linux only; needs /dev/net/tun.
"""
import argparse
import ctypes
import fcntl
import json
import os
import random
import socket
import struct
import sys
import threading
import time

TUNSETIFF = 0x400454CA
IFF_TAP, IFF_NO_PI = 0x0002, 0x1000
SIOCSIFHWADDR, SIOCSIFMTU, SIOCSIFADDR, SIOCSIFNETMASK = 0x8924, 0x8922, 0x8916, 0x891C
SIOCGIFFLAGS, SIOCSIFFLAGS, SIOCADDRT = 0x8913, 0x8914, 0x890B
IFF_UP, IFF_RUNNING = 0x1, 0x40


def csum(b):
    if len(b) % 2:
        b += b"\0"
    s = sum(struct.unpack("!%dH" % (len(b) // 2), b))
    while s >> 16:
        s = (s & 0xFFFF) + (s >> 16)
    return (~s) & 0xFFFF


def dhcp_frame(mac, xid, msgtype, req_ip=None, server=None):
    opts = bytes([53, 1, msgtype])
    if req_ip:
        opts += bytes([50, 4]) + socket.inet_aton(req_ip)
    if server:
        opts += bytes([54, 4]) + socket.inet_aton(server)
    opts += bytes([55, 6, 1, 3, 6, 15, 26, 119]) + bytes([255])
    bootp = struct.pack("!BBBBIHH4s4s4s4s16s64s128s", 1, 1, 6, 0, xid, 0, 0x8000,
                        b"\0" * 4, b"\0" * 4, b"\0" * 4, b"\0" * 4, mac + b"\0" * 10, b"", b"")
    bootp += b"\x63\x82\x53\x63" + opts
    udp = struct.pack("!HHHH", 68, 67, 8 + len(bootp), 0) + bootp
    ip = struct.pack("!BBHHHBBH4s4s", 0x45, 0, 20 + len(udp), 0, 0, 64, 17, 0,
                     b"\0" * 4, b"\xff" * 4)
    ip = ip[:10] + struct.pack("!H", csum(ip)) + ip[12:]
    return b"\xff" * 6 + mac + b"\x08\x00" + ip + udp


def parse_dhcp(frame, xid):
    if len(frame) < 14 + 20 + 8 + 240 or frame[12:14] != b"\x08\x00":
        return None
    ihl = (frame[14] & 0xF) * 4
    if frame[14 + 9] != 17:
        return None
    u = 14 + ihl
    if struct.unpack("!H", frame[u + 2:u + 4])[0] != 68:
        return None
    b = frame[u + 8:]
    op, _, _, _, rxid = struct.unpack("!BBBBI", b[:8])
    if op != 2 or rxid != xid:
        return None
    yiaddr = socket.inet_ntoa(b[16:20])
    opts, i, o = {}, 240, b
    while i < len(o) and o[i] != 255:
        if o[i] == 0:
            i += 1
            continue
        code, ln = o[i], o[i + 1]
        opts[code] = o[i + 2:i + 2 + ln]
        i += 2 + ln
    r = {"address": yiaddr, "type": opts.get(53, b"\0")[0]}
    if 1 in opts:
        r["mask"] = socket.inet_ntoa(opts[1])
    if 3 in opts:
        r["router"] = socket.inet_ntoa(opts[3][:4])
    if 6 in opts:
        r["dns"] = [socket.inet_ntoa(opts[6][j:j + 4]) for j in range(0, len(opts[6]), 4)]
    if 26 in opts:
        r["mtu"] = struct.unpack("!H", opts[26])[0]
    if 54 in opts:
        r["server"] = socket.inet_ntoa(opts[54])
    if 51 in opts:
        r["lease_s"] = struct.unpack("!I", opts[51])[0]
    return r


def dhcp(conn, mac, timeout=20):
    xid = random.getrandbits(32)
    t0 = time.time()
    conn.settimeout(1.0)
    offer = ack = None
    while time.time() - t0 < timeout and not ack:
        conn.send(dhcp_frame(mac, xid, 1 if not offer else 3,
                             offer and offer["address"], offer and offer.get("server")))
        end = time.time() + 2
        while time.time() < end:
            try:
                f = conn.recv(65600)
            except socket.timeout:
                break
            r = parse_dhcp(f, xid)
            if not r:
                continue
            if r["type"] == 2 and not offer:
                offer = r
                break
            if r["type"] == 5 and offer:
                ack = r
                break
    if not ack:
        raise SystemExit("no DHCP answer from gvproxy in %ds" % timeout)
    ack["seconds"] = round(time.time() - t0, 3)
    return ack


def ifreq(name, payload):
    return struct.pack("16s", name.encode()) + payload + b"\0" * (24 - len(payload))


def sin(ip):
    return struct.pack("HH4s8s", socket.AF_INET, 0, socket.inet_aton(ip), b"\0" * 8)


class Rtentry(ctypes.Structure):
    _fields_ = [("rt_pad1", ctypes.c_ulong), ("rt_dst", ctypes.c_char * 16),
                ("rt_gateway", ctypes.c_char * 16), ("rt_genmask", ctypes.c_char * 16),
                ("rt_flags", ctypes.c_ushort), ("rt_pad2", ctypes.c_short),
                ("rt_pad3", ctypes.c_ulong), ("rt_pad4", ctypes.c_void_p),
                ("rt_metric", ctypes.c_short), ("rt_dev", ctypes.c_char_p),
                ("rt_mtu", ctypes.c_ulong), ("rt_window", ctypes.c_ulong),
                ("rt_irtt", ctypes.c_ushort)]


def up(c, name):
    flags = struct.unpack("16sH", fcntl.ioctl(c, SIOCGIFFLAGS, ifreq(name, b""))[:18])[1]
    fcntl.ioctl(c, SIOCSIFFLAGS, ifreq(name, struct.pack("H", flags | IFF_UP | IFF_RUNNING)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fd", type=int, required=True)
    ap.add_argument("--mac", required=True)
    ap.add_argument("--ready", required=True)
    ap.add_argument("--pump", required=True, help="the nicpump binary")
    a = ap.parse_args()
    mac = bytes(int(x, 16) for x in a.mac.split(":"))
    conn = socket.socket(fileno=a.fd)
    lease = dhcp(conn, mac)

    tap = os.open("/dev/net/tun", os.O_RDWR)
    fcntl.ioctl(tap, TUNSETIFF, ifreq("tap0", struct.pack("H", IFF_TAP | IFF_NO_PI)))
    c = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    fcntl.ioctl(c, SIOCSIFHWADDR, ifreq("tap0", struct.pack("H", 1) + mac))
    fcntl.ioctl(c, SIOCSIFMTU, ifreq("tap0", struct.pack("i", lease.get("mtu", 1500))))
    fcntl.ioctl(c, SIOCSIFADDR, ifreq("tap0", sin(lease["address"])))
    fcntl.ioctl(c, SIOCSIFNETMASK, ifreq("tap0", sin(lease.get("mask", "255.255.255.0"))))
    up(c, "tap0")
    up(c, "lo")
    if lease.get("router"):
        rt = Rtentry()
        for field, ip in (("rt_dst", "0.0.0.0"), ("rt_gateway", lease["router"]),
                          ("rt_genmask", "0.0.0.0")):
            ctypes.memmove(ctypes.addressof(rt) + getattr(Rtentry, field).offset, sin(ip), 16)
        rt.rt_flags = 0x1 | 0x2  # RTF_UP | RTF_GATEWAY
        rt.rt_dev = b"tap0"
        fcntl.ioctl(c, SIOCADDRT, bytes(rt))
    with open(a.ready + ".tmp", "w") as f:
        json.dump(lease, f)
    os.rename(a.ready + ".tmp", a.ready)

    # The data path: nicpump (nicpump.c) moves frames in C, so this stand-in does not set the
    # throughput measured.
    conn.settimeout(None)
    os.set_inheritable(tap, True)
    os.set_inheritable(conn.fileno(), True)
    os.execv(a.pump, [a.pump, str(tap), str(conn.fileno())])


if __name__ == "__main__":
    main()
