"""A stand-in for the part of Silo's helper that owns one computer's network link, with gvproxy as
its child. Five ways to hold the link, the first being what silovm and vfkit do today:

  direct    the computer's socket is bound by the helper and connected to gvproxy's socket, then
            handed to the network card; after a restart of gvproxy nothing is done.
  repoint   the same socket, but the helper keeps its own copy of it and, once a new gvproxy is up,
            connects that copy again to gvproxy's socket and says "VFKT". The card's writes then go
            to the new gvproxy: no copy, no patch, one connect() per restart.
  fd        a socketpair: the card holds one end, gvproxy is started with the other as an inherited descriptor
            (gvproxy-fd.patch, `vfkit: fd://N`), and the helper keeps a copy of that end to hand to
            the next gvproxy. No copy, no address to learn.
  relay     a socketpair to relay.c, which connects to gvproxy and connects again after a restart
            (the stand-in #390 used, in C).
  rstrelay  rstrelay.py: the relay, plus a reset sent into the computer for each connection open at
            the restart.

After each start of gvproxy the helper also opens the forward of the computer's SSH again (gvproxy
keeps its forwards in memory only).

Used by bench.py; as a library only.
"""
import json
import os
import signal
import socket
import subprocess
import time
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
GUEST_IP = "192.168.127.2"
GATEWAY = "192.168.127.1"
MAC = "5a:94:ef:4b:11:37"
MODES = ("direct", "repoint", "fd", "relay", "rstrelay")


def api(d, path, body=None):
    """One call to gvproxy's API on its Unix socket. Returns (status, body)."""
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(5)
    s.connect(os.path.join(d, "api.sock"))
    data = json.dumps(body).encode() if body is not None else b""
    req = "%s %s HTTP/1.0\r\nHost: gvproxy\r\nContent-Length: %d\r\n\r\n" % ("POST" if body is not None else "GET", path, len(data))
    s.sendall(req.encode() + data)
    out = b""
    while True:
        b = s.recv(65536)
        if not b:
            break
        out += b
    s.close()
    head, _, rest = out.partition(b"\r\n\r\n")
    return int(head.split()[1]), rest


class Link:
    def __init__(self, mode, d, gvproxy, gvproxy_fd, mtu=1500, bins="/tmp/linkbin"):
        assert mode in MODES
        self.mode, self.d, self.mtu, self.bins = mode, d, mtu, bins
        self.gvproxy = gvproxy_fd if mode == "fd" else gvproxy
        os.makedirs(d, exist_ok=True)
        os.chmod(d, 0o700)
        self.gv = self.guest = self.relay = None
        self.keep = None       # the helper's own copy of the socket it re-links
        self.events = []
        self.forwards = []     # (local unix path, remote tcp://ip:port), opened again after each start

    def ev(self, name, **kw):
        kw.update(event=name, t=round(time.time(), 3))
        self.events.append(kw)

    def _config(self):
        vfkit = "fd://%d" % self.keep.fileno() if self.mode == "fd" else "unixgram://%s/gv.sock" % self.d
        with open(os.path.join(self.d, "gvproxy.yaml"), "w") as f:
            f.write("""log-level: info
listen:
  - unix://{d}/api.sock
interfaces:
  vfkit: {vfkit}
log-file: {d}/gvproxy.log
stack:
  mtu: {mtu}
  subnet: 192.168.127.0/24
  gatewayIP: {gw}
  deviceIP: {ip}
  dhcpStaticLeases:
    {ip}: "{mac}"
""".format(d=self.d, vfkit=vfkit, mtu=self.mtu, gw=GATEWAY, ip=GUEST_IP, mac=MAC))

    def _start_gvproxy(self):
        for n in ("gv.sock", "api.sock"):
            try:
                os.unlink(os.path.join(self.d, n))
            except OSError:
                pass
        kw = {}
        if self.mode == "fd":
            # gvproxy inherits the helper's end of the socketpair under the same number; the
            # helper keeps its own copy open, so the link outlives this gvproxy.
            kw = {"pass_fds": (self.keep.fileno(),)}
        out = open(os.path.join(self.d, "gvproxy.out"), "a")
        self.gv = subprocess.Popen([self.gvproxy, "-config", os.path.join(self.d, "gvproxy.yaml")],
                                   stdout=out, stderr=out, **kw)
        need = ["api.sock"] + ([] if self.mode == "fd" else ["gv.sock"])
        t0 = time.time()
        while not all(os.path.exists(os.path.join(self.d, n)) for n in need):
            if self.gv.poll() is not None or time.time() - t0 > 10:
                raise RuntimeError("gvproxy did not start: " + open(os.path.join(self.d, "gvproxy.out")).read()[-500:])
            time.sleep(0.01)
        self.ev("gvproxy-up", pid=self.gv.pid)

    def _vm_socket(self, connect_to_gvproxy=True):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1 << 20)   # vfkit's sizes
        s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
        p = os.path.join(self.d, "vm.sock")
        try:
            os.unlink(p)
        except OSError:
            pass
        s.bind(p)
        if connect_to_gvproxy:
            s.connect(os.path.join(self.d, "gv.sock"))
            s.send(b"VFKT")
        return s

    def start(self):
        if self.mode == "fd":
            card, self.keep = socket.socketpair(socket.AF_UNIX, socket.SOCK_DGRAM)
            for s in (card, self.keep):
                s.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1 << 20)
                s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
        self._config()
        if self.mode == "fd":
            self._start_gvproxy()
        elif self.mode in ("direct", "repoint"):
            self._start_gvproxy()
            card = self._vm_socket()
            if self.mode == "repoint":
                self.keep = socket.socket(fileno=os.dup(card.fileno()))
        else:
            self._start_gvproxy()
            card, mine = socket.socketpair(socket.AF_UNIX, socket.SOCK_DGRAM)
            for s in (card, mine):
                s.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1 << 20)
                s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
            os.set_inheritable(mine.fileno(), True)
            argv = ([os.path.join(self.bins, "relay")] if self.mode == "relay"
                    else ["python3", "-I", os.path.join(HERE, "rstrelay.py")])
            argv += [str(mine.fileno()), os.path.join(self.d, "gv.sock"), os.path.join(self.d, "relay-up.sock")]
            self.relay = subprocess.Popen(argv, pass_fds=(mine.fileno(),), stdout=open(os.path.join(self.d, "relay.log"), "a"),
                                          stderr=subprocess.STDOUT)
            mine.close()
            time.sleep(0.3)
        os.set_inheritable(card.fileno(), True)
        ready = os.path.join(self.d, "guest-ready.json")
        try:
            os.unlink(ready)
        except OSError:
            pass
        self.guest = subprocess.Popen(["unshare", "-n", "python3", "-I", os.path.join(HERE, "guest-nic.py"), "--fd", str(card.fileno()),
                                       "--mac", MAC, "--ready", ready, "--pump", os.path.join(self.bins, "nicpump")],
                                      pass_fds=(card.fileno(),), stdout=open(os.path.join(self.d, "guest.log"), "a"), stderr=subprocess.STDOUT)
        card.close()
        t0 = time.time()
        while not os.path.exists(ready):
            if self.guest.poll() is not None or time.time() - t0 > 30:
                raise RuntimeError("the computer's card got no address: " + open(os.path.join(self.d, "guest.log")).read()[-500:])
            time.sleep(0.05)
        self.lease = json.load(open(ready))
        self.ev("computer-up", lease=self.lease)
        return self.lease

    def guest_pid(self):
        # unshare execs python, which execs nicpump: the pid of the unshare process is the card's.
        return self.guest.pid

    def forward(self, local, remote):
        self.forwards.append((local, remote))
        return self._forward(local, remote)

    def _forward(self, local, remote):
        try:
            os.unlink(local)
        except OSError:
            pass
        st, body = api(self.d, "/services/forwarder/expose", {"local": local, "protocol": "unix", "remote": remote})
        t0 = time.time()
        while st == 200 and not os.path.exists(local) and time.time() - t0 < 5:
            time.sleep(0.01)
        return st

    def kill_stack(self, sig=signal.SIGKILL):
        """gvproxy dies (SIGKILL: a crash; SIGTERM: a stop asked for)."""
        self.ev("gvproxy-kill", sig=int(sig))
        self.gv.send_signal(sig)
        self.gv.wait()

    def start_stack_again(self):
        """What the helper does when its gvproxy has died: start it again and re-link."""
        self._start_gvproxy()
        if self.mode == "repoint":
            self.keep.connect(os.path.join(self.d, "gv.sock"))
            self.keep.send(b"VFKT")
        elif self.mode in ("relay", "rstrelay"):
            self.relay.send_signal(signal.SIGUSR1)
        self.ev("relinked")
        for local, remote in self.forwards:
            st = self._forward(local, remote)
            self.ev("forward-again", local=local, status=st)

    def stop(self):
        for p in (self.guest, self.relay, self.gv):
            if p and p.poll() is None:
                p.kill()
                p.wait()
