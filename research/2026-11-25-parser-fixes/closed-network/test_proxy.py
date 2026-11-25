"""#378's closed-network/proxy.py, for plain http:// requests: every request that reaches an
allowed host names that host, in its URL and in its Host header. The upstream here records the raw
bytes it is sent, so a request carried past the allowlist shows as a request line or a Host header
for another name (#401, finding 12). Run: python3 -m unittest test_proxy -v"""
import json, os, re, socket, subprocess, sys, tempfile, threading, time, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
PROXY = os.environ.get('PROXY_PY', os.path.join(
    HERE, '../../2026-11-25-techniques-from-two-tools/closed-network/proxy.py'))
ALLOWED = 'registry.npmjs.org'


class Upstream:
    """Stands in for the allowed host: answers each connection once, and keeps what it was sent."""
    def __init__(self):
        self.s = socket.socket(); self.s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.s.bind(('127.0.0.1', 0)); self.s.listen(16); self.port = self.s.getsockname()[1]
        self.got = []
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            c, _ = self.s.accept()
            threading.Thread(target=self.one, args=(c,), daemon=True).start()

    def one(self, c):
        c.settimeout(0.5); data = b''
        try:
            while (d := c.recv(65536)):
                data += d
        except socket.timeout:
            pass
        self.got.append(data)
        try:
            c.sendall(b'HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok')
        except OSError:
            pass
        c.close()

    def names(self):
        """Every host a request line or a Host header the upstream received names."""
        out = []
        for data in self.got:
            for line in data.split(b'\n'):        # a bare LF ends a line for many hosts
                m = re.match(rb'[A-Z]+ http://([^/:\s]+)', line)
                if m: out.append(m[1].decode())
                m = re.match(rb'(?i)host:\s*([^:\s]+)', line)
                if m: out.append(m[1].decode())
        return out


def setUpModule():
    global UP, PX, W
    W = tempfile.mkdtemp(prefix='closed-')
    UP = Upstream()
    env = dict(os.environ, SOCK=W + '/p.sock', LOG=W + '/log.jsonl', ALLOW=ALLOWED,
               MAP=json.dumps({f'{ALLOWED}:80': f'127.0.0.1:{UP.port}'}))
    PX = subprocess.Popen([sys.executable, PROXY], env=env)
    for _ in range(50):
        if os.path.exists(W + '/p.sock'): break
        time.sleep(0.05)


def tearDownModule():
    PX.kill()


def send(raw):
    s = socket.socket(socket.AF_UNIX); s.connect(W + '/p.sock'); s.sendall(raw); s.settimeout(2)
    out = b''
    try:
        while (d := s.recv(65536)):
            out += d
    except socket.timeout:
        pass
    s.close()
    time.sleep(0.6)       # the upstream keeps a connection's bytes once it has waited for more
    return out


class Allowlist(unittest.TestCase):
    def setUp(self):
        UP.got.clear()

    def only_allowed(self):
        self.assertEqual(sorted(set(UP.names()) - {ALLOWED}), [], 'a request for another name reached the host')

    def test_an_allowed_request_is_answered(self):
        out = send(b'GET http://registry.npmjs.org/x HTTP/1.1\r\nHost: registry.npmjs.org\r\n\r\n')
        self.assertTrue(out.startswith(b'HTTP/1.1 200'), out)
        self.assertEqual(len(UP.got), 1)
        self.only_allowed()

    def test_a_second_request_on_the_connection(self):
        """Finding 12: the second request names another host."""
        send(b'GET http://registry.npmjs.org/ HTTP/1.1\r\nHost: registry.npmjs.org\r\n\r\n'
             b'GET http://collector.example/x HTTP/1.1\r\nHost: collector.example\r\nConnection: close\r\n\r\n')
        self.only_allowed()

    def test_a_host_header_other_than_the_url(self):
        """A front that serves many names and reads the Host header serves collector.example."""
        send(b'GET http://registry.npmjs.org/ HTTP/1.1\r\nHost: collector.example\r\nConnection: close\r\n\r\n')
        self.only_allowed()

    def test_a_request_hidden_after_a_chunked_body(self):
        send(b'POST http://registry.npmjs.org/ HTTP/1.1\r\nHost: registry.npmjs.org\r\n'
             b'Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n'
             b'GET http://collector.example/x HTTP/1.1\r\nHost: collector.example\r\n\r\n')
        self.only_allowed()

    def test_a_request_after_a_body_of_declared_length(self):
        send(b'POST http://registry.npmjs.org/ HTTP/1.1\r\nHost: registry.npmjs.org\r\n'
             b'Content-Length: 5\r\n\r\nhello'
             b'GET http://collector.example/x HTTP/1.1\r\nHost: collector.example\r\n\r\n')
        self.only_allowed()

    def test_a_header_ended_by_a_bare_lf(self):
        """The proxy and the host must agree where the head ends and what each header is."""
        send(b'GET http://registry.npmjs.org/ HTTP/1.1\r\nHost: registry.npmjs.org\r\n'
             b'x-a: b\nHost: collector.example\r\n\r\n')
        self.only_allowed()


if __name__ == '__main__':
    unittest.main()
