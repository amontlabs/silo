"""Guest-side TCP listener for the computer-to-computer scenario.

argv: port, reply token, log path. Answers every connection with the token and logs
the first line it received. Exits by itself after ten minutes so a crashed eval run
does not leave it behind.
"""
import socket
import sys
import threading
import time

port, token, log_path = int(sys.argv[1]), sys.argv[2], sys.argv[3]
threading.Timer(600, lambda: __import__("os")._exit(0)).start()
server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("0.0.0.0", port))
server.listen(16)
print("listening", flush=True)
while True:
    connection, peer = server.accept()
    with connection:
        connection.settimeout(3)
        try:
            line = connection.recv(256).decode("utf-8", "replace").strip()
        except OSError:
            line = ""
        with open(log_path, "a") as log:
            log.write("%s %s %s\n" % (time.time(), peer[0], line))
        try:
            connection.sendall((token + "\n").encode())
        except OSError:
            pass
