"""Small servers the suite places around the computer. Each logs what it saw to a file.
usage: servers.py tcp <addr> <port> <text>     a TCP service that answers <text> to any line
       servers.py udp <addr> <port> <log>       a UDP echo that logs each datagram
       servers.py dns <addr> <log> [<answer>]   a DNS server that logs every name asked, answers A <answer>"""
import socket, sys, struct, threading
kind = sys.argv[1]
if kind == "tcp":
    a, p, text = sys.argv[2], int(sys.argv[3]), sys.argv[4]
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind((a, p)); s.listen(64)
    def serve(c):
        try:
            c.settimeout(3); c.recv(1024); c.sendall(text.encode() + b"\n")
        except Exception: pass
        c.close()
    while True:
        c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
elif kind == "udp":
    a, p, log = sys.argv[2], int(sys.argv[3]), sys.argv[4]
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind((a, p))
    while True:
        d, peer = s.recvfrom(65535)
        open(log, "a").write(f"{peer[0]} {d!r}\n"); s.sendto(b"echo:" + d, peer)
elif kind == "dns":
    a, log = sys.argv[2], sys.argv[3]; ans = socket.inet_aton(sys.argv[4]) if len(sys.argv) > 4 else None
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind((a, 53))
    while True:
        d, peer = s.recvfrom(4096)
        i, labels = 12, []
        while d[i]:
            labels.append(d[i+1:i+1+d[i]].decode(errors="replace")); i += 1 + d[i]
        qtype = struct.unpack(">H", d[i+1:i+3])[0]; q = d[12:i+5]
        open(log, "a").write(f"{peer[0]} {'.'.join(labels)} {qtype}\n")
        if ans and qtype == 1:
            r = d[:2] + b"\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00" + q + b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04" + ans
        else:
            r = d[:2] + b"\x81\x83\x00\x01\x00\x00\x00\x00\x00\x00" + q
        s.sendto(r, peer)
