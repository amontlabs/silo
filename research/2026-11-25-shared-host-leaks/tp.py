# Throughput over TCP, a stand-in for iperf3 (which hangs on this kernel without IPv6).
# server <addr> <port>: receives and discards. client <addr> <port> <seconds>: sends 1 MiB
# writes for that long, then prints Gbit/s.
import socket, sys, time
mode, addr, port = sys.argv[1], sys.argv[2], int(sys.argv[3])
if mode == "server":
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind((addr, port)); s.listen(8)
    while True:
        c, _ = s.accept()
        while c.recv(1 << 20): pass
        c.close()
c = socket.create_connection((addr, port)); buf = bytes(1 << 20); n = 0; end = time.time() + float(sys.argv[4]); t0 = time.time()
while time.time() < end: c.sendall(buf); n += len(buf)
c.close(); print(f"{n * 8 / (time.time() - t0) / 1e9:.1f} Gbit/s")
