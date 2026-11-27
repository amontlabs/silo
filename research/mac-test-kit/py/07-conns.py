"""Many new connections to an SSH server, as fast as they go: how long each takes, which fail.

    07-conns.py (--unix PATH | --tcp HOST:PORT) --count N [--parallel P] [--wait S]

Each connection connects, reads the server's first line (an SSH server sends "SSH-2.0-..."
before anything else) and closes. That is the part of `ssh main` the network stack carries:
gvproxy's forward accepts on the Unix socket, opens a TCP connection inside, and copies bytes.
A connection that waits seconds for its first line is the TIME_WAIT stall research 04 names.

Prints one JSON object: count, parallel, ok, failed, errors by kind, seconds, connections per
second, p50/p99/max in ms, and how many took over 1 s and over 5 s.
"""
import argparse
import json
import socket
import threading
import time


def one(target, timeout):
    t0 = time.time()
    s = None
    try:
        if target[0] == "unix":
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        else:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(timeout)
        s.connect(target[1])
        data = b""
        while b"\n" not in data and len(data) < 512:
            chunk = s.recv(512)
            if not chunk:
                break
            data += chunk
        if not data.startswith(b"SSH-"):
            return time.time() - t0, "no-banner" if not data else "not-ssh"
        return time.time() - t0, None
    except socket.timeout:
        return time.time() - t0, "timeout"
    except OSError as e:
        return time.time() - t0, type(e).__name__ + ":" + str(e.errno)
    finally:
        if s is not None:
            s.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--unix")
    ap.add_argument("--tcp")
    ap.add_argument("--count", type=int, default=2000)
    ap.add_argument("--parallel", type=int, default=1)
    ap.add_argument("--wait", type=float, default=30.0, help="seconds before a connection counts as timed out")
    a = ap.parse_args()
    if a.unix:
        target = ("unix", a.unix)
    else:
        host, port = a.tcp.rsplit(":", 1)
        target = ("tcp", (host, int(port)))

    lock = threading.Lock()
    times, errors, left = [], {}, [a.count]

    def worker():
        while True:
            with lock:
                if left[0] <= 0:
                    return
                left[0] -= 1
            dt, err = one(target, a.wait)
            with lock:
                times.append(dt)
                if err:
                    errors[err] = errors.get(err, 0) + 1

    t0 = time.time()
    threads = [threading.Thread(target=worker) for _ in range(max(1, a.parallel))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    total = time.time() - t0
    times.sort()

    def pct(p):
        return round(times[min(len(times) - 1, int(p * len(times)))] * 1000, 2) if times else None

    failed = sum(errors.values())
    print(json.dumps({
        "target": a.unix or a.tcp, "count": a.count, "parallel": a.parallel,
        "ok": a.count - failed, "failed": failed, "errors": errors,
        "seconds": round(total, 3), "per_second": round(a.count / total, 1) if total else None,
        "p50_ms": pct(0.5), "p99_ms": pct(0.99), "max_ms": pct(1.0),
        "over_1s": sum(1 for t in times if t > 1), "over_5s": sum(1 for t in times if t > 5),
    }))


if __name__ == "__main__":
    main()
