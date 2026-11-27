"""A file server with HTTP Range support, so test 00's resumable `curl -C -` runs on Linux.
python3 linux/range-server.py FILE PORTFILE [CUT_AT]: serves FILE at /restore.ipsw on 127.0.0.1,
writes the port it took into PORTFILE. With CUT_AT, the first full GET stops after CUT_AT bytes,
as a dropped connection would."""
import http.server
import os
import sys

PATH, PORTFILE = sys.argv[1], sys.argv[2]
CUT = int(sys.argv[3]) if len(sys.argv) > 3 else 0
state = {"cut_done": False}


class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def head_or_get(self, body):
        size = os.path.getsize(PATH)
        start, end = 0, size - 1
        rng = self.headers.get("Range")
        if rng and rng.startswith("bytes="):
            a, _, b = rng[6:].partition("-")
            start = int(a or 0)
            end = int(b) if b else size - 1
            if start >= size:
                self.send_response(416)
                self.send_header("Content-Range", "bytes */%d" % size)
                self.end_headers()
                return
            self.send_response(206)
            self.send_header("Content-Range", "bytes %d-%d/%d" % (start, end, size))
        else:
            self.send_response(200)
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        if not body:
            return
        with open(PATH, "rb") as f:
            f.seek(start)
            left = end - start + 1
            if CUT and not state["cut_done"] and not rng:
                state["cut_done"] = True
                left = min(left, CUT)
                self.wfile.write(f.read(left))
                self.wfile.flush()
                self.connection.shutdown(2)
                return
            while left > 0:
                chunk = f.read(min(left, 1 << 16))
                self.wfile.write(chunk)
                left -= len(chunk)

    def do_HEAD(self):
        self.head_or_get(False)

    def do_GET(self):
        self.head_or_get(True)


srv = http.server.HTTPServer(("127.0.0.1", 0), H)
with open(PORTFILE, "w") as f:
    f.write(str(srv.server_address[1]))
srv.serve_forever()
