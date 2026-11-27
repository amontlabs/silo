"""Frames shorter than an Ethernet header, sent to gvproxy on its computer's datagram socket.

For each frame: start gvproxy with -listen-vfkit, say VFKT once, check that gvproxy answers an ARP
for its gateway, send the frame on the same socket a computer's frames come from, then check again,
and see whether gvproxy still stops on SIGTERM. Exits 1 if any frame cost the computer its link,
ended gvproxy, or kept it from stopping.

    python3 short-frames.py <gvproxy binary>
"""
import os, shutil, socket, subprocess, sys, tempfile, time

GV = sys.argv[1]
GATEWAY_MAC = bytes.fromhex("5a94efe40cdd")  # gvproxy's default
ME = bytes.fromhex("5a94ef000002")
FRAMES = [
    ("a second VFKT", b"VFKT"),
    ("5 zero bytes", b"\0" * 5),
    ("6 bytes, to everyone", b"\xff" * 6),
    ("12 bytes, to everyone", b"\xff" * 12),
    ("13 bytes, to the gateway", GATEWAY_MAC + ME + b"\x08"),
    ("6 zero bytes", b"\0" * 6),
    ("14 bytes, a header alone", GATEWAY_MAC + ME + b"\x08\x00"),
]


def arp(s):
    """Asks gvproxy who has its gateway's address; True if it answers within a second."""
    s.send(b"\xff" * 6 + ME + b"\x08\x06" + bytes.fromhex("0001080006040001") + ME + bytes([192, 168, 127, 2])
           + b"\0" * 6 + bytes([192, 168, 127, 1]))
    end = time.time() + 1
    while time.time() < end:
        try:
            r = s.recv(65536)
        except socket.timeout:
            return False
        if r[12:14] == b"\x08\x06" and r[20:22] == b"\x00\x02":
            return True
    return False


def one(frame):
    d = tempfile.mkdtemp()
    try:
        g = subprocess.Popen([GV, "-listen-vfkit", f"unixgram://{d}/gv.sock", "-log-file", f"{d}/gv.log"],
                             stdout=subprocess.DEVNULL, stderr=open(f"{d}/gv.err", "w"))
        while not os.path.exists(f"{d}/gv.sock"):
            time.sleep(0.02)
        s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        s.bind(f"{d}/me.sock")
        s.connect(f"{d}/gv.sock")
        s.settimeout(1)
        s.send(b"VFKT")
        time.sleep(0.2)
        before = arp(s)
        s.send(frame)
        time.sleep(0.3)
        running = g.poll() is None
        after = arp(s) if running else False
        stops = None
        if running:
            g.terminate()
            try:
                g.wait(3)
                stops = True
            except subprocess.TimeoutExpired:
                stops = False
                g.kill()
                g.wait()
        log = open(f"{d}/gv.log").read() + open(f"{d}/gv.err").read()
        panic = next((l for l in log.splitlines() if l.startswith("panic:")), "")
        return before, running, after, stops, panic, "dropped a frame" in log
    finally:
        shutil.rmtree(d)


bad = 0
print(f"{'frame':28} {'link before':12} {'gvproxy':10} {'link after':11} {'SIGTERM':9} log")
for name, frame in FRAMES:
    before, running, after, stops, panic, dropped = one(frame)
    ok = before and running and after and stops
    bad += not ok
    print(f"{name:28} {'yes' if before else 'no':12} {'running' if running else 'ENDED':10} {'yes' if after else 'LOST':11} "
          f"{'-' if stops is None else 'stops' if stops else 'HANGS':9} {panic or ('dropped, counted' if dropped else '')}")
print("PASS" if not bad else f"FAIL: {bad} of {len(FRAMES)} frames cost the link, the process, or its stop")
sys.exit(1 if bad else 0)
