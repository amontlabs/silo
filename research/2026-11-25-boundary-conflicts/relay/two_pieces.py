"""Stands in for the boundary behind the relay: a Unix socket server that answers each request of a
kept-alive HTTP/1.1 connection in two writes, the head and then the body a millisecond later, as
the boundary does when the body comes from the host after the head.
usage: two_pieces.py <socket>"""
import os, socket, sys, threading, time

def serve(c):
    buf = b""
    try:
        while True:
            while b"\r\n\r\n" not in buf:
                d = c.recv(65536)
                if not d:
                    return
                buf += d
            _, buf = buf.split(b"\r\n\r\n", 1)
            c.sendall(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n")
            time.sleep(0.001)
            c.sendall(b"ok")
    finally:
        c.close()

p = sys.argv[1]
if os.path.exists(p):
    os.unlink(p)
L = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); L.bind(p); L.listen(16)
while True:
    c, _ = L.accept()
    threading.Thread(target=serve, args=(c,), daemon=True).start()
