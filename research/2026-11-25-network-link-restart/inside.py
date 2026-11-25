"""Programs run inside the stand-in computer (nsenter -t PID -n python3 -I inside.py ...). Each
writes JSON to OUT and exits.

  probe HOST PORT SECONDS OUT          a new connection and request every 100 ms, 1 s timeout:
                                       one line "time ok|fail" each
  hold HOST PORT SILENT CAP OUT [ka]   one request whose reply comes after SILENT seconds of
                                       silence; gives up after CAP seconds. ka: TCP keep-alive
                                       after 5 s idle, every 2 s, 3 probes
  stream HOST PORT N OUT               a reply of N events, one every 0.5 s
  send HOST PORT SECONDS OUT           an upload: 1 KB every 100 ms for SECONDS
  echo PORT OUT                        a server: echoes lines; one JSON line per connection's end
  renew MAC IP OUT                     a DHCP renewal (REQUEST for IP) on tap0; the answer
  bulk HOST PORT up|down SECONDS OUT   one connection moving bytes for SECONDS; Gbit/s
"""
import json
import socket
import struct
import sys
import time


def w(out, d):
    d.setdefault("t_end", round(time.time(), 3))
    with open(out, "w") as f:
        json.dump(d, f)


def req(host, port, path, timeout=None, ka=False):
    s = socket.socket()
    if ka:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_KEEPALIVE, 1)
        s.setsockopt(socket.IPPROTO_TCP, socket.TCP_KEEPIDLE, 5)
        s.setsockopt(socket.IPPROTO_TCP, socket.TCP_KEEPINTVL, 2)
        s.setsockopt(socket.IPPROTO_TCP, socket.TCP_KEEPCNT, 3)
    s.settimeout(timeout)
    s.connect((host, port))
    s.sendall(("GET %s HTTP/1.0\r\n\r\n" % path).encode())
    return s


def main():
    cmd = sys.argv[1]
    a = sys.argv[2:]
    if cmd == "probe":
        host, port, secs, out = a[0], int(a[1]), float(a[2]), a[3]
        end = time.time() + secs
        with open(out, "w") as f:
            while time.time() < end:
                t = time.time()
                try:
                    s = req(host, port, "/fast", timeout=1)
                    ok = s.recv(100).startswith(b"HTTP/1.0 200")
                    s.close()
                except OSError:
                    ok = False
                f.write("%.3f %s\n" % (t, "ok" if ok else "fail"))
                f.flush()
                time.sleep(max(0, 0.1 - (time.time() - t)))
    elif cmd == "hold":
        host, port, silent, cap, out = a[0], int(a[1]), int(a[2]), float(a[3]), a[4]
        ka = len(a) > 5 and a[5] == "ka"
        t0 = time.time()
        try:
            s = req(host, port, "/slow?s=%d" % silent, timeout=cap, ka=ka)
            got = b""
            while True:
                b = s.recv(65536)
                if not b:
                    break
                got += b
            r = "whole" if got.endswith(b"done\n") else "cut (%d bytes)" % len(got)
        except socket.timeout:
            r = "still waiting at the cap"
        except OSError as e:
            r = "error: %s" % e.strerror
        w(out, {"result": r, "t_start": round(t0, 3), "keepalive": ka})
    elif cmd == "stream":
        host, port, n, out = a[0], int(a[1]), int(a[2]), a[3]
        t0 = time.time()
        events, last = 0, None
        try:
            s = req(host, port, "/stream?n=%d" % n, timeout=60)
            buf = b""
            while True:
                b = s.recv(65536)
                if not b:
                    r = "ended"
                    break
                buf += b
                c = buf.count(b"event ")
                if c > events:
                    events, last = c, time.time()
            r = "whole" if events == n else "ended after %d of %d events" % (events, n)
        except socket.timeout:
            r = "still waiting at the cap (60 s silent) after %d of %d events" % (events, n)
        except OSError as e:
            r = "error after %d of %d events: %s" % (events, n, e.strerror)
        w(out, {"result": r, "events": events, "t_last_event": last and round(last, 3), "t_start": round(t0, 3)})
    elif cmd == "send":
        host, port, secs, out = a[0], int(a[1]), float(a[2]), a[3]
        t0 = time.time()
        sent = 0
        try:
            s = req(host, port, "/sink", timeout=60)
            end = t0 + secs
            while time.time() < end:
                s.sendall(b"x" * 1024)
                sent += 1
                time.sleep(0.1)
            s.shutdown(socket.SHUT_WR)
            r = "whole" if s.recv(100) else "no answer"
        except socket.timeout:
            r = "still waiting at the cap (60 s) after %d KB" % sent
        except OSError as e:
            r = "error after %d KB: %s" % (sent, e.strerror)
        w(out, {"result": r, "kb": sent, "t_start": round(t0, 3)})
    elif cmd == "echo":
        port, out = int(a[0]), a[1]
        ls = socket.socket()
        ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        ls.bind(("0.0.0.0", port))
        ls.listen(64)
        import threading

        def serve(c):
            t0 = time.time()
            n = 0
            try:
                c.settimeout(60)
                while True:
                    b = c.recv(4096)
                    if not b:
                        r = "closed by the other side"
                        break
                    n += 1
                    c.sendall(b)
            except socket.timeout:
                r = "still waiting at the cap (60 s)"
            except OSError as e:
                r = "error: %s" % e.strerror
            with open(out, "a") as f:
                f.write(json.dumps({"result": r, "reads": n, "t_start": round(t0, 3), "t_end": round(time.time(), 3)}) + "\n")
        while True:
            c, _ = ls.accept()
            threading.Thread(target=serve, args=(c,), daemon=True).start()
    elif cmd == "renew":
        mac_s, ip, out = a[0], a[1], a[2]
        sys.path.insert(0, sys.argv[0].rsplit("/", 1)[0])
        import importlib.util
        spec = importlib.util.spec_from_file_location("nic", sys.argv[0].rsplit("/", 1)[0] + "/guest-nic.py")
        nic = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(nic)
        import random
        mac = bytes(int(x, 16) for x in mac_s.split(":"))
        s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(0x0800))
        s.bind(("tap0", 0))
        s.settimeout(0.5)
        xid = random.getrandbits(32)
        t0 = time.time()
        res = None
        for _ in range(10):
            s.send(nic.dhcp_frame(mac, xid, 3, ip))
            end = time.time() + 0.5
            while time.time() < end and not res:
                try:
                    f = s.recv(65600)
                except socket.timeout:
                    break
                r = nic.parse_dhcp(f, xid)
                if r:
                    res = r
            if res:
                break
        if res:
            res["answer"] = {5: "ACK", 6: "NAK"}.get(res["type"], str(res["type"]))
            res["same_address"] = res["address"] == ip
            res["seconds"] = round(time.time() - t0, 3)
        w(out, res or {"answer": "none in 5 s"})
    elif cmd == "bulk":
        host, port, way, secs, out = a[0], int(a[1]), a[2], float(a[3]), a[4]
        s = req(host, port, "/%s?s=%s" % ("sink" if way == "up" else "source", secs), timeout=30)
        n = 0
        t0 = time.time()
        if way == "up":
            buf = b"x" * 65536
            while time.time() - t0 < secs:
                s.sendall(buf)
                n += len(buf)
            s.shutdown(socket.SHUT_WR)
            s.recv(100)
        else:
            while True:
                b = s.recv(1 << 20)
                if not b:
                    break
                n += len(b)
        dt = time.time() - t0
        w(out, {"bytes": n, "seconds": round(dt, 3), "gbit_s": round(n * 8 / dt / 1e9, 3)})


if __name__ == "__main__":
    main()
