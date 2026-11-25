"""A dev server inside the computer that tries to take its page out of the preview's policy.
usage: app.py PORT MODE. Each mode answers with a page whose script fetches a host the policy's
connect-src forbids (the sink) and then sets the title to "ran".

  crlf     a well-formed head: the policy must hold
  lf       a head ended by bare LFs, and a CRLF pair in the body where the forwarder of #384 stops
           (#401, finding 10)
  interim  a 100 Continue first, then the real head: the policy lands on the interim answer,
           which Chromium drops
  hints    the same with 103 Early Hints
  fold     a header line folded onto the one before it
"""
import socket, sys, threading
MODE = sys.argv[2]
PAGE = (b"<script>fetch('http://127.0.0.1:7999/leak?mode=" + MODE.encode()
        + b"').catch(()=>{});document.title='ran'</script>")
FINAL = (b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: %d\r\nconnection: close\r\n\r\n"
         % len(PAGE) + PAGE)
ANSWER = {
    "crlf": FINAL,
    "lf": b"HTTP/1.1 200 OK\ncontent-type: text/html\nconnection: close\n\n" + PAGE + b"\r\n\r\n",
    "interim": b"HTTP/1.1 100 Continue\r\n\r\n" + FINAL,
    "hints": b"HTTP/1.1 103 Early Hints\r\nlink: </x.css>; rel=preload\r\n\r\n" + FINAL,
    "fold": (b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\nx-a: b\r\n c\r\ncontent-length: %d\r\n"
             b"connection: close\r\n\r\n" % len(PAGE) + PAGE),
}[MODE]
def serve(c):
    c.recv(65536); c.sendall(ANSWER); c.close()
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen()
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,)).start()
