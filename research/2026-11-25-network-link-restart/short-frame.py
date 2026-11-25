"""A datagram shorter than an Ethernet header, sent to gvproxy once it has its computer, ends
gvproxy's reading of that computer for good, with nothing in its log and the process still up.
A second "VFKT" is such a datagram. Run as root on Linux (link.py's requirements):

    python3 short-frame.py
"""
import http.server
import importlib.util
import os
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("link", os.path.join(HERE, "link.py"))
link = importlib.util.module_from_spec(spec)
spec.loader.exec_module(link)
GVPROXY = os.environ.get("GVPROXY", "/tmp/silo-kit-35-home/bin/gvproxy-v0.9.0")
host = subprocess.run(["hostname", "-I"], capture_output=True, text=True).stdout.split()[0]


class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()

    def log_message(self, *a):
        pass


srv = http.server.ThreadingHTTPServer((host, 0), H)
threading.Thread(target=srv.serve_forever, daemon=True).start()
bad = 0
for extra in (b"VFKT", b"ABCD", b"\0" * 6, b"\0" * 11, b"\0" * 12, b"\0" * 13, b"A" * 20):
    L = link.Link("repoint", "/tmp/lk-short", GVPROXY, GVPROXY)
    try:
        L.start()
        get = lambda: subprocess.run(["nsenter", "-t", str(L.guest_pid()), "-n", "curl", "-s", "-m", "3", "-o", "/dev/null",
                                      "-w", "%{http_code}", "http://%s:%d/" % (host, srv.server_address[1])],
                                     capture_output=True, text=True).stdout.strip()
        before = get()
        L.keep.send(extra)  # the helper's copy of the card's socket: the same path a frame takes
        time.sleep(0.3)
        after = get()
        alive = L.gv.poll() is None
        print("%-24r %2d bytes  before %s  after %s  gvproxy running: %s" % (extra, len(extra), before, after or "000", alive))
        bad += after != "200" and len(extra) >= 12
    finally:
        L.stop()
sys.exit(1 if bad else 0)
