# A stand-in for the boundary's metadata server (cloud-credentials, #379), shared by every computer
# of the host. It answers GET / with the token of the member whose computer it thinks is asking.
# $1 = by-address: it knows a computer by the source address of the connection, as one shared
#      server on a bridge must.
# $1 = by-link: one listener per computer link, bound to that link (SO_BINDTODEVICE); the link the
#      connection arrived on names the computer, whatever address the computer claims.
import socket, sys, threading
mode = sys.argv[1]
owners = {"10.9.0.11": "A", "10.9.0.12": "B"}            # by-address
links = {"hA": ("10.9.1.1", "A"), "hB": ("10.9.2.1", "B")}  # by-link
def serve(sock, who):
    while True:
        c, (addr, _) = sock.accept()
        c.recv(4096)
        m = who(addr)
        body = f"token-of-{m}\n" if m else "unknown\n"
        c.sendall(f"HTTP/1.0 200 OK\r\nContent-Length: {len(body)}\r\n\r\n{body}".encode()); c.close()
def listen(ip, dev=None):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    if dev: s.setsockopt(socket.SOL_SOCKET, socket.SO_BINDTODEVICE, dev.encode())
    s.bind((ip, 80)); s.listen(); return s
if mode == "by-address":
    serve(listen("10.9.0.1"), owners.get)
else:
    for dev, (ip, m) in links.items():
        threading.Thread(target=serve, args=(listen(ip, dev), lambda a, m=m: m), daemon=True).start()
    threading.Event().wait()
