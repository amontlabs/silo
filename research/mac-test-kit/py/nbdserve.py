"""A disk served over NBD with checkpoints at a moment, for test 32 (and its Linux analogue).

It stands in for the checkpoint engine's NBD server (research/2026-11-24-checkpoint-engine,
proto/src/nbd.zig) where that server cannot do what the test needs: its `nbd-serve` takes no
checkpoint and listens only on a Unix socket. This one takes the same copy-before-write checkpoint
(the prototype's `cbw`) through a control socket, while the guest writes. It is Python, so its cost
per write says nothing of Silo's; test 32 measures that with the prototype's server.

  nbdserve.py serve --image PATH (--unix SOCK | --tcp HOST:PORT) [--control SOCK] [--block BYTES]
      Serves PATH (a raw image) to one client after another. Prints one JSON event per line:
      listening, connected, left, error. Advertises flush, FUA and TRIM; a TRIM is counted, not
      applied (NBD lets a server ignore it).
  nbdserve.py ctl SOCK stats | checkpoint DST | quit
      One request to a running server's control socket; prints its JSON answer. The first
      checkpoint clones the live image with no write in flight (the guest's writes wait for the
      clone); each later one clones the last checkpoint, which nobody writes, takes its moment
      between two writes (a swap of two sets of blocks), then copies the blocks written since,
      a block the guest is about to overwrite first.
  nbdserve.py probe URL
      Connects as a client (option GO), reads the first 4 KiB, disconnects; prints size and flags.
  nbdserve.py writer URL SECONDS LOG [RATE]
      A client that writes 4 KiB blocks at random places, RATE a second (200), each block holding
      its sequence number and place; LOG gets "seq block" per acknowledged write.
  nbdserve.py verify IMAGE LOG [BLOCKS]
      Checks that IMAGE holds exactly the writes of LOG up to one moment; prints JSON.

Python 3.9, standard library only.
"""
import ctypes
import json
import os
import random
import socket
import struct
import sys
import threading
import time
from urllib.parse import parse_qs, urlparse

NBDMAGIC = 0x4E42444D41474943
IHAVEOPT = 0x49484156454F5054
REPLY_MAGIC = 0x3E889045565A9
REQ_MAGIC = 0x25609513
SIMPLE_REPLY = 0x67446698
FLAG_FIXED_NEWSTYLE, FLAG_NO_ZEROES = 1, 2
TX_HAS_FLAGS, TX_READ_ONLY, TX_SEND_FLUSH, TX_SEND_FUA, TX_SEND_TRIM = 1, 2, 4, 8, 32
OPT_EXPORT_NAME, OPT_ABORT, OPT_LIST, OPT_INFO, OPT_GO = 1, 2, 3, 6, 7
REP_ACK, REP_SERVER, REP_INFO, REP_ERR_UNSUP = 1, 2, 3, 0x80000001
INFO_EXPORT, INFO_BLOCK_SIZE = 0, 3
CMD_READ, CMD_WRITE, CMD_DISC, CMD_FLUSH, CMD_TRIM = 0, 1, 2, 3, 4
CMD_FLAG_FUA = 1
MAX_REQ = 32 << 20
EINVAL, EIO = 22, 5
# For the kit's own check (linux/test-ckpt.sh): a planted bug, and a slower copy that widens the
# window in which the guest overwrites a block not yet copied.
MUTANT = os.environ.get("NBDSERVE_MUTANT", "")
COPY_DELAY = float(os.environ.get("NBDSERVE_COPY_DELAY", "0") or 0)
WMAGIC = 0x5110C4EC5110C4EC
CMD_NAMES = {0: "read", 1: "write", 2: "disc", 3: "flush", 4: "trim"}


def emit(**kw):
    kw.setdefault("t", round(time.time(), 6))
    sys.stdout.write(json.dumps(kw) + "\n")
    sys.stdout.flush()


def recv_exact(s, n):
    buf = bytearray(n)
    view = memoryview(buf)
    got = 0
    while got < n:
        k = s.recv_into(view[got:], n - got)
        if k == 0:
            raise EOFError("closed")
        got += k
    return bytes(buf)


def full_sync(fd):
    """F_FULLFSYNC on a Mac (the flush that reaches the disk), fsync elsewhere."""
    import fcntl
    if hasattr(fcntl, "F_FULLFSYNC"):
        try:
            fcntl.fcntl(fd, fcntl.F_FULLFSYNC)
            return
        except OSError:
            pass
    os.fsync(fd)


def clone_file(src, dst):
    """clonefile(2) on a Mac, FICLONE on Linux. Returns how ("clonefile", "ficlone")."""
    if sys.platform == "darwin":
        libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
        if libc.clonefile(src.encode(), dst.encode(), 0) != 0:
            e = ctypes.get_errno()
            raise OSError(e, "clonefile %s: %s" % (src, os.strerror(e)))
        return "clonefile"
    import fcntl
    with open(src, "rb") as fs, open(dst, "wb") as fd:
        fcntl.ioctl(fd.fileno(), 0x40049409, fs.fileno())  # FICLONE
    return "ficlone"


class Disk:
    """The served image, the blocks written since the last checkpoint, and the checkpoint being
    copied. Every write holds `lock` from its copy-before-write to its pwrite, so a moment taken
    under the lock falls between two writes."""

    def __init__(self, path, block):
        self.path = os.path.abspath(path)
        self.fd = os.open(self.path, os.O_RDWR)
        self.size = os.fstat(self.fd).st_size
        self.block = block
        self.lock = threading.Lock()
        self.dirty = set()
        self.pending = set()
        self.ck_fd = None
        self.prev = None
        self.stats = {"requests": 0, "read": 0, "write": 0, "flush": 0, "trim": 0, "disc": 0, "other": 0,
                      "bytes_read": 0, "bytes_written": 0, "bytes_trimmed": 0, "fua": 0, "connections": 0,
                      "max_write_ms": 0.0, "max_flush_ms": 0.0, "cow_copies": 0, "errors": [], "checkpoints": []}

    def error(self, what):
        e = {"t": round(time.time(), 3), "error": what}
        self.stats["errors"] = (self.stats["errors"] + [e])[-50:]
        emit(event="error", error=what)

    def copy_block(self, b):
        off = b * self.block
        n = min(self.block, self.size - off)
        os.pwrite(self.ck_fd, os.pread(self.fd, n, off), off)

    def write(self, off, data):
        first, last = off // self.block, (off + len(data) - 1) // self.block
        with self.lock:
            for b in range(first, last + 1):
                if b in self.pending and MUTANT != "no-cow":
                    self.copy_block(b)
                    self.pending.discard(b)
                    self.stats["cow_copies"] += 1
                self.dirty.add(b)
            os.pwrite(self.fd, data, off)

    def checkpoint(self, dst):
        """Returns the checkpoint's measures. dst must not exist; it appears whole, by a rename."""
        dst = os.path.abspath(dst)
        tmp = dst + ".tmp"
        if os.path.exists(tmp):
            os.unlink(tmp)
        r = {"path": dst}
        t0 = time.time()
        if self.prev is None:
            # The first: a clone of the live image, no write in flight while it runs.
            w0 = time.time()
            with self.lock:
                got = time.time()
                r["how"] = clone_file(self.path, tmp)
                self.dirty = set()
                r["pause_ms"] = round((time.time() - got) * 1000, 3)
            r["lock_wait_ms"] = round((got - w0) * 1000, 3)
            r["kind"] = "clone-of-live"
            r["copied_blocks"] = 0
        else:
            r["kind"] = "copy-before-write"
            r["how"] = clone_file(self.prev, tmp)
            r["clone_prev_s"] = round(time.time() - t0, 4)
            fd = os.open(tmp, os.O_RDWR)
            w0 = time.time()
            with self.lock:
                got = time.time()
                self.ck_fd = fd
                self.pending, self.dirty = self.dirty, set()
                r["pause_ms"] = round((time.time() - got) * 1000, 3)
            r["lock_wait_ms"] = round((got - w0) * 1000, 3)
            cow0 = self.stats["cow_copies"]
            copied = 0
            for b in sorted(self.pending):
                if COPY_DELAY:
                    time.sleep(COPY_DELAY)
                with self.lock:
                    if b in self.pending:
                        self.copy_block(b)
                        self.pending.discard(b)
                        copied += 1
            with self.lock:
                self.ck_fd = None
            r["copied_blocks"] = copied
            r["cow_copies"] = self.stats["cow_copies"] - cow0
            full_sync(fd)
            os.close(fd)
        os.rename(tmp, dst)
        dfd = os.open(os.path.dirname(dst), os.O_RDONLY)
        try:
            os.fsync(dfd)
        finally:
            os.close(dfd)
        self.prev = dst
        r["op_s"] = round(time.time() - t0, 4)
        self.stats["checkpoints"].append(r)
        emit(event="checkpoint", **r)
        return r


def opt_reply(c, opt, typ, data=b""):
    c.sendall(struct.pack(">QIII", REPLY_MAGIC, opt, typ, len(data)) + data)


def handshake(c, disk):
    """Fixed newstyle. Returns True when transmission starts, False when the client left."""
    c.sendall(struct.pack(">QQH", NBDMAGIC, IHAVEOPT, FLAG_FIXED_NEWSTYLE | FLAG_NO_ZEROES))
    (cflags,) = struct.unpack(">I", recv_exact(c, 4))
    txflags = TX_HAS_FLAGS | TX_SEND_FLUSH | TX_SEND_FUA | TX_SEND_TRIM
    while True:
        magic, opt, ln = struct.unpack(">QII", recv_exact(c, 16))
        if magic != IHAVEOPT:
            raise ValueError("bad option magic %x" % magic)
        if ln > 65536:
            raise ValueError("option of %d bytes" % ln)
        data = recv_exact(c, ln) if ln else b""
        if opt == OPT_EXPORT_NAME:
            r = struct.pack(">QH", disk.size, txflags)
            c.sendall(r if cflags & FLAG_NO_ZEROES else r + b"\0" * 124)
            return True
        if opt in (OPT_INFO, OPT_GO):
            want = []
            if len(data) >= 6:
                (nl,) = struct.unpack(">I", data[:4])
                if len(data) >= 4 + nl + 2:
                    (nr,) = struct.unpack(">H", data[4 + nl:6 + nl])
                    want = [struct.unpack(">H", data[6 + nl + 2 * i:8 + nl + 2 * i])[0] for i in range(nr)
                            if 8 + nl + 2 * i <= len(data)]
            opt_reply(c, opt, REP_INFO, struct.pack(">HQH", INFO_EXPORT, disk.size, txflags))
            if INFO_BLOCK_SIZE in want:
                opt_reply(c, opt, REP_INFO, struct.pack(">HIII", INFO_BLOCK_SIZE, 1, 4096, MAX_REQ))
            opt_reply(c, opt, REP_ACK)
            if opt == OPT_GO:
                return True
        elif opt == OPT_LIST:
            opt_reply(c, opt, REP_SERVER, struct.pack(">I", 0))
            opt_reply(c, opt, REP_ACK)
        elif opt == OPT_ABORT:
            opt_reply(c, opt, REP_ACK)
            return False
        else:
            emit(event="option-refused", option=opt)
            opt_reply(c, opt, REP_ERR_UNSUP)


def serve_conn(c, disk):
    st = disk.stats
    if not handshake(c, disk):
        return
    while True:
        try:
            h = recv_exact(c, 28)
        except EOFError:
            return
        magic, flags, typ, handle, off, ln = struct.unpack(">IHHQQI", h)
        if magic != REQ_MAGIC:
            raise ValueError("bad request magic %x" % magic)
        st["requests"] += 1
        st[CMD_NAMES.get(typ, "other")] += 1
        bad = ln > MAX_REQ or off + ln > disk.size
        if typ == CMD_READ:
            if bad:
                disk.error("read out of range: %d+%d" % (off, ln))
                c.sendall(struct.pack(">IIQ", SIMPLE_REPLY, EINVAL, handle))
                continue
            try:
                data = os.pread(disk.fd, ln, off)
                if len(data) < ln:
                    data += b"\0" * (ln - len(data))
                err = 0
            except OSError as e:
                disk.error("pread %d+%d: %s" % (off, ln, e))
                data, err = b"", EIO
            c.sendall(struct.pack(">IIQ", SIMPLE_REPLY, err, handle) + data)
            st["bytes_read"] += ln
        elif typ == CMD_WRITE:
            data = recv_exact(c, ln) if ln <= MAX_REQ else None
            if bad or data is None:
                disk.error("write out of range: %d+%d" % (off, ln))
                c.sendall(struct.pack(">IIQ", SIMPLE_REPLY, EINVAL, handle))
                if data is None:
                    return
                continue
            t0 = time.time()
            err = 0
            try:
                disk.write(off, data)
                if flags & CMD_FLAG_FUA:
                    st["fua"] += 1
                    full_sync(disk.fd)
            except OSError as e:
                disk.error("pwrite %d+%d: %s" % (off, ln, e))
                err = EIO
            ms = (time.time() - t0) * 1000
            if ms > st["max_write_ms"]:
                st["max_write_ms"] = round(ms, 3)
            st["bytes_written"] += ln
            c.sendall(struct.pack(">IIQ", SIMPLE_REPLY, err, handle))
        elif typ == CMD_FLUSH:
            t0 = time.time()
            err = 0
            try:
                full_sync(disk.fd)
            except OSError as e:
                disk.error("flush: %s" % e)
                err = EIO
            ms = (time.time() - t0) * 1000
            if ms > st["max_flush_ms"]:
                st["max_flush_ms"] = round(ms, 3)
            c.sendall(struct.pack(">IIQ", SIMPLE_REPLY, err, handle))
        elif typ == CMD_TRIM:
            st["bytes_trimmed"] += ln
            c.sendall(struct.pack(">IIQ", SIMPLE_REPLY, 0, handle))
        elif typ == CMD_DISC:
            return
        else:
            disk.error("command %d refused" % typ)
            c.sendall(struct.pack(">IIQ", SIMPLE_REPLY, EINVAL, handle))


def listen_on(args):
    if args.get("unix"):
        p = args["unix"]
        if os.path.exists(p):
            os.unlink(p)
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.bind(p)
    else:
        host, _, port = args["tcp"].rpartition(":")
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((host or "127.0.0.1", int(port)))
    s.listen(4)
    return s


def control_loop(path, disk, stop):
    if os.path.exists(path):
        os.unlink(path)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(path)
    s.listen(4)
    while not stop.is_set():
        c, _ = s.accept()
        try:
            line = c.makefile("r").readline().strip().split()
            if not line:
                ans = {"ok": False, "error": "empty request"}
            elif line[0] == "stats":
                ans = dict(disk.stats, ok=True, size=disk.size)
            elif line[0] == "checkpoint" and len(line) == 2:
                try:
                    ans = dict(disk.checkpoint(line[1]), ok=True)
                except OSError as e:
                    disk.error("checkpoint: %s" % e)
                    ans = {"ok": False, "error": str(e)}
            elif line[0] == "quit":
                ans = {"ok": True}
                stop.set()
            else:
                ans = {"ok": False, "error": "unknown request %s" % line[0]}
            c.sendall((json.dumps(ans) + "\n").encode())
        finally:
            c.close()
    os._exit(0)


def cmd_serve(argv):
    a = {}
    i = 0
    while i < len(argv):
        k = argv[i].lstrip("-")
        a[k] = argv[i + 1]
        i += 2
    disk = Disk(a["image"], int(a.get("block", 65536)))
    lsock = listen_on(a)
    stop = threading.Event()
    if a.get("control"):
        threading.Thread(target=control_loop, args=(a["control"], disk, stop), daemon=True).start()
    emit(event="listening", image=disk.path, size=disk.size, unix=a.get("unix"), tcp=a.get("tcp"))
    # One client at a time, as Apple's client opens one connection (it reconnects after a timeout).
    while not stop.is_set():
        c, _ = lsock.accept()
        disk.stats["connections"] += 1
        emit(event="connected", n=disk.stats["connections"])
        try:
            serve_conn(c, disk)
        except (OSError, ValueError, EOFError, struct.error) as e:
            disk.error("connection: %s: %s" % (type(e).__name__, e))
        finally:
            c.close()
        emit(event="left", requests=disk.stats["requests"])
    return 0


def cmd_ctl(argv):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(argv[0])
    s.sendall((" ".join(argv[1:]) + "\n").encode())
    ans = s.makefile("r").readline()
    print(ans.strip())
    return 0 if json.loads(ans or "{}").get("ok") else 1


# ---------------------------------------------------------------- the client side


def connect(url):
    u = urlparse(url)
    if u.scheme == "nbd+unix":
        sock = parse_qs(u.query).get("socket", [""])[0]
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(sock)
    elif u.scheme == "nbd":
        s = socket.create_connection((u.hostname or "127.0.0.1", u.port or 10809))
    else:
        raise ValueError("not an NBD URL: %s" % url)
    name = u.path.lstrip("/").encode()
    magic, opt, gflags = struct.unpack(">QQH", recv_exact(s, 18))
    if magic != NBDMAGIC or opt != IHAVEOPT:
        raise ValueError("not a newstyle NBD server")
    s.sendall(struct.pack(">I", FLAG_FIXED_NEWSTYLE | (gflags & FLAG_NO_ZEROES)))
    data = struct.pack(">I", len(name)) + name + struct.pack(">HH", 1, INFO_BLOCK_SIZE)
    s.sendall(struct.pack(">QII", IHAVEOPT, OPT_GO, len(data)) + data)
    size = flags = None
    while True:
        m, o, typ, ln = struct.unpack(">QIII", recv_exact(s, 20))
        body = recv_exact(s, ln) if ln else b""
        if m != REPLY_MAGIC:
            raise ValueError("bad option reply")
        if typ == REP_INFO and struct.unpack(">H", body[:2])[0] == INFO_EXPORT:
            _, size, flags = struct.unpack(">HQH", body[:12])
        elif typ == REP_ACK:
            break
        elif typ & 0x80000000:
            raise ValueError("option GO refused: %x" % typ)
    return s, size, flags


class Client:
    def __init__(self, url):
        self.s, self.size, self.flags = connect(url)
        self.handle = 0

    def req(self, typ, off, ln, data=b"", flags=0):
        self.handle += 1
        self.s.sendall(struct.pack(">IHHQQI", REQ_MAGIC, flags, typ, self.handle, off, ln) + data)
        magic, err, handle = struct.unpack(">IIQ", recv_exact(self.s, 16))
        if magic != SIMPLE_REPLY or handle != self.handle:
            raise ValueError("bad reply")
        body = recv_exact(self.s, ln) if typ == CMD_READ and err == 0 else b""
        return err, body

    def close(self):
        try:
            self.s.sendall(struct.pack(">IHHQQI", REQ_MAGIC, 0, CMD_DISC, 0, 0, 0))
        finally:
            self.s.close()


def cmd_probe(argv):
    t0 = time.time()
    try:
        c = Client(argv[0])
        err, b = c.req(CMD_READ, 0, 4096)
        c.close()
    except (OSError, ValueError, EOFError, struct.error) as e:
        print(json.dumps({"ok": False, "error": "%s: %s" % (type(e).__name__, e)}))
        return 1
    print(json.dumps({"ok": err == 0 and len(b) == 4096, "size": c.size, "flags": c.flags,
                      "trim": bool(c.flags & TX_SEND_TRIM), "flush": bool(c.flags & TX_SEND_FLUSH),
                      "s": round(time.time() - t0, 4)}))
    return 0 if err == 0 else 1


def cmd_writer(argv):
    url, seconds, logp = argv[0], float(argv[1]), argv[2]
    rate = float(argv[3]) if len(argv) > 3 else 200.0
    c = Client(url)
    blocks = c.size // 4096
    rnd = random.Random(7)
    lat = []
    seq = 0
    end = time.time() + seconds
    with open(logp, "w") as log:
        while time.time() < end:
            b = rnd.randrange(blocks)
            data = struct.pack("<QQQ", WMAGIC, seq, b).ljust(4096, b"w")
            t0 = time.time()
            err, _ = c.req(CMD_WRITE, b * 4096, 4096, data)
            lat.append(time.time() - t0)
            if err:
                print(json.dumps({"ok": False, "error": "write %d refused: %d" % (seq, err)}))
                return 1
            log.write("%d %d\n" % (seq, b))
            log.flush()
            seq += 1
            time.sleep(max(0.0, 1.0 / rate - (time.time() - t0)))
    c.req(CMD_FLUSH, 0, 0)
    c.close()
    lat.sort()
    print(json.dumps({"ok": True, "writes": seq, "p50_ms": round(lat[len(lat) // 2] * 1000, 3),
                      "max_ms": round(lat[-1] * 1000, 3)}))
    return 0


def cmd_verify(argv):
    """The image must hold, for some moment m, the last write before m of every block written
    before m, and nothing written at or after m."""
    img, logp = argv[0], argv[1]
    writes = [tuple(int(x) for x in l.split()) for l in open(logp) if l.strip()]
    seen = {}
    with open(img, "rb") as f:
        for seq, b in writes:
            if b in seen:
                continue
            f.seek(b * 4096)
            h = f.read(24)
            mg, q, bb = struct.unpack("<QQQ", h) if len(h) == 24 else (0, 0, 0)
            seen[b] = (q, bb) if mg == WMAGIC and bb == b else (None, None)
    present = [q for q, _ in seen.values() if q is not None]
    m = max(present) + 1 if present else 0
    last_before = {}
    for seq, b in writes:
        if seq < m:
            last_before[b] = seq
    wrong = 0
    for b, (q, bb) in seen.items():
        want = last_before.get(b)
        if want != q:
            wrong += 1
    print(json.dumps({"ok": wrong == 0, "moment": m, "blocks_checked": len(seen), "wrong": wrong,
                      "writes_logged": len(writes)}))
    return 0 if wrong == 0 else 1


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    cmd, rest = sys.argv[1], sys.argv[2:]
    f = {"serve": cmd_serve, "ctl": cmd_ctl, "probe": cmd_probe, "writer": cmd_writer, "verify": cmd_verify}.get(cmd)
    if f is None:
        print(__doc__)
        return 2
    return f(rest)


if __name__ == "__main__":
    sys.exit(main())
