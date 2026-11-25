"""Python's h11, a strict HTTP/1.1 parser: what it parsed, as the reply's body."""
import hashlib, json, socket, threading, h11
def serve(c):
    conn = h11.Connection(h11.SERVER)
    try:
        while True:
            ev = conn.next_event()
            if ev is h11.NEED_DATA:
                d = c.recv(65536)
                conn.receive_data(d)
                if not d and conn.their_state is not h11.SEND_BODY: pass
                continue
            if isinstance(ev, h11.Request):
                req = ev; body = b""
            elif isinstance(ev, h11.Data):
                body += ev.data
            elif isinstance(ev, h11.EndOfMessage):
                host = dict((k.decode(), v.decode()) for k, v in req.headers).get("host")
                o = json.dumps({"parsed": {"method": req.method.decode(), "target": req.target.decode(), "host": host, "len": len(body), "sha": hashlib.sha256(body).hexdigest()[:12]}}).encode()
                c.sendall(conn.send(h11.Response(status_code=200, headers=[("content-length", str(len(o)))])) + conn.send(h11.Data(data=o)) + conn.send(h11.EndOfMessage()))
                conn.start_next_cycle()
            elif isinstance(ev, h11.ConnectionClosed) or ev is h11.PAUSED:
                return
    except Exception:
        try: c.sendall(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        except Exception: pass
    finally: c.close()
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(("127.0.0.1", 9103)); s.listen(16)
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
