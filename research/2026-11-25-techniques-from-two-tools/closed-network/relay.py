"""The computer's side of a closed network: 127.0.0.1:3128 relayed to the Mac's proxy.

Run inside a network namespace that has only the loopback. Each connection to 127.0.0.1:3128 is
carried to the Unix socket SOCK, which stands in for vsock: a channel between the computer and
the Mac that is not a network. It also brings the loopback up, since a new namespace starts with
it down.

Environment: SOCK.
"""
import fcntl, os, socket, struct, threading

SIOCGIFFLAGS, SIOCSIFFLAGS, IFF_UP = 0x8913, 0x8914, 0x1
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
ifr = struct.pack("16sH", b"lo", 0)
flags = struct.unpack("16sH", fcntl.ioctl(s, SIOCGIFFLAGS, ifr))[1]
fcntl.ioctl(s, SIOCSIFFLAGS, struct.pack("16sH", b"lo", flags | IFF_UP))
s.close()

SOCK = os.environ["SOCK"]


def pump(a, b):
    try:
        while True:
            d = a.recv(65536)
            if not d:
                break
            b.sendall(d)
    except OSError:
        pass
    finally:
        try:
            b.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle(c):
    u = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        u.connect(SOCK)
    except OSError:
        c.close(); return
    t = threading.Thread(target=pump, args=(c, u)); t.start(); pump(u, c); t.join()
    c.close(); u.close()


L = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
L.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
L.bind(("127.0.0.1", 3128)); L.listen(64)
open(os.environ.get("READY", "/dev/null"), "w").write("ready\n")
while True:
    c, _ = L.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
