# Times N requests to the metadata server, one connection each. Prints p50 and p99 in µs.
import socket, sys, time
n = int(sys.argv[1]); t = []
for _ in range(n):
    a = time.perf_counter(); s = socket.create_connection(("169.254.169.254", 80))
    s.sendall(b"GET / HTTP/1.0\r\n\r\n"); b = s.recv(4096); s.close(); t.append(time.perf_counter() - a)
    assert b"token-of-" in b
t.sort(); print(f"p50 {t[n//2]*1e6:.0f} µs, p99 {t[int(n*0.99)]*1e6:.0f} µs")
