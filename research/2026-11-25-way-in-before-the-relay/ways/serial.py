"""The console as the way in: the computer has no network card, and a shell runs on its serial
console (here a pty; a Linux main gets one from `console=hvc0` and an autologin getty). The Mac
types an installer into it: the relay, sent as base64 in lines, then checked by its hash.
usage: serial.py <work dir>       Prints the way-in lines of ways.sh."""
import base64, hashlib, os, pty, select, sys, threading, time, tty
t0 = time.time()
pid, fd = pty.fork()
if pid == 0:
    os.execvp("ip", ["ip", "netns", "exec", "vm", "bash", "--norc", "--noprofile"])
tty.setraw(fd)
buf = bytearray(); lock = threading.Lock()
def reader():
    while True:
        try: d = os.read(fd, 65536)
        except OSError: return
        if not d: return
        with lock: buf.extend(d)
threading.Thread(target=reader, daemon=True).start()
def wait_for(token, timeout):
    end = time.time() + timeout
    while time.time() < end:
        with lock:
            if token in buf: return bytes(buf)
        time.sleep(0.05)
    with lock: return bytes(buf)
os.write(fd, b"stty -echo raw; echo READY\n"); wait_for(b"READY", 20)
print("way-in", "yes"); print("way-in-wait-s", round(time.time() - t0, 1))
blob = os.urandom(2_000_000); want = hashlib.sha256(blob).hexdigest()[:16]
b64 = base64.b64encode(blob)
t1 = time.time()
lines = b"".join(b64[i:i + 76] + b"\n" for i in range(0, len(b64), 76))
os.write(fd, b"base64 -d > /tmp/relay.blob <<'EOF'\n")
for i in range(0, len(lines), 4096):
    os.write(fd, lines[i:i + 4096])
os.write(fd, b"EOF\necho SUM $(sha256sum < /tmp/relay.blob | cut -c1-16) END\n")
out = wait_for(b" END", 120)
got = out.split(b"SUM ")[-1].split()[0].decode() if b"SUM " in out else ""
print("copy-2MB", "intact" if got == want else "broken", f"{round(time.time() - t1, 2)}s")
os.write(fd, b"exit\n"); time.sleep(0.2)
