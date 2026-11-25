"""A store-and-forward relay that neither side trusts: a stand-in for ntfy, a push service, or a
relay Amont Labs would run. Both sides only make outbound requests to it, so the Mac needs no
inbound port.

    POST /t/<topic>                     body: opaque bytes        -> {"seq": n}
    GET  /t/<topic>?since=N&wait=S      long poll                 -> [{"seq": n, "t": ..., "b": base64}]

For the tamper tests the relay can turn hostile, through POST /_evil with a JSON body:
    {"mode": "replay", "topic": T, "seq": n}   send message n of topic T again, as new
    {"mode": "inject", "topic": T, "b": b64}   post bytes of its own making
    {"mode": "flip", "topic": T}               flip one bit in every later message of T
    {"mode": "delay", "topic": T, "s": 30}     hold every later message of T for s seconds
    {"mode": "drop", "topic": T}               drop every later message of T
    {"mode": "honest"}
GET /_seen returns what the relay could log: topic, size and time of each message.
"""
import base64, json, os, sys, threading, time, http.server
from urllib.parse import urlparse, parse_qs

lock = threading.Condition()
topics = {}     # topic -> list of {"seq", "t", "b"}
seen = []
evil = {}       # topic -> mode dict


def post(topic, data, t=None):
    with lock:
        lst = topics.setdefault(topic, [])
        m = {"seq": len(lst) + 1, "t": t or time.time(), "b": base64.b64encode(data).decode()}
        lst.append(m)
        lock.notify_all()
        return m["seq"]


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def reply(self, obj, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code); self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def do_POST(self):
        data = self.rfile.read(int(self.headers.get("content-length", 0)))
        u = urlparse(self.path)
        if u.path == "/_evil":
            e = json.loads(data)
            with lock:
                if e["mode"] == "honest":
                    evil.clear()
                elif e["mode"] == "replay":
                    old = topics[e["topic"]][e["seq"] - 1]
                    lst = topics[e["topic"]]
                    lst.append({"seq": len(lst) + 1, "t": time.time(), "b": old["b"]}); lock.notify_all()
                elif e["mode"] == "inject":
                    lst = topics.setdefault(e["topic"], [])
                    lst.append({"seq": len(lst) + 1, "t": time.time(), "b": e["b"]}); lock.notify_all()
                else:
                    evil[e["topic"]] = e
            return self.reply({"ok": True})
        topic = u.path.split("/")[-1]
        seen.append({"topic": topic, "size": len(data), "t": time.time()})
        e = evil.get(topic)
        if e and e["mode"] == "drop":
            return self.reply({"seq": 0})
        if e and e["mode"] == "flip":
            data = bytearray(data); data[len(data) // 2] ^= 1; data = bytes(data)
        if e and e["mode"] == "delay":
            threading.Timer(e["s"], post, (topic, data)).start()
            return self.reply({"seq": -1})
        self.reply({"seq": post(topic, data)})

    def do_GET(self):
        u = urlparse(self.path)
        if u.path == "/_seen":
            return self.reply(seen)
        q = parse_qs(u.query)
        topic = u.path.split("/")[-1]
        since = int(q.get("since", ["0"])[0]); wait = float(q.get("wait", ["25"])[0])
        end = time.time() + wait
        with lock:
            while True:
                out = [m for m in topics.get(topic, []) if m["seq"] > since]
                left = end - time.time()
                if out or left <= 0:
                    break
                lock.wait(left)
        self.reply(out)


class S(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    S(("127.0.0.1", int(os.environ.get("PORT", "8810"))), H).serve_forever()
