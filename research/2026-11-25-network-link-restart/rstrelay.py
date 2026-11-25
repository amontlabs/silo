"""The relay of relay.c, plus one thing only a relay can do: it reads the TCP headers of the frames
it passes, keeps each open connection of the computer with the next sequence number the
computer expects, and when gvproxy is back after a restart, sends the computer a reset for each
connection that was open, as if from the far end. Programs inside then see "connection reset" at
once and connect again, instead of waiting on a connection whose other half died with gvproxy.

    rstrelay.py DOWNFD UPPATH BINDPATH [--no-reset]

Like relay.c: SIGUSR1 means "a new gvproxy is up, connect again". One JSON line per event on
stdout. Python, so it is the stand-in for the logic, not for the speed.
"""
import json
import os
import select
import signal
import socket
import struct
import sys
import time

M32 = 1 << 32


def csum(b):
    if len(b) % 2:
        b += b"\0"
    s = sum(struct.unpack("!%dH" % (len(b) // 2), b))
    while s >> 16:
        s = (s & 0xFFFF) + (s >> 16)
    return (~s) & 0xFFFF


def tcp_of(frame):
    """(src_mac, dst_mac, src_ip, dst_ip, sport, dport, seq, ack, flags, payload_len) or None."""
    if len(frame) < 54 or frame[12:14] != b"\x08\x00" or frame[23] != 6:
        return None
    ihl = (frame[14] & 0xF) * 4
    total = struct.unpack("!H", frame[16:18])[0]
    t = 14 + ihl
    sport, dport, seq, ack, off_flags = struct.unpack("!HHIIH", frame[t:t + 14])
    doff = (off_flags >> 12) * 4
    flags = off_flags & 0x3F
    plen = total - ihl - doff
    return frame[6:12], frame[0:6], frame[26:30], frame[30:34], sport, dport, seq, ack, flags, plen


def reset_frame(guest_mac, gw_mac, remote_ip, guest_ip, rport, gport, seq, ack):
    tcp = struct.pack("!HHIIHHHH", rport, gport, seq, ack, (5 << 12) | 0x14, 0, 0, 0)  # RST|ACK
    pseudo = remote_ip + guest_ip + struct.pack("!BBH", 0, 6, len(tcp))
    tcp = tcp[:16] + struct.pack("!H", csum(pseudo + tcp)) + tcp[18:]
    ip = struct.pack("!BBHHHBBH4s4s", 0x45, 0, 20 + len(tcp), 0, 0x4000, 64, 6, 0, remote_ip, guest_ip)
    ip = ip[:10] + struct.pack("!H", csum(ip)) + ip[12:]
    return guest_mac + gw_mac + b"\x08\x00" + ip + tcp


def main():
    down = socket.socket(fileno=int(sys.argv[1]))
    up_path, bind_path = sys.argv[2], sys.argv[3]
    send_resets = "--no-reset" not in sys.argv
    state = {"up": None, "want": False, "dropped": 0, "n": 0}
    flows = {}  # (guest_ip, gport, remote_ip, rport) -> dict(guest_mac, gw_mac, expect, guest_next)

    def emit(**kw):
        kw["t"] = round(time.time(), 3)
        print(json.dumps(kw), flush=True)

    def connect_up():
        if state["up"] is not None:
            state["up"].close()
            state["up"] = None
        try:
            os.unlink(bind_path)
        except OSError:
            pass
        u = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        u.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
        u.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1 << 20)
        u.bind(bind_path)
        try:
            u.connect(up_path)
            u.send(b"VFKT")
        except OSError:
            u.close()
            return False
        state["up"] = u
        state["n"] += 1
        resets = 0
        if state["n"] > 1 and send_resets:
            for (gip, gport, rip, rport), f in list(flows.items()):
                down.send(reset_frame(f["guest_mac"], f["gw_mac"], rip, gip, rport, gport, f["expect"], f["guest_next"]))
                resets += 1
            flows.clear()
        emit(event="connected", n=state["n"], dropped=state["dropped"], resets_sent=resets)
        return True

    signal.signal(signal.SIGUSR1, lambda *_: state.update(want=True))
    while not connect_up():
        time.sleep(0.05)
    while True:
        if state["want"] or state["up"] is None:
            state["want"] = False
            if not connect_up():
                time.sleep(0.05)
        socks = [down] + ([state["up"]] if state["up"] is not None else [])
        try:
            r, _, _ = select.select(socks, [], [], 0.1)
        except InterruptedError:
            continue
        for s in r:
            if s is down:
                frame = down.recv(65600)
                t = tcp_of(frame)
                if t:
                    smac, dmac, sip, dip, sp, dp, seq, ack, fl, plen = t
                    key = (sip, sp, dip, dp)
                    if fl & 0x04:
                        flows.pop(key, None)
                    elif key in flows or fl & 0x02:
                        f = flows.setdefault(key, {"guest_mac": smac, "gw_mac": dmac, "expect": ack, "guest_next": 0})
                        f["guest_next"] = (seq + plen + (1 if fl & 0x03 else 0)) % M32
                        if fl & 0x10:
                            f["expect"] = ack
                        if fl & 0x01:
                            f["guest_fin"] = True
                            if f.get("remote_fin"):
                                flows.pop(key, None)
                if state["up"] is None:
                    state["dropped"] += 1
                    continue
                try:
                    state["up"].send(frame)
                except OSError:
                    state["dropped"] += 1
                    state["up"].close()
                    state["up"] = None
            else:
                try:
                    frame = s.recv(65600)
                except OSError:
                    s.close()
                    state["up"] = None
                    continue
                t = tcp_of(frame)
                if t:
                    smac, dmac, sip, dip, sp, dp, seq, ack, fl, plen = t
                    key = (dip, dp, sip, sp)
                    if fl & 0x04:
                        flows.pop(key, None)
                    elif fl & 0x12 == 0x12 or key in flows:
                        f = flows.setdefault(key, {"guest_mac": dmac, "gw_mac": smac, "expect": 0, "guest_next": ack})
                        nxt = (seq + plen + (1 if fl & 0x03 else 0)) % M32
                        if (nxt - f["expect"]) % M32 < (1 << 31):
                            f["expect"] = nxt
                        if fl & 0x01:
                            f["remote_fin"] = True
                            if f.get("guest_fin"):
                                flows.pop(key, None)
                try:
                    down.send(frame)
                except OSError:
                    pass


if __name__ == "__main__":
    main()
