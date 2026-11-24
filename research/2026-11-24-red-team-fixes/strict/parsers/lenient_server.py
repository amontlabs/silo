"""The lenient host of PR #358 (first Content-Length wins, whitespace around names stripped,
Transfer-Encoding wins over Content-Length), without TLS. Stands for the host a boundary must not
assume it agrees with."""
import hashlib, json, socket, threading
def serve(c):
    buf = b""
    def need(k):
        nonlocal buf
        while len(buf) < k:
            d = c.recv(65536)
            if not d: raise EOFError
            buf += d
    try:
        while True:
            while b"\r\n\r\n" not in buf:
                d = c.recv(65536)
                if not d: return
                buf += d
            head, buf = buf.split(b"\r\n\r\n", 1)
            lines = head.split(b"\r\n"); m, t = lines[0].split(b" ")[:2]; hs = []
            for l in lines[1:]:
                k, _, v = l.partition(b":"); hs.append((k.strip().lower(), v.strip()))
            cls = [v for k, v in hs if k == b"content-length"]; te = [v for k, v in hs if k == b"transfer-encoding"]
            host = [v for k, v in hs if k == b"host"]
            if te and b"chunked" in te[0].lower():
                body = b""
                while True:
                    while b"\r\n" not in buf: need(len(buf) + 1)
                    szl, buf = buf.split(b"\r\n", 1); sz = int(szl.split(b";")[0], 16)
                    need(sz + 2); body += buf[:sz]; buf = buf[sz + 2:]
                    if sz == 0: break
            elif cls:
                L = int(cls[0].split(b",")[0]); need(L); body, buf = buf[:L], buf[L:]
            else: body = b""
            o = json.dumps({"parsed": {"method": m.decode(), "target": t.decode(), "host": host[0].decode() if host else None, "len": len(body), "sha": hashlib.sha256(body).hexdigest()[:12]}}).encode()
            c.sendall(b"HTTP/1.1 200 OK\r\ncontent-length: %d\r\n\r\n%s" % (len(o), o))
    except Exception: return
    finally: c.close()
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(("127.0.0.1", 9104)); s.listen(16)
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
