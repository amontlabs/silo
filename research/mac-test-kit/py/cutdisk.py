"""A disk with a volatile write cache, served over NBD, that can lose its cache as a drive does in a
power cut. For test 51 (and its Linux self-check).

A Mac's drive acknowledges a write once it is in its cache, and writes the cache out later, in any
order; only a flush (what F_FULLFSYNC asks for) makes what came before it durable. A macOS
computer whose disk this serves runs the real APFS and the real flush calls of the program under
test; the cache here plays the drive. `cut` is the power cut: each block written since the last
flush reaches the image with probability KEEP, in a random order, and the rest is lost; then
every request fails, as a disk without power does, until the server is stopped.

  cutdisk.py serve --image PATH (--unix SOCK | --tcp HOST:PORT) --control SOCK [--seed N]
      Serves PATH (a raw image) to one client after another, prints one JSON event per line.
      Advertises flush and FUA. A write lands in the cache; a FUA write and a flush reach the image.
  cutdisk.py ctl SOCK stats | cut KEEP | quit
      One request to a running server's control socket; prints its JSON answer. `cut` answers
      with how many cached blocks were kept and lost.
  cutdisk.py selfcheck
      Serves a small image, writes and flushes as a client, cuts, and checks the image holds every
      flushed block and only blocks that were written; prints PASS or FAIL. CUTDISK_MUTANT=flush-noop
      plants a flush that does nothing, which it must catch.

Python 3.9, standard library only. The NBD handshake is py/nbdserve.py's, loaded from its path.
"""
import importlib.util
import json
import os
import random
import socket
import struct
import sys
import tempfile
import threading
import time

_spec = importlib.util.spec_from_file_location("nbdserve", os.path.join(os.path.dirname(os.path.abspath(__file__)), "nbdserve.py"))
nbd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(nbd)

BLOCK = 4096
EIO, EINVAL = 5, 22
# For the self-check: a planted bug, a flush that leaves the cache where it is.
MUTANT = os.environ.get("CUTDISK_MUTANT", "")


class CutDisk:
    def __init__(self, path, seed=None):
        self.path = os.path.abspath(path)
        self.fd = os.open(self.path, os.O_RDWR)
        self.size = os.fstat(self.fd).st_size
        self.lock = threading.Lock()
        self.cache = {}  # block -> 4 KiB, written since the last flush
        self.dead = False
        self.rng = random.Random(seed)
        self.stats = {"read": 0, "write": 0, "flush": 0, "fua": 0, "trim": 0, "connections": 0,
                      "cached_blocks": 0, "max_cached_blocks": 0, "flushed_blocks": 0, "cut": None}

    def _block(self, b):
        if b in self.cache:
            return self.cache[b]
        d = os.pread(self.fd, BLOCK, b * BLOCK)
        return d + b"\0" * (BLOCK - len(d))

    def read(self, off, ln):
        with self.lock:
            out = bytearray()
            for b in range(off // BLOCK, (off + ln - 1) // BLOCK + 1):
                out += self._block(b)
            s = off - (off // BLOCK) * BLOCK
            return bytes(out[s:s + ln])

    def write(self, off, data, fua):
        with self.lock:
            first, last = off // BLOCK, (off + len(data) - 1) // BLOCK
            pos = 0
            for b in range(first, last + 1):
                cur = bytearray(self._block(b))
                s = off + pos - b * BLOCK
                n = min(BLOCK - s, len(data) - pos)
                cur[s:s + n] = data[pos:pos + n]
                pos += n
                if fua:
                    os.pwrite(self.fd, bytes(cur), b * BLOCK)
                    self.cache.pop(b, None)
                else:
                    self.cache[b] = bytes(cur)
            if fua:
                nbd.full_sync(self.fd)
            self.stats["cached_blocks"] = len(self.cache)
            self.stats["max_cached_blocks"] = max(self.stats["max_cached_blocks"], len(self.cache))

    def flush(self):
        if MUTANT == "flush-noop":
            return
        with self.lock:
            for b in sorted(self.cache):
                os.pwrite(self.fd, self.cache[b], b * BLOCK)
            nbd.full_sync(self.fd)
            self.stats["flushed_blocks"] += len(self.cache)
            self.cache = {}
            self.stats["cached_blocks"] = 0

    def cut(self, keep):
        """The power cut. Returns what reached the image."""
        with self.lock:
            blocks = list(self.cache)
            self.rng.shuffle(blocks)
            kept = [b for b in blocks if self.rng.random() < keep]
            for b in kept:
                os.pwrite(self.fd, self.cache[b], b * BLOCK)
            nbd.full_sync(self.fd)
            self.dead = True
            r = {"cached": len(blocks), "kept": len(kept), "lost": len(blocks) - len(kept), "keep": keep}
            self.cache = {}
            self.stats["cut"] = r
            nbd.emit(event="cut", **r)
            return r


def serve_conn(c, disk):
    st = disk.stats
    if not nbd.handshake(c, disk):
        return
    while True:
        try:
            h = nbd.recv_exact(c, 28)
        except EOFError:
            return
        magic, flags, typ, handle, off, ln = struct.unpack(">IHHQQI", h)
        if magic != nbd.REQ_MAGIC:
            raise ValueError("bad request magic %x" % magic)
        data = nbd.recv_exact(c, ln) if typ == nbd.CMD_WRITE else None
        if typ == nbd.CMD_DISC:
            return
        if disk.dead:
            # No power: nothing is acknowledged any more.
            c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, EIO, handle))
            continue
        bad = ln > nbd.MAX_REQ or off + ln > disk.size
        if typ == nbd.CMD_READ:
            st["read"] += 1
            if bad:
                c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, EINVAL, handle))
                continue
            c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, 0, handle) + disk.read(off, ln))
        elif typ == nbd.CMD_WRITE:
            st["write"] += 1
            if bad:
                c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, EINVAL, handle))
                continue
            fua = bool(flags & nbd.CMD_FLAG_FUA)
            st["fua"] += fua
            disk.write(off, data, fua)
            c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, 0, handle))
        elif typ == nbd.CMD_FLUSH:
            st["flush"] += 1
            disk.flush()
            c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, 0, handle))
        elif typ == nbd.CMD_TRIM:
            st["trim"] += 1  # a server may ignore a trim
            c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, 0, handle))
        else:
            c.sendall(struct.pack(">IIQ", nbd.SIMPLE_REPLY, EINVAL, handle))


def control(sock_path, disk, stop):
    if os.path.exists(sock_path):
        os.unlink(sock_path)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(sock_path)
    s.listen(4)
    while not stop.is_set():
        c, _ = s.accept()
        with c:
            words = c.recv(4096).decode().split()
            if not words:
                continue
            if words[0] == "stats":
                r = disk.stats
            elif words[0] == "cut" and len(words) == 2:
                r = disk.cut(float(words[1]))
            elif words[0] == "quit":
                r = {"ok": True}
                stop.set()
            else:
                r = {"error": "unknown request"}
            c.sendall(json.dumps(r).encode())
    os._exit(0)


def serve(args):
    disk = CutDisk(args["image"], int(args["seed"]) if args.get("seed") else None)
    stop = threading.Event()
    threading.Thread(target=control, args=(args["control"], disk, stop), daemon=True).start()
    srv = nbd.listen_on(args)
    nbd.emit(event="listening", size=disk.size)
    while not stop.is_set():
        c, _ = srv.accept()
        disk.stats["connections"] += 1
        nbd.emit(event="connected")
        try:
            serve_conn(c, disk)
        except (OSError, ValueError, EOFError) as e:
            nbd.emit(event="error", error=str(e))
        finally:
            c.close()
            nbd.emit(event="left")


def ctl(sock_path, words):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(sock_path)
    s.sendall(" ".join(words).encode())
    s.shutdown(socket.SHUT_WR)
    out = b""
    while True:
        k = s.recv(65536)
        if not k:
            break
        out += k
    print(out.decode())


class Client:
    """Just enough of an NBD client for the self-check: option GO, then requests."""

    def __init__(self, sock_path):
        self.s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.s.connect(sock_path)
        nbd.recv_exact(self.s, 18)
        self.s.sendall(struct.pack(">I", nbd.FLAG_FIXED_NEWSTYLE | nbd.FLAG_NO_ZEROES))
        self.s.sendall(struct.pack(">QII", nbd.IHAVEOPT, nbd.OPT_GO, 6) + struct.pack(">IH", 0, 0))
        while True:
            _, _, typ, ln = struct.unpack(">QIII", nbd.recv_exact(self.s, 20))
            if ln:
                nbd.recv_exact(self.s, ln)
            if typ == nbd.REP_ACK:
                break
        self.h = 0

    def req(self, typ, off=0, data=b"", ln=0, flags=0):
        self.h += 1
        self.s.sendall(struct.pack(">IHHQQI", nbd.REQ_MAGIC, flags, typ, self.h, off, len(data) or ln) + data)
        _, err, _ = struct.unpack(">IIQ", nbd.recv_exact(self.s, 16))
        return err, (nbd.recv_exact(self.s, ln) if typ == nbd.CMD_READ and err == 0 else b"")


def selfcheck():
    ok = True
    with tempfile.TemporaryDirectory() as d:
        for keep in (0.0, 0.5, 1.0):
            img = os.path.join(d, "disk.img")
            with open(img, "wb") as f:
                f.truncate(64 * BLOCK)
            sock, ctl_sock = os.path.join(d, "n.sock"), os.path.join(d, "c.sock")
            disk = CutDisk(img, 7)
            stop = threading.Event()
            threading.Thread(target=control, args=(ctl_sock, disk, stop), daemon=True).start()
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            if os.path.exists(sock):
                os.unlink(sock)
            srv.bind(sock)
            srv.listen(1)
            t = threading.Thread(target=lambda: serve_conn(srv.accept()[0], disk), daemon=True)
            t.start()
            while not os.path.exists(ctl_sock):
                time.sleep(0.01)
            c = Client(sock)
            pattern = lambda i: struct.pack("<Q", 1000 + i) * (BLOCK // 8)
            for i in range(0, 16):  # flushed
                c.req(nbd.CMD_WRITE, i * BLOCK, pattern(i))
            c.req(nbd.CMD_FLUSH)
            c.req(nbd.CMD_WRITE, 16 * BLOCK, pattern(16), flags=nbd.CMD_FLAG_FUA)  # durable alone
            for i in range(17, 48):  # cached only
                c.req(nbd.CMD_WRITE, i * BLOCK, pattern(i))
            err, back = c.req(nbd.CMD_READ, 20 * BLOCK, ln=BLOCK)
            reads_cache = err == 0 and back == pattern(20)
            r = disk.cut(keep)
            err_after, _ = c.req(nbd.CMD_WRITE, 0, pattern(99))
            with open(img, "rb") as f:
                got = [f.read(BLOCK) for _ in range(64)]
            flushed_held = all(got[i] == pattern(i) for i in range(17))
            only_written = all(got[i] in (pattern(i), b"\0" * BLOCK) for i in range(17, 64))
            kept = sum(got[i] == pattern(i) for i in range(17, 48))
            want_kept = {0.0: kept == 0, 1.0: kept == 31, 0.5: 0 < kept < 31}[keep]
            good = reads_cache and flushed_held and only_written and want_kept and err_after == EIO and r["kept"] == kept
            ok &= good
            print(json.dumps({"keep": keep, "cached_kept": kept, "of": 31, "reads_see_cache": reads_cache,
                              "flushed_held": flushed_held, "refuses_after_cut": err_after == EIO, "ok": good}))
            c.s.close()
            srv.close()
            os.unlink(img)
    print("PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv):
    if len(argv) >= 1 and argv[0] == "selfcheck":
        return selfcheck()
    if len(argv) >= 2 and argv[0] == "ctl":
        ctl(argv[1], argv[2:])
        return 0
    if argv and argv[0] == "serve":
        args, i = {}, 1
        while i < len(argv):
            args[argv[i].lstrip("-")] = argv[i + 1]
            i += 2
        serve(args)
        return 0
    sys.stderr.write(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
