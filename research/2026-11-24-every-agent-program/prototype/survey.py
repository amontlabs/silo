"""A TLS listener that logs what each agent program does when it reaches a host it thinks is its
provider: SNI, ALPN, whether it trusted the certificate, and its first requests (method, path,
header names, content type). Header values are logged only for a few harmless headers."""
import socket, ssl, json, sys, time, os, threading
import h2.connection, h2.config, h2.events
LOG = open(sys.argv[1], "a"); STATUS = int(os.environ.get("STATUS", "401")); MSG = os.environ.get("MSG", "survey")
BODY = json.dumps({"type": "error", "error": {"type": "permission_error" if STATUS == 403 else "rate_limit_error" if STATUS == 429 else "authentication_error", "code": STATUS, "status": "PERMISSION_DENIED" if STATUS == 403 else "RESOURCE_EXHAUSTED" if STATUS == 429 else "UNAUTHENTICATED", "message": MSG}}).encode()
REASON = {401: "Unauthorized", 403: "Forbidden", 429: "Too Many Requests"}.get(STATUS, "Error"); ALPN = os.environ.get("ALPN", "h2,http/1.1").split(","); lock = threading.Lock()
KEEP = {"content-type", "user-agent", "upgrade", "accept", "content-encoding", "x-goog-api-client", "te", "grpc-encoding", "connect-protocol-version", "anthropic-version", "openai-beta", "x-stainless-lang", "editor-version", "copilot-integration-id", "x-github-api-version", "openai-intent", "sec-websocket-protocol", "x-ghost-mode", "x-cursor-client-version", "x-amz-target"}
def log(**k):
    k["t"] = round(time.time(), 3)
    with lock: LOG.write(json.dumps(k) + "\n"); LOG.flush()
HOLD = os.environ.get("HOLD") == "1"
def is_model(p): return any(k in p for k in ("/messages", "/responses", "/chat/completions", "enerateContent"))
def hdrs(pairs): return {k: (v if k in KEEP else "…") for k, v in pairs}
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); ctx.load_cert_chain(sys.argv[2], sys.argv[3]); ctx.set_alpn_protocols(ALPN)
def sni_cb(sock, name, c): sock.context_sni = name
ctx.sni_callback = sni_cb
def serve(raw):
    raw.settimeout(10)
    s = ctx.wrap_socket(raw, server_side=True, do_handshake_on_connect=False)
    try: s.do_handshake()
    except Exception as e:
        log(sni=getattr(s, "context_sni", None), handshake_failed=str(e)[:200]); s.close(); return
    sni = getattr(s, "context_sni", None); alpn = s.selected_alpn_protocol(); log(sni=sni, alpn=alpn, tls=s.version())
    try:
        if alpn == "h2":
            c = h2.connection.H2Connection(h2.config.H2Configuration(client_side=False, header_encoding="utf-8")); c.initiate_connection(); s.sendall(c.data_to_send())
            n = 0
            if HOLD: s.settimeout(None)
            while n < 6:
                data = s.recv(65536)
                if not data: log(sni=sni, alpn=alpn, closed_by_program="h2"); break
                for ev in c.receive_data(data):
                    if isinstance(ev, h2.events.StreamReset): log(sni=sni, alpn=alpn, reset=ev.stream_id)
                    if isinstance(ev, h2.events.RequestReceived):
                        h = dict(ev.headers); n += 1
                        log(sni=sni, alpn=alpn, method=h.get(":method"), path=h.get(":path"), headers=hdrs(ev.headers), stream=ev.stream_id)
                        if HOLD and is_model(h.get(":path") or ""): n -= 1; continue
                        c.send_headers(ev.stream_id, [(":status", str(STATUS)), ("content-type", "application/json")]); c.send_data(ev.stream_id, BODY, end_stream=True)
                s.sendall(c.data_to_send())
        else:
            buf = b""
            while b"\r\n\r\n" not in buf:
                d = s.recv(65536)
                if not d: break
                buf += d
            lines = buf.split(b"\r\n\r\n")[0].decode("latin1").split("\r\n"); parts = (lines[0].split(" ") + ["", ""])
            pairs = [(l.split(":", 1)[0].strip().lower(), l.split(":", 1)[1].strip()) for l in lines[1:] if ":" in l]
            log(sni=sni, alpn=alpn, method=parts[0], path=parts[1], headers=hdrs(pairs))
            if HOLD and is_model(parts[1]):
                s.settimeout(None)
                while True:
                    d = s.recv(65536)
                    if not d: break
                log(sni=sni, alpn=alpn, closed_by_program=parts[1]); s.close(); return
            body = BODY
            s.sendall(b"HTTP/1.1 %d %s\r\ncontent-type: application/json\r\ncontent-length: %d\r\nconnection: close\r\n\r\n" % (STATUS, REASON.encode(), len(body)) + body)
    except Exception as e: log(sni=sni, alpn=alpn, err=repr(e)[:200])
    try: s.close()
    except Exception: pass
ls = socket.socket(); ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); ls.bind(("127.0.0.1", int(os.environ.get("PORT", 443)))); ls.listen(64)
while True:
    c, _ = ls.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
