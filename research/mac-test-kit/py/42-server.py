"""For test 42: an HTTP server that sends and takes bulk bytes, to measure a computer's network.

    python3 -I py/42-server.py --port-file FILE [--bind 0.0.0.0]

GET /bytes/N sends N bytes; POST /sink reads the body and answers its length. Nothing is kept.
Writes the port it listens on to FILE. Runs until killed.
"""
import argparse
import http.server
import os
import socketserver

CHUNK = b"\0" * (1 << 20)


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_GET(self):
        if not self.path.startswith("/bytes/"):
            self.send_error(404)
            return
        n = int(self.path[7:])
        self.send_response(200)
        self.send_header("Content-Length", str(n))
        self.end_headers()
        while n > 0:
            k = min(n, len(CHUNK))
            self.wfile.write(CHUNK[:k])
            n -= k

    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        got = 0
        while got < n:
            b = self.rfile.read(min(1 << 20, n - got))
            if not b:
                break
            got += len(b)
        body = str(got).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port-file", required=True)
    ap.add_argument("--bind", default="0.0.0.0")
    a = ap.parse_args()
    s = S((a.bind, 0), H)
    with open(a.port_file + ".tmp", "w") as f:
        f.write("%d\n" % s.server_address[1])
    os.replace(a.port_file + ".tmp", a.port_file)
    s.serve_forever()


if __name__ == "__main__":
    main()
