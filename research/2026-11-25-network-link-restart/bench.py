"""#390's restart test, against each way a helper can hold a computer's network link (link.py).

    python3 bench.py restart [--modes direct,repoint,fd,relay,rstrelay] [--runs 3] [--out DIR]
    python3 bench.py speed   [--modes direct,repoint,fd,relay] [--runs 3] [--mtu 1500] [--out DIR]

Linux, as root, with /dev/net/tun, unshare and nsenter. Needs gvproxy v0.9.0 as released
(GVPROXY, checked against the release's sha256) and the same built with gvproxy-fd.patch
(GVPROXY_FD), and nicpump and relay built from nicpump.c and relay.c (BINS).

restart: a computer runs, with things open across the restart: a new request every 100 ms, a
reply held silent for 25 s (with and without TCP keep-alive), a reply streamed for 30 s, an upload,
and a session coming in through gvproxy's forward. gvproxy is killed (SIGKILL), started again 1 s
later, and the helper re-links and opens the forward again. Measured: when new connections work
again, what became of each open one, when a new connection comes in again, and what a DHCP renewal
gets. speed: one connection out and one in, 5 s each, no restart.
"""
import argparse
import hashlib
import importlib.util
import json
import os
import signal
import socket
import statistics
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("link", os.path.join(HERE, "link.py"))
link = importlib.util.module_from_spec(spec)
spec.loader.exec_module(link)

GVPROXY = os.environ.get("GVPROXY", "/tmp/silo-kit-35-home/bin/gvproxy-v0.9.0")
GVPROXY_SHA = "95c0ee5b5e5d401094ed58ac2f984ba5e935c2cb416c8752329fcfacb721aa9a"
GVPROXY_FD = os.environ.get("GVPROXY_FD", "/tmp/src/gvproxy-fd")
BINS = os.environ.get("BINS", "/tmp/linkbin")
SLOW, CAP = 25, 60


class Server:
    """The internet, on this machine's address: /fast, /slow?s=N (headers at once, then N s of
    silence, then "done"), /stream?n=N (an event every 0.5 s), /sink (reads to the end, answers),
    /source?s=N (sends for N s). Logs what each connection met."""

    def __init__(self, host):
        self.ls = socket.socket()
        self.ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.ls.bind((host, 0))
        self.ls.listen(256)
        self.port = self.ls.getsockname()[1]
        self.log = []
        threading.Thread(target=self.run, daemon=True).start()

    def run(self):
        while True:
            c, _ = self.ls.accept()
            threading.Thread(target=self.one, args=(c,), daemon=True).start()

    def one(self, c):
        t0 = time.time()
        path = "?"
        try:
            line = b""
            while b"\r\n\r\n" not in line:
                b = c.recv(4096)
                if not b:
                    return
                line += b
            path = line.split()[1].decode()
            arg = float(path.split("=")[1]) if "=" in path else 0
            if path == "/fast":
                c.sendall(b"HTTP/1.0 200 OK\r\n\r\nok\n")
            elif path.startswith("/slow"):
                c.sendall(b"HTTP/1.0 200 OK\r\n\r\n")
                time.sleep(arg)
                c.sendall(b"done\n")
            elif path.startswith("/stream"):
                c.sendall(b"HTTP/1.0 200 OK\r\n\r\n")
                for i in range(int(arg)):
                    c.sendall(b"event %d\n" % i)
                    time.sleep(0.5)
            elif path.startswith("/sink"):
                while c.recv(1 << 20):
                    pass
                c.sendall(b"HTTP/1.0 200 OK\r\n\r\nok\n")
            elif path.startswith("/source"):
                c.sendall(b"HTTP/1.0 200 OK\r\n\r\n")
                buf = b"y" * 65536
                end = time.time() + arg
                while time.time() < end:
                    c.sendall(buf)
            r = "whole"
        except OSError as e:
            r = "error: %s" % e.strerror
        finally:
            c.close()
        if path != "/fast":
            self.log.append({"path": path, "result": r, "t_start": round(t0, 3), "t_end": round(time.time(), 3)})


def inside(L, *args, wait=True):
    argv = ["nsenter", "-t", str(L.guest_pid()), "-n", "python3", "-I", os.path.join(HERE, "inside.py")] + [str(a) for a in args]
    p = subprocess.Popen(argv, stdout=subprocess.DEVNULL, stderr=open(os.path.join(L.d, "inside.err"), "a"))
    if wait:
        p.wait()
    return p


def echo_once(path, timeout=1.0):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    try:
        s.connect(path)
        s.sendall(b"ping\n")
        return s.recv(10) == b"ping\n"
    except OSError:
        return False
    finally:
        s.close()


def session_in(path, secs, res):
    """A session coming into the computer through gvproxy's forward: a line every 0.5 s."""
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(2)
    t0 = time.time()
    n = 0
    try:
        s.connect(path)
        while time.time() - t0 < secs:
            s.sendall(b"line\n")
            if s.recv(10) != b"line\n":
                raise ConnectionError("closed")
            n += 1
            time.sleep(0.5)
        r = "whole"
    except (OSError, ConnectionError) as e:
        r = "error: %s" % (getattr(e, "strerror", None) or e)
    res.update(result=r, echoes=n, t_start=round(t0, 3), t_end=round(time.time(), 3))


def one_restart(mode, d, host, srv):
    L = link.Link(mode, d, GVPROXY, GVPROXY_FD, bins=BINS)
    out = {"mode": mode}
    try:
        L.start()
        echo_sock = os.path.join(d, "echo.sock")
        inside(L, "echo", 7000, os.path.join(d, "echo.jsonl"), wait=False)
        time.sleep(0.3)
        L.forward(echo_sock, "tcp://%s:7000" % link.GUEST_IP)
        f = lambda n: os.path.join(d, n)
        for n in ("probe.txt", "hold.json", "hold-ka.json", "stream.json", "send.json", "renew.json", "echo.jsonl"):
            try:
                os.unlink(f(n))
            except OSError:
                pass
        ps = [inside(L, "probe", host, srv.port, 40, f("probe.txt"), wait=False),
              inside(L, "hold", host, srv.port, SLOW, CAP, f("hold.json"), wait=False),
              inside(L, "hold", host, srv.port, SLOW, CAP, f("hold-ka.json"), "ka", wait=False),
              inside(L, "stream", host, srv.port, 60, f("stream.json"), wait=False),
              inside(L, "send", host, srv.port, 20, f("send.json"), wait=False)]
        sess = {}
        th = threading.Thread(target=session_in, args=(echo_sock, 25, sess))
        th.start()
        time.sleep(5)
        L.kill_stack(signal.SIGKILL)
        t_kill = time.time()
        time.sleep(1.0)
        L.start_stack_again()
        t_up = time.time()
        back_in = None
        while time.time() - t_up < 30:
            if echo_once(echo_sock):
                back_in = round(time.time() - t_up, 2)
                break
            time.sleep(0.1)
        time.sleep(max(0, 2 - (time.time() - t_up)))
        inside(L, "renew", link.MAC, link.GUEST_IP, f("renew.json"))
        for p in ps:
            p.wait(timeout=CAP + 30)
        th.join()
        time.sleep(0.5)
        rows = [l.split() for l in open(f("probe.txt"))]
        first_ok = next((float(t) for t, s in rows if float(t) > t_up and s == "ok"), None)
        last_ok = max((float(t) for t, s in rows if float(t) < t_kill and s == "ok"), default=None)
        out.update(
            t_kill=round(t_kill, 3), t_up=round(t_up, 3),
            new_connections_back_s=round(first_ok - t_up, 2) if first_ok else None,
            outage_seen_inside_s=round(first_ok - last_ok, 2) if first_ok and last_ok else None,
            failed_requests=sum(1 for _, s in rows if s == "fail"),
            new_session_in_back_s=back_in,
        )
        for name in ("hold", "hold-ka", "stream", "send"):
            r = json.load(open(f(name + ".json")))
            r["ended_s_after_up"] = round(r["t_end"] - t_up, 1)
            out[name] = r
        out["session_in_host_side"] = dict(sess, ended_s_after_up=round(sess["t_end"] - t_up, 1))
        ends = [json.loads(l) for l in open(f("echo.jsonl"))] if os.path.exists(f("echo.jsonl")) else []
        first = [e for e in ends if e["t_start"] < t_kill]
        out["session_in_guest_side"] = (dict(first[0], ended_s_after_up=round(first[0]["t_end"] - t_up, 1)) if first
                                        else "still open when the run ended")
        out["renew"] = json.load(open(f("renew.json")))
        out["events"] = L.events
        if mode in ("relay", "rstrelay"):
            out["relay_log"] = [json.loads(l) for l in open(f("relay.log")) if l.startswith("{")]
        out["card_log"] = open(f("guest.log")).read()[-400:]
    finally:
        L.stop()
        subprocess.run(["pkill", "-f", "inside.py echo 7000"], stderr=subprocess.DEVNULL)
    return out


def one_speed(mode, d, host, srv, mtu):
    L = link.Link(mode, d, GVPROXY, GVPROXY_FD, mtu=mtu, bins=BINS)
    try:
        L.start()
        r = {}
        for way in ("up", "down"):
            p = os.path.join(d, "bulk-%s.json" % way)
            inside(L, "bulk", host, srv.port, way, 5, p)
            r[way] = json.load(open(p))["gbit_s"]
        return r
    finally:
        L.stop()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("what", choices=("restart", "speed"))
    ap.add_argument("--modes", default=None)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--mtu", type=int, default=1500)
    ap.add_argument("--out", default=os.path.join(HERE, "results"))
    a = ap.parse_args()
    if hashlib.sha256(open(GVPROXY, "rb").read()).hexdigest() != GVPROXY_SHA:
        sys.exit("GVPROXY is not gvproxy v0.9.0 as released")
    host = subprocess.run(["hostname", "-I"], capture_output=True, text=True).stdout.split()[0]
    srv = Server(host)
    os.makedirs(a.out, exist_ok=True)
    modes = (a.modes or ("direct,repoint,fd,relay,rstrelay" if a.what == "restart" else "direct,repoint,fd,relay")).split(",")
    results = []
    for run in range(a.runs):
        for mode in modes:
            d = "/tmp/lk-%s" % mode
            if a.what == "restart":
                r = one_restart(mode, d, host, srv)
            else:
                r = {"mode": mode, "mtu": a.mtu, **one_speed(mode, d, host, srv, a.mtu)}
            r["run"] = run + 1
            results.append(r)
            brief = {k: r[k] for k in r if k in ("mode", "run", "new_connections_back_s", "new_session_in_back_s", "up", "down")}
            for k in ("hold", "hold-ka", "stream", "send"):
                if k in r:
                    brief[k] = "%s @%s" % (r[k]["result"], r[k]["ended_s_after_up"])
            print(json.dumps(brief), flush=True)
    name = "%s%s.json" % (a.what, "" if a.what == "restart" else "-mtu%d" % a.mtu)
    with open(os.path.join(a.out, name), "w") as f:
        json.dump({"server": srv.log, "runs": results}, f, indent=1)


if __name__ == "__main__":
    main()
