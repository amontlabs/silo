"""Inside the computer: N requests on one kept-alive connection through the relay at 127.0.0.1:3128.
Prints the median and the slowest in ms, and exits 1 when the median is over LIMIT_MS (default 10)."""
import os, socket, statistics, sys, time
N = int(os.environ.get("N", "50")); LIMIT = float(os.environ.get("LIMIT_MS", "10"))
s = socket.create_connection(("127.0.0.1", 3128))
times = []
for _ in range(N):
    t = time.perf_counter()
    s.sendall(b"GET / HTTP/1.1\r\nhost: x\r\n\r\n")
    got = b""
    while not got.endswith(b"\r\n\r\nok"):
        got += s.recv(4096)
    times.append((time.perf_counter() - t) * 1000)
med = statistics.median(times)
print(f"median {med:.2f} ms, slowest {max(times):.2f} ms, {N} requests")
sys.exit(0 if med <= LIMIT else 1)
