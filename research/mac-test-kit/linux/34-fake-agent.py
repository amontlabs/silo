"""A stand-in for Claude Code or Codex in the Linux run of test 34, called by guest/34-night.sh as
claude or codex. It reads its sign-in from the fake guest's home, talks to the providers' hosts at
SILOKIT_34_AT (the boundary, or the fake providers directly in the control) with the authority of
NODE_EXTRA_CA_CERTS or CODEX_CA_CERTIFICATE, and prints its own events as the real programs do
(stream-json, codex exec --json). Each launch asks for the same replies that are not normal ones, so
the night and the control see the same errors.

  claude -p PROMPT [--resume ID] ...
  codex exec ... [resume ID] PROMPT
"""
import base64
import gzip
import http.client
import json
import os
import socket
import ssl
import struct
import sys
import uuid
import zlib

HOME = os.environ["HOME"]
AT = os.environ.get("SILOKIT_34_AT", "127.0.0.1:443")


def out(o):
    sys.stdout.write(json.dumps(o, separators=(",", ":")) + "\n")
    sys.stdout.flush()


class Pinned(http.client.HTTPSConnection):
    def __init__(self, host, ctx):
        a, p = AT.rsplit(":", 1)
        http.client.HTTPSConnection.__init__(self, host, int(p), context=ctx, timeout=20)
        self._addr = a

    def connect(self):
        s = socket.create_connection((self._addr, self.port), self.timeout)
        self.sock = self._context.wrap_socket(s, server_hostname=self.host)


def ctx_for(var):
    return ssl.create_default_context(cafile=os.environ[var])


def claude(argv):
    sid = argv[argv.index("--resume") + 1] if "--resume" in argv else str(uuid.uuid4())
    cred = json.load(open(os.path.join(HOME, ".claude/.credentials.json")))
    tok = cred["claudeAiOauth"]["accessToken"]
    out({"type": "system", "subtype": "init", "session_id": sid})
    c = Pinned("api.anthropic.com", ctx_for("NODE_EXTRA_CA_CERTS"))
    base = {"Authorization": "Bearer " + tok, "x-claude-code-session-id": sid, "anthropic-version": "2023-06-01",
            "user-agent": "claude-cli/2.1.999 (fake)", "x-app": "cli", "Cookie": "__cf_bm=" + cred["silokitCookie"]}
    c.request("GET", "/api/claude_code/settings", headers=base)
    c.getresponse().read()

    def messages(fake, retry, tools=True):
        body = {"model": "claude-opus-5-5" if tools else "claude-haiku-4-5", "stream": True, "max_tokens": 100,
                "messages": [{"role": "user", "content": "hi"}], "tools": [{"name": "Bash"}] if tools else [],
                "metadata": {"user_id": json.dumps({"device_id": "d" * 64, "account_uuid": "acc-0001", "session_id": sid})},
                "silokit_planted_token": cred["claudeAiOauth"]["refreshToken"]}
        h = dict(base, **{"content-type": "application/json", "content-encoding": "gzip",
                          "x-stainless-retry-count": str(retry)})
        if fake:
            h["x-silokit-fake"] = fake
        cc = Pinned("api.anthropic.com", ctx_for("NODE_EXTRA_CA_CERTS"))
        try:
            cc.request("POST", "/v1/messages?beta=true", body=gzip.compress(json.dumps(body).encode()), headers=h)
            r = cc.getresponse()
            data = r.read()
        except (http.client.HTTPException, OSError):
            return "cut"
        if r.status != 200:
            return "status-%d" % r.status
        if b"event: error" in data:
            return "stream-error"
        return "ok" if b"message_stop" in data else "cut"

    for fake in ("", "429", "529", "cut", "midstream", ""):
        got = messages(fake, 0 if not fake else 1)
        if got != "ok":
            out({"type": "system", "subtype": "api_retry", "error": got, "session_id": sid})
    messages("", 0, tools=False)
    out({"type": "result", "subtype": "success", "is_error": False, "session_id": sid})


def ws_frame(data, op=1, deflate=None):
    rsv = 0
    if deflate is not None and op == 1:
        data, rsv = (deflate.compress(data) + deflate.flush(zlib.Z_SYNC_FLUSH))[:-4], 0x40
    n, mask = len(data), os.urandom(4)
    hd = bytes([0x80 | rsv | op]) + (bytes([0x80 | n]) if n < 126 else bytes([0x80 | 126]) + struct.pack(">H", n))
    return hd + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data))


def codex(argv):
    args = [a for a in argv if not a.startswith("-")]
    tid = args[args.index("resume") + 1] if "resume" in args else str(uuid.uuid4())
    auth = json.load(open(os.path.join(HOME, ".codex/auth.json")))["tokens"]
    out({"type": "thread.started", "thread_id": tid})
    ctx = ctx_for("CODEX_CA_CERTIFICATE")
    base = {"Authorization": "Bearer " + auth["access_token"], "chatgpt-account-id": auth["account_id"],
            "session-id": tid, "originator": "codex_exec", "user-agent": "codex_exec/0.999.0 (fake)"}
    host = "chatgpt.com"
    path = "/backend-api/codex/responses"
    if "resume" not in args:
        # A socket the provider refuses, then plain requests, as Codex falls back.
        c = Pinned(host, ctx)
        c.request("GET", path, headers=dict(base, **{"Upgrade": "websocket", "Connection": "Upgrade",
                                                      "Sec-WebSocket-Key": base64.b64encode(os.urandom(16)).decode(),
                                                      "Sec-WebSocket-Version": "13", "x-silokit-fake": "nows"}))
        r = c.getresponse()
        r.read()
        out({"type": "error", "message": "Reconnecting... 1/5 (websocket refused: %d)" % r.status})
        for fake in ("", "429", ""):
            c = Pinned(host, ctx)
            body = {"model": "gpt-5.5-codex", "stream": True, "store": False, "prompt_cache_key": tid,
                    "input": [{"type": "message", "role": "user", "content": "hi"}], "tools": [{"name": "shell"}]}
            h = dict(base, **{"content-type": "application/json"})
            if fake:
                h["x-silokit-fake"] = fake
            c.request("POST", path, body=json.dumps(body), headers=h)
            r = c.getresponse()
            data = r.read()
            if r.status != 200 or b"response.completed" not in data:
                out({"type": "error", "message": "stream error: retrying 1/5 (%d)" % r.status})
    else:
        # The socket, compressed: a warm-up, the whole conversation, then only what is new.
        s = ctx.wrap_socket(socket.create_connection((AT.rsplit(":", 1)[0], int(AT.rsplit(":", 1)[1])), 20), server_hostname=host)
        key = base64.b64encode(os.urandom(16)).decode()
        head = "GET %s HTTP/1.1\r\nHost: %s\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: %s\r\n" \
               "Sec-WebSocket-Version: 13\r\nSec-WebSocket-Extensions: permessage-deflate; client_max_window_bits\r\n" % (path, host, key)
        head += "".join("%s: %s\r\n" % kv for kv in base.items()) + "\r\n"
        s.sendall(head.encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            buf += s.recv(4096)
        resp, buf = buf.split(b"\r\n\r\n", 1)
        deflate = zlib.compressobj(6, zlib.DEFLATED, -15) if b"permessage-deflate" in resp.lower() else None
        inflate = zlib.decompressobj(-15)

        def recv_msg():
            nonlocal buf
            while True:
                while len(buf) < 2:
                    buf += s.recv(65536)
                n, i = buf[1] & 0x7F, 2
                if n == 126:
                    while len(buf) < 4:
                        buf += s.recv(65536)
                    n, i = struct.unpack(">H", buf[2:4])[0], 4
                while len(buf) < i + n:
                    buf += s.recv(65536)
                comp, data, buf = buf[0] & 0x40, buf[i:i + n], buf[i + n:]
                if comp:
                    data = inflate.decompress(data + b"\x00\x00\xff\xff")
                return json.loads(data)

        prev = None
        for k, inp in enumerate([[], [{"type": "message", "role": "user", "content": "go on"}],
                                 [{"type": "function_call_output", "call_id": "call_1", "output": "ok"}]]):
            msg = {"type": "response.create", "model": "gpt-5.5-codex", "store": False, "prompt_cache_key": tid, "input": inp}
            if k == 0:
                msg["generate"] = False
            if prev:
                msg["previous_response_id"] = prev
            s.sendall(ws_frame(json.dumps(msg).encode(), deflate=deflate))
            while True:
                e = recv_msg()
                if e.get("type") == "response.completed":
                    prev = e["response"]["id"]
                    break
        s.sendall(ws_frame(b"\x03\xe8", op=8))
        s.close()
    out({"type": "turn.completed", "usage": {"input_tokens": 150, "output_tokens": 60}})


if __name__ == "__main__":
    name = os.path.basename(sys.argv[1])
    if "--version" in sys.argv:
        print({"claude": "2.1.999 (Claude Code, fake)", "codex": "codex-cli 0.999.0 (fake)"}[name])
        sys.exit(0)
    {"claude": claude, "codex": codex}[name](sys.argv[2:])
