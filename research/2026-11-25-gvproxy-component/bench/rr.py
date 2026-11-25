"""rr.py server PORT | rr.py client HOST PORT: one-byte round trips on one connection, then new
connections. Prints the median round trip and connections per second."""
import socket, sys, time, statistics, threading
if sys.argv[1] == "server":
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("0.0.0.0", int(sys.argv[2]))); s.listen(512)
    def h(c):
        c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        while (b := c.recv(1)): c.sendall(b)
        c.close()
    while True: threading.Thread(target=h, args=(s.accept()[0],), daemon=True).start()
host, port = sys.argv[2], int(sys.argv[3])
c = socket.create_connection((host, port)); c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
rt = []
for _ in range(3000):
    t = time.perf_counter(); c.sendall(b"x"); c.recv(1); rt.append(time.perf_counter() - t)
c.close()
t = time.perf_counter(); n = 300
for _ in range(n):
    k = socket.create_connection((host, port)); k.sendall(b"x"); k.recv(1); k.close()
cps = n / (time.perf_counter() - t)
print(f"rtt_p50_us={statistics.median(rt)*1e6:.0f} rtt_p99_us={sorted(rt)[int(len(rt)*.99)]*1e6:.0f} conns_per_s={cps:.0f}")
