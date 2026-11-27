"""A relay that passes a TLS connection on unread, as the boundary does for a computer whose Google
sign-in traffic passes unread (`google-sign-in-unread`). It sees what such a boundary sees: when
each connection opens and closes, and the encrypted bytes each way. It writes one line per
connection to <OUT>/relay.jsonl with its bytes and its "turns": how many times the client started
sending again after the server had sent, the best guess at a request count from the outside.

Run: OUT=out LISTEN=9000 TARGET=8500 python3 relay.py
"""
import json, os, socket, threading, time

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
LOG = open(f"{OUT}/relay.jsonl", "a"); LOCK = threading.Lock()
TARGET = int(os.environ["TARGET"])


def handle(c, n):
    s = socket.create_connection(("127.0.0.1", TARGET))
    st = {"conn": n, "opened": time.time(), "up": 0, "down": 0, "turns": 0, "last": None}
    done = threading.Event()

    def pump(src, dst, way):
        try:
            while True:
                b = src.recv(65536)
                if not b:
                    break
                with LOCK:
                    st[way] += len(b)
                    if way == "up" and st["last"] != "up":
                        st["turns"] += 1
                    st["last"] = way
                dst.sendall(b)
        except OSError:
            pass
        finally:
            for x in (src, dst):
                try:
                    x.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
            done.set()

    t1 = threading.Thread(target=pump, args=(c, s, "up")); t2 = threading.Thread(target=pump, args=(s, c, "down"))
    t1.start(); t2.start(); t1.join(); t2.join()
    st["closed"] = time.time(); st.pop("last")
    with LOCK:
        LOG.write(json.dumps(st) + "\n"); LOG.flush()


srv = socket.socket(); srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", int(os.environ["LISTEN"]))); srv.listen()
n = 0
while True:
    c, _ = srv.accept(); n += 1
    threading.Thread(target=handle, args=(c, n), daemon=True).start()
