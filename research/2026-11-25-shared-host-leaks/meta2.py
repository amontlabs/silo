# The boundary's metadata server for every computer of a host, one listener per computer link.
# Every link carries the same address, 169.254.169.254, as clouds do; each listener is bound to
# its link (SO_BINDTODEVICE), so the link a connection came in on names the computer.
# argv[1] = a file of lines "<link> <owner>", read at start. argv[2] = "by-index" (default): the
# listener holds the link it was opened on, by the kernel's index; "by-name": each connection's
# link is looked up by name in the file again, as a server that keys its table by name would.
import socket, sys, threading, os
links = dict(l.split() for l in open(sys.argv[1]))
mode = sys.argv[2] if len(sys.argv) > 2 else "by-index"
def owner_now(dev):
    return dict(l.split() for l in open(sys.argv[1])).get(dev)
def serve(sock, dev, owner):
    while True:
        c, _ = sock.accept()
        c.recv(4096)
        m = owner if mode == "by-index" else owner_now(dev)
        body = f"token-of-{m}\n"
        c.sendall(f"HTTP/1.0 200 OK\r\nContent-Length: {len(body)}\r\n\r\n{body}".encode()); c.close()
for dev, owner in links.items():
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_BINDTODEVICE, dev.encode())
    s.bind(("169.254.169.254", 80)); s.listen(64)
    threading.Thread(target=serve, args=(s, dev, owner), daemon=True).start()
print("listening on", len(links), "links", flush=True)
threading.Event().wait()
