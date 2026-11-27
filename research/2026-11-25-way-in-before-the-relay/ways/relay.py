"""The relay in the computer, at its smallest: each connection on the vsock port (here a unix socket,
which stands in for vsock as in #378's run) gets its own `sshd -i`, so SSH needs no TCP port and
Remote Login on the network card can be turned off.  usage: relay.py <socket> <sshd_config>"""
import os, socket, subprocess, sys
path, cfg = sys.argv[1], sys.argv[2]
try: os.unlink(path)
except FileNotFoundError: pass
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.bind(path); s.listen(16)
while True:
    c, _ = s.accept()
    subprocess.Popen(["/usr/sbin/sshd", "-i", "-f", cfg], stdin=c.fileno(), stdout=c.fileno(), stderr=subprocess.DEVNULL, close_fds=True)
    c.close()
