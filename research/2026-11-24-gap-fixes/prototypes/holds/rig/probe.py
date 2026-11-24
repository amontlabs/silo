"""A stand-in for Silo's probe: while armed, it makes every exec, and every open of a path
outside an allowlist, by the agent program's process tree fail with EPERM (fanotify permission
events). Linux only, needs CAP_SYS_ADMIN."""
import ctypes, os, struct, threading, time

FAN_CLASS_CONTENT = 0x4
FAN_CLOEXEC = 0x1
FAN_UNLIMITED_QUEUE = 0x10
FAN_MARK_ADD = 0x1
FAN_MARK_FILESYSTEM = 0x100
FAN_OPEN_PERM = 0x10000
FAN_OPEN_EXEC_PERM = 0x40000
FAN_ALLOW, FAN_DENY = 1, 2
AT_FDCWD = -100

libc = ctypes.CDLL(None, use_errno=True)
libc.fanotify_mark.argtypes = [ctypes.c_int, ctypes.c_uint, ctypes.c_uint64, ctypes.c_int, ctypes.c_char_p]


class Probe:
    def __init__(self, allow_prefixes, log):
        self.fd = libc.fanotify_init(FAN_CLASS_CONTENT | FAN_CLOEXEC | FAN_UNLIMITED_QUEUE, os.O_RDONLY | os.O_LARGEFILE)
        if self.fd < 0:
            raise OSError(ctypes.get_errno(), "fanotify_init")
        r = libc.fanotify_mark(self.fd, FAN_MARK_ADD | FAN_MARK_FILESYSTEM, FAN_OPEN_PERM | FAN_OPEN_EXEC_PERM, AT_FDCWD, b"/")
        if r < 0:
            raise OSError(ctypes.get_errno(), "fanotify_mark")
        self.root_pid = None
        self.armed = False
        self.allow = allow_prefixes
        self.log = log
        self.denied = []
        self.me = os.getpid()
        threading.Thread(target=self._loop, daemon=True).start()

    def arm(self, why):
        self.armed = True
        self.log(f"probe armed: {why}")

    def disarm(self, why):
        self.armed = False
        self.log(f"probe disarmed: {why}")

    def _in_tree(self, pid):
        if self.root_pid is None:
            return False
        for _ in range(64):
            if pid == self.root_pid:
                return True
            if pid <= 1:
                return False
            try:
                with open(f"/proc/{pid}/stat") as f:
                    s = f.read()
                pid = int(s[s.rindex(")") + 2:].split()[1])
            except Exception:
                return False
        return False

    def _loop(self):
        hdr = struct.Struct("=IBBHQii")
        while True:
            buf = os.read(self.fd, 65536)
            off = 0
            while off < len(buf):
                ev_len, ver, _, mlen, mask, fd, pid = hdr.unpack_from(buf, off)
                off += ev_len
                verdict = FAN_ALLOW
                try:
                    if self.armed and pid != self.me and self._in_tree(pid):
                        path = os.readlink(f"/proc/self/fd/{fd}")
                        exe = bool(mask & FAN_OPEN_EXEC_PERM)
                        if exe or not any(path.startswith(p) for p in self.allow):
                            verdict = FAN_DENY
                            try:
                                comm = open(f"/proc/{pid}/comm").read().strip()
                            except Exception:
                                comm = "?"
                            ent = f"DENY {'exec' if exe else 'open'} {path} by pid {pid} ({comm})"
                            self.denied.append(ent)
                            self.log(ent)
                except Exception as e:
                    self.log(f"probe error {e}")
                os.write(self.fd, struct.pack("=iI", fd, verdict))
                os.close(fd)


class FifoProbe:
    """Drives sc/sprobe (seccomp user notification) through its FIFO."""
    def __init__(self, fifo, allow_prefixes, log):
        self.fifo = fifo; self.log = log; self.armed = False; self.root_pid = None
        self.f = open(fifo, "w", buffering=1); self.allow = allow_prefixes

    def arm(self, why):
        self.f.write("reset\n" + "".join(f"allow {p}\n" for p in self.allow) + "arm\n"); self.f.flush(); self.armed = True; time.sleep(0.05); self.log(f"probe armed: {why}")

    def disarm(self, why):
        self.f.write("disarm\n"); self.f.flush(); self.armed = False; self.log(f"probe disarmed: {why}")
