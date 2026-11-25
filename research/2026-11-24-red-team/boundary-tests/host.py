"""A fake provider host with a lenient HTTP/1.1 parser, the kind of parser a boundary must not
assume it shares with the host. Logs each request it parsed, as JSON lines."""
import json, socket, ssl, sys, threading, hashlib
port, cert, key, log = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); cx.load_cert_chain(cert, key); cx.set_alpn_protocols(["http/1.1"])
def serve(c):
    c = cx.wrap_socket(c, server_side=True); buf = b""; n = 0
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
            lines = head.split(b"\r\n"); first = lines[0].decode("latin1"); hs = []
            for l in lines[1:]:
                k, _, v = l.partition(b":"); hs.append((k.strip().lower().decode("latin1"), v.strip().decode("latin1")))  # lenient: strips tabs too
            cls = [v for k, v in hs if k == "content-length"]; te = [v for k, v in hs if k == "transfer-encoding"]
            host = [v for k, v in hs if k == "host"]
            if te and "chunked" in te[0].lower():  # Transfer-Encoding wins over Content-Length, as RFC 9112 says
                body = b""
                while True:
                    while b"\r\n" not in buf: need(len(buf) + 1)
                    szl, buf = buf.split(b"\r\n", 1); sz = int(szl.split(b";")[0], 16)
                    need(sz + 2); body += buf[:sz]; buf = buf[sz + 2:]
                    if sz == 0: break
            elif cls:  # lenient: the first of two Content-Length headers wins
                L = int(cls[0]); need(L); body, buf = buf[:L], buf[L:]
            else: body = b""
            n += 1
            with open(log, "a") as f:
                f.write(json.dumps({"first": first, "host": host, "body_len": len(body), "body_head": body[:120].decode("latin1"), "sha": hashlib.sha256(body).hexdigest()[:16]}) + "\n")
            out = json.dumps({"host_parsed_request": n, "line": first}).encode()
            c.sendall(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: %d\r\n\r\n%s" % (len(out), out))
    except (EOFError, ConnectionError, ssl.SSLError): return
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(("127.0.0.1", port)); s.listen(16)
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
