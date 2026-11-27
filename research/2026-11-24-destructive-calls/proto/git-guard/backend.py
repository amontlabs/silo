"""The 'server': plain HTTP in front of git http-backend (CGI), standing in for github.com.
Logs the sha256 of every request body it receives, so a run can prove the guard forwarded
the push byte for byte."""
import argparse, hashlib, json, os, subprocess, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def read_body(h):
    if h.headers.get("Transfer-Encoding", "").lower() == "chunked":
        out = b""
        while True:
            size = int(h.rfile.readline().split(b";")[0], 16)
            if size == 0:
                h.rfile.readline()
                return out
            out += h.rfile.read(size)
            h.rfile.readline()
    return h.rfile.read(int(h.headers.get("Content-Length", 0)))


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def run(self):
        body = read_body(self) if self.command == "POST" else b""
        path, _, qs = self.path.partition("?")
        env = dict(os.environ, GIT_PROJECT_ROOT=ARGS.root, GIT_HTTP_EXPORT_ALL="1",
                   REQUEST_METHOD=self.command, PATH_INFO=path, QUERY_STRING=qs,
                   CONTENT_TYPE=self.headers.get("Content-Type", ""),
                   CONTENT_LENGTH=str(len(body)), REMOTE_ADDR="127.0.0.1",
                   REMOTE_USER="agent", GIT_PROTOCOL=self.headers.get("Git-Protocol", ""))
        if self.headers.get("Content-Encoding"):
            env["HTTP_CONTENT_ENCODING"] = self.headers["Content-Encoding"]
        out = subprocess.run(["git", "http-backend"], input=body, env=env,
                             capture_output=True).stdout
        head, _, payload = out.partition(b"\r\n\r\n")
        status = 200
        hdrs = []
        for line in head.split(b"\r\n"):
            k, _, v = line.decode().partition(":")
            if k.lower() == "status":
                status = int(v.strip().split()[0])
            elif k:
                hdrs.append((k, v.strip()))
        with open(ARGS.log, "a") as f:
            f.write(json.dumps({"t": time.time(), "method": self.command, "path": self.path,
                                "body_len": len(body),
                                "body_sha256": hashlib.sha256(body).hexdigest()}) + "\n")
        self.send_response(status)
        for k, v in hdrs:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    do_GET = do_POST = run


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=9000)
    ap.add_argument("--root", required=True)
    ap.add_argument("--log", required=True)
    ARGS = ap.parse_args()
    ThreadingHTTPServer(("127.0.0.1", ARGS.port), H).serve_forever()
