"""A ProxyCommand: carries stdin/stdout to host:port. Stands for a tunnel an outsider gets into the
computer (a WebSocket to an allowed host, say), with the computer opening the TCP connection."""
import os, socket, sys, threading
c = socket.create_connection((sys.argv[1], int(sys.argv[2])))
def up():
    while (d := os.read(0, 65536)): c.sendall(d)
    c.shutdown(socket.SHUT_WR)
threading.Thread(target=up, daemon=True).start()
while (d := c.recv(65536)): os.write(1, d)
