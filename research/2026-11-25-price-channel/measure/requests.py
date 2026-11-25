"""What a request for the index sends, by client: curl, Python's urllib, Node's fetch, and the
channel's Fetcher. Each asks a local server for /index.json; the server prints the request head.
A header that differs between Macs (a version, an OS build, a cookie, an ETag) is a way to tell
installs apart. macOS's URLSession is not measured here: no Mac in this container.

usage: requests.py > results/requests.txt"""
import http.server, subprocess, sys, threading, urllib.request
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "channel"))
import client

seen = []
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        seen.append(self.requestline + "\n" + "".join(f"{k}: {v}\n" for k, v in self.headers.items()))
        self.send_response(200); self.send_header("Content-Length", "2"); self.end_headers(); self.wfile.write(b"{}")
    def log_message(self, *a): pass

srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
threading.Thread(target=srv.serve_forever, daemon=True).start()
url = f"http://127.0.0.1:{srv.server_address[1]}/index.json"
clients = {
    "curl": lambda: subprocess.run(["curl", "-s", "-o", "/dev/null", url], check=True),
    "python urllib, default": lambda: urllib.request.urlopen(url).read(),
    "node fetch, default": lambda: subprocess.run(["node", "-e", f"fetch('{url}').then(r=>r.text())"], check=True),
    "channel Fetcher": lambda: client.Fetcher([url.rsplit("/", 1)[0]]).get("index.json"),
}
for name, run in clients.items():
    seen.clear(); run()
    print(f"== {name}\n{seen[0].replace(url.split('/')[2], '<host>')}")
srv.shutdown()
