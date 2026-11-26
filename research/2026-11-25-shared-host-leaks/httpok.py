# A tiny HTTP server answering 200 to anything: a stand-in for a person's service.
# (python3 -m http.server looks up its own name first, which stalls in a namespace with no DNS.)
# argv[1] = address, argv[2] = port.
import socket, sys
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((sys.argv[1], int(sys.argv[2]))); s.listen(64)
while True:
    c, _ = s.accept(); c.recv(4096); c.sendall(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n"); c.close()
