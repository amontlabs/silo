"""One guest under QEMU, paused and resumed the way a Mac's sleep pauses main.

Why QMP stop/cont is the model: on a Mac, Virtualization.framework's guest
counter runs on mach_absolute_time, which stops while the Mac sleeps (QEMU's
HVF backend has the same issue, qemu#864). The guest's clocks stop with it, and
the Mac's wall clock does not. QMP `stop` freezes QEMU's virtual clock in the
same way; the host's clock goes on. TCG is used because cloud containers have
no KVM; the clock behaviour under `stop` is QEMU's, not the accelerator's.
"""
import json, os, socket, subprocess, tempfile, time

BUILD = os.environ.get("BUILD", os.path.join(os.path.dirname(__file__), "..", ".build"))


class Guest:
    def __init__(self, rtc_clock="host"):
        self.dir = tempfile.mkdtemp(prefix="clockvm-")
        self.qmp_path = os.path.join(self.dir, "qmp")
        self.agent_path = os.path.join(self.dir, "agent")
        self.console = os.path.join(self.dir, "console.log")
        self.proc = subprocess.Popen([
            "qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-m", "256",
            "-nographic", "-no-reboot", "-nodefaults",
            "-kernel", os.path.join(BUILD, "vmlinuz"),
            "-initrd", os.path.join(BUILD, "initrd.gz"),
            "-append", "console=ttyS0 quiet panic=-1",
            "-rtc", f"base=utc,clock={rtc_clock}",
            "-serial", f"file:{self.console}",
            "-chardev", f"socket,id=agent,path={self.agent_path},server=on,wait=off",
            "-serial", "chardev:agent",
            "-qmp", f"unix:{self.qmp_path},server=on,wait=off",
        ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self._wait_file(self.qmp_path)
        self.qmp = socket.socket(socket.AF_UNIX)
        self.qmp.connect(self.qmp_path)
        self.qf = self.qmp.makefile("rw")
        self.qf.readline()
        self._qmp("qmp_capabilities")
        deadline = time.time() + 180
        while "guest-ready" not in open(self.console, errors="replace").read():
            if time.time() > deadline:
                raise RuntimeError("guest did not boot")
            time.sleep(0.5)
        self.ag = socket.socket(socket.AF_UNIX)
        self.ag.connect(self.agent_path)
        self.af = self.ag.makefile("rw")
        self.clocksource = open(self.console, errors="replace").read().split("clocksource: ")[1].split()[0]

    def _wait_file(self, p):
        for _ in range(200):
            if os.path.exists(p):
                return
            time.sleep(0.05)
        raise RuntimeError(p)

    def _qmp(self, cmd):
        self.qf.write(json.dumps({"execute": cmd}) + "\n"); self.qf.flush()
        while True:
            r = json.loads(self.qf.readline())
            if "return" in r or "error" in r:
                return r

    def _ask(self, line):
        self.af.write(line + "\n"); self.af.flush()
        return self.af.readline().split()

    def offset(self):
        """Guest wall clock minus host wall clock, in seconds, and the round trip."""
        t0 = time.time_ns()
        r = self._ask("T")
        t1 = time.time_ns()
        return (int(r[1]) - (t0 + t1) // 2) / 1e9, (t1 - t0) / 1e9

    def set_clock(self):
        """Step the guest's wall clock to the host's, allowing for half the round trip."""
        _, rtt = self.offset()
        self._ask(f"S {time.time_ns() + int(rtt * 1e9 / 2)}")
        return self.offset()

    def rtc_offset(self):
        return int(self._ask("R")[1]) - time.time()

    def sleep(self, seconds):
        """The Mac sleeps: main's clocks stop, the Mac's wall clock goes on."""
        self._qmp("stop")
        time.sleep(seconds)
        self._qmp("cont")

    def close(self):
        self.proc.kill(); self.proc.wait()
