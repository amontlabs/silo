"""The stand-in boundary of test 34: ends TLS for the providers' hosts, forwards every byte as it
came, both ways, and reads a copy. Python 3.9, no packages. Silo's own boundary (step 6 of #373)
does not exist yet; this one is only as good as the test needs.

  34-boundary.py serve --port-file F --cert CHAIN --key KEY --dir OUT [--max-seconds N]
        [--upstream-ca F] [--upstream-at host=ip:port,...]
  34-boundary.py analyze --calls OUT/calls.jsonl --conns OUT/conns.jsonl --guest DIR
        --night-hours H --control-hours H [--until T]   facts (name TAB value lines), then a verdict line
  34-boundary.py scan DIR... [-SKIP_DIR...]          reads credential files on stdin (documents
                                                     separated by a line ---silokit---), prints
                                                     where any of their values is found; never a value

serve writes, under OUT:
  calls.jsonl   one line per call (an HTTP request and its reply, or one message on a WebSocket and
                the events that answer it): host, path, status, timing, sizes, the shape of the
                reply, the ids that name a conversation, the plan's headers, and what was not normal;
  conns.jsonl   one line per connection: opened, ended, refused handshakes;
  scan.json     when OUT/scan-request appears (folders one a line, "-" before one to leave out): where any secret this
                process saw in a request (Authorization, x-api-key, cookies, tokens in bodies) is
                found in those folders. The secrets are only ever in this process's memory.

Nothing it writes holds a header value outside a fixed list, a body, a cookie or a token: each line
is checked against every secret seen before it is written.
"""
import gzip
import json
import os
import re
import socket
import ssl
import struct
import sys
import threading
import time
import zlib

# Response headers kept, by prefix or name: the plan's share and rate limits, the provider's
# request id, and what says a reply is a challenge rather than an answer.
KEEP_PREFIX = ("anthropic-ratelimit-", "x-ratelimit-", "x-codex-", "openai-")
KEEP_RESP = {"retry-after", "x-should-retry", "request-id", "x-request-id", "content-type",
             "content-encoding", "cf-mitigated", "server", "x-envoy-upstream-service-time"}
# Request headers kept by value; every other request header is kept by name only.
KEEP_REQ = {"user-agent", "anthropic-version", "anthropic-beta", "x-app", "content-encoding",
            "content-type", "x-stainless-retry-count", "x-stainless-timeout", "originator", "version",
            "x-claude-code-session-id", "x-claude-code-agent-id", "session-id", "session_id",
            "thread-id", "thread_id", "conversation_id", "conversation-id", "x-codex-window-id",
            "x-codex-parent-thread-id", "x-openai-subagent", "openai-beta", "sec-websocket-extensions",
            "accept", "accept-encoding", "connection", "upgrade"}
SECRET_REQ = {"authorization", "x-api-key", "proxy-authorization", "chatgpt-account-id", "openai-organization",
              "openai-project", "x-codex-turn-state"}
SECRET_KEY = re.compile(r"(token|secret|password|api[_-]?key|credential|session_key|code_verifier)", re.I)
TERMINAL = {"message_stop", "response.completed", "response.done", "response.failed", "response.incomplete"}
MAX_COPY = 64 * 1024 * 1024


def now():
    return time.time()


# --- secrets: kept in memory, checked before each line is written ------------------------------

class Secrets:
    def __init__(self):
        self.values = set()
        self.lock = threading.Lock()
        self.redactions = 0

    def add(self, v):
        if isinstance(v, bytes):
            v = v.decode("latin-1")
        v = (v or "").strip()
        if v.lower().startswith(("bearer ", "basic ")):
            v = v.split(None, 1)[1] if len(v.split(None, 1)) > 1 else ""
        if len(v) >= 12:
            with self.lock:
                self.values.add(v)
                # A JWT's claims and signature are each as secret as the whole.
                for part in v.split(".")[1:]:
                    if len(part) >= 20:
                        self.values.add(part)

    def add_json(self, obj):
        """Every string under a key that names a token, a secret or a password."""
        stack = [(None, obj)]
        while stack:
            k, o = stack.pop()
            if isinstance(o, dict):
                stack.extend(o.items())
            elif isinstance(o, list):
                stack.extend((k, x) for x in o)
            elif isinstance(o, str) and k and SECRET_KEY.search(str(k)):
                self.add(o)

    def clean(self, line):
        with self.lock:
            vals = list(self.values)
        for v in vals:
            if v in line:
                line = line.replace(v, "<redacted>")
                self.redactions += 1
        return line


def find_in_dirs(values, dirs, skip=()):
    """The files under dirs, outside the folders in skip, that hold any of values, as text, gzip
    or not."""
    vals = [v.encode() for v in values if len(v) >= 12]
    skip = [os.path.abspath(x) for x in skip]
    found, files = [], 0
    for d in dirs:
        for root, subdirs, names in os.walk(d):
            subdirs[:] = [x for x in subdirs if os.path.abspath(os.path.join(root, x)) not in skip]
            for n in names:
                p = os.path.join(root, n)
                if os.path.islink(p) or not os.path.isfile(p):
                    continue
                files += 1
                try:
                    with open(p, "rb") as f:
                        data = f.read(512 * 1024 * 1024)
                except OSError:
                    continue
                blobs = [data]
                if data[:2] == b"\x1f\x8b":
                    try:
                        blobs.append(gzip.decompress(data))
                    except (OSError, EOFError, zlib.error):
                        pass
                if any(v in b for v in vals for b in blobs):
                    found.append(p)
    return {"values": len(vals), "files": files, "found": found}


# --- reading a copy: chunked framing, compression, server-sent events, WebSocket frames ---------

class Chunked:
    """Follows a chunked body as bytes arrive: data for the copy, and where the body ends."""

    def __init__(self):
        self.buf, self.state, self.left, self.done, self.data = b"", "size", 0, False, []
        self.malformed = False

    def feed(self, b):
        """Returns the number of bytes of b that belong to the body (all of them until the end)."""
        start = len(self.buf)
        self.buf += b
        i = 0
        while not self.done:
            if self.state == "size":
                j = self.buf.find(b"\r\n", i)
                if j < 0:
                    break
                try:
                    self.left = int(self.buf[i:j].split(b";")[0].strip() or b"x", 16)
                except ValueError:
                    self.malformed, self.done = True, True
                    i = len(self.buf)
                    break
                i = j + 2
                self.state = "data" if self.left else "trailer"
            elif self.state == "data":
                take = min(self.left, len(self.buf) - i)
                if take <= 0:
                    break
                self.data.append(self.buf[i:i + take])
                self.left -= take
                i += take
                if self.left == 0:
                    self.state = "crlf"
            elif self.state == "crlf":
                if len(self.buf) - i < 2:
                    break
                i += 2
                self.state = "size"
            elif self.state == "trailer":
                j = self.buf.find(b"\r\n", i)
                if j < 0:
                    break
                line = self.buf[i:j]
                i = j + 2
                if line == b"":
                    self.done = True
        used = i - start
        self.buf = self.buf[i:]
        if self.done:
            return max(0, used)
        return len(b)

    def take(self):
        out = b"".join(self.data)
        self.data = []
        return out


class Decoder:
    """Content-Encoding of a copy, as bytes arrive. Unknown codings are named, not read."""

    def __init__(self, coding):
        self.coding = (coding or "identity").strip().lower()
        self.d = None
        self.unreadable = None
        if self.coding in ("gzip", "x-gzip"):
            self.d = zlib.decompressobj(16 + zlib.MAX_WBITS)
        elif self.coding == "deflate":
            self.d = zlib.decompressobj()
        elif self.coding not in ("identity", ""):
            self.unreadable = self.coding

    def feed(self, b):
        if self.unreadable:
            return b""
        if self.d is None:
            return b
        try:
            return self.d.decompress(b)
        except zlib.error as e:
            self.unreadable = "%s: %s" % (self.coding, e)
            return b""


class SSE:
    """Server-sent events from a decoded copy."""

    def __init__(self, on_event):
        self.buf, self.on_event, self.n = "", on_event, 0

    def feed(self, text):
        self.buf += text.replace("\r\n", "\n")
        while "\n\n" in self.buf:
            block, self.buf = self.buf.split("\n\n", 1)
            ev, data = None, []
            for line in block.split("\n"):
                if line.startswith("event:"):
                    ev = line[6:].strip()
                elif line.startswith("data:"):
                    data.append(line[5:].lstrip())
            if not data and ev is None:
                continue
            self.n += 1
            raw = "\n".join(data)
            try:
                obj = json.loads(raw) if raw and raw != "[DONE]" else {}
            except ValueError:
                obj = {}
            if not isinstance(obj, dict):
                obj = {}
            self.on_event(ev or (obj.get("type") if isinstance(obj, dict) else None) or "message", obj)


class WSReader:
    """WebSocket frames of one direction, from a copy: masks, fragments, permessage-deflate."""

    def __init__(self, deflate, no_takeover, on_message):
        self.buf = b""
        self.deflate, self.no_takeover = deflate, no_takeover
        self.z = zlib.decompressobj(-15)
        self.parts, self.op, self.comp = [], None, False
        self.on_message = on_message

    def feed(self, b):
        self.buf += b
        while True:
            if len(self.buf) < 2:
                return
            b0, b1 = self.buf[0], self.buf[1]
            fin, rsv1, op = b0 & 0x80, b0 & 0x40, b0 & 0x0F
            masked, n, i = b1 & 0x80, b1 & 0x7F, 2
            if n == 126:
                if len(self.buf) < 4:
                    return
                n, i = struct.unpack(">H", self.buf[2:4])[0], 4
            elif n == 127:
                if len(self.buf) < 10:
                    return
                n, i = struct.unpack(">Q", self.buf[2:10])[0], 10
            mk = b""
            if masked:
                if len(self.buf) < i + 4:
                    return
                mk, i = self.buf[i:i + 4], i + 4
            if len(self.buf) < i + n:
                return
            data = self.buf[i:i + n]
            self.buf = self.buf[i + n:]
            if masked:
                data = bytes(c ^ mk[k % 4] for k, c in enumerate(data))
            if op >= 8:
                self.on_message(op, data)
                continue
            if op != 0:
                self.op, self.comp, self.parts = op, bool(rsv1), []
            self.parts.append(data)
            if fin:
                msg = b"".join(self.parts)
                if self.deflate and self.comp:
                    try:
                        msg = self.z.decompress(msg + b"\x00\x00\xff\xff")
                    except zlib.error:
                        msg = b""
                    if self.no_takeover:
                        self.z = zlib.decompressobj(-15)
                self.on_message(self.op, msg)
                self.parts = []


# --- what a request and its reply say ----------------------------------------------------------

def session_from_user_id(uid):
    """Claude Code's metadata.user_id: a JSON string holding session_id, or the older
    user_<hash>_account_<uuid>_session_<uuid> form. Only the session is kept."""
    if not isinstance(uid, str):
        return None
    try:
        o = json.loads(uid)
        if isinstance(o, dict) and o.get("session_id"):
            return str(o["session_id"])
    except ValueError:
        pass
    m = re.search(r"session_([0-9a-fA-F-]{8,})", uid)
    return m.group(1) if m else None


def provider_of(host, path):
    if host.endswith("anthropic.com"):
        return "anthropic"
    if host.endswith("chatgpt.com") or host.endswith("openai.com"):
        return "openai"
    return "other"


def is_model_path(provider, method, path):
    p = path.split("?")[0]
    if provider == "anthropic":
        return method == "POST" and p.startswith("/v1/messages") and "count_tokens" not in p
    if provider == "openai":
        return method == "POST" and (p.endswith("/responses") or p.endswith("/chat/completions"))
    return False


def read_request_body(rec, provider, obj, secrets):
    if not isinstance(obj, dict):
        return
    secrets.add_json(obj)
    ids = rec["ids"]
    if obj.get("model"):
        rec["model_asked"] = str(obj["model"])[:80]
    if "stream" in obj:
        rec["stream_asked"] = bool(obj.get("stream"))
    rec["tools"] = len(obj.get("tools") or [])
    if provider == "anthropic":
        rec["messages"] = len(obj.get("messages") or [])
        s = session_from_user_id((obj.get("metadata") or {}).get("user_id"))
        if s:
            ids["claude_session_meta"] = s
    else:
        inp = obj.get("input")
        rec["input_items"] = len(inp) if isinstance(inp, list) else (1 if inp else 0)
        for k in ("prompt_cache_key", "previous_response_id"):
            if obj.get(k):
                ids[k] = str(obj[k])[:120]
        if obj.get("generate") is False:
            rec["warmup"] = True
        last = inp[-1] if isinstance(inp, list) and inp else None
        if isinstance(last, dict) and last.get("type") == "compaction_trigger":
            rec["compaction"] = True


def ids_from_headers(headers):
    out = {}
    for name, key in (("x-claude-code-session-id", "claude_session"), ("x-claude-code-agent-id", "claude_agent"),
                      ("session-id", "codex_session"), ("session_id", "codex_session"),
                      ("thread-id", "codex_thread"), ("thread_id", "codex_thread"),
                      ("conversation_id", "codex_conversation"), ("conversation-id", "codex_conversation"),
                      ("x-codex-window-id", "codex_window"), ("x-codex-parent-thread-id", "codex_parent")):
        if headers.get(name):
            out[key] = headers[name][:120]
    return out


def kept_response_headers(headers):
    out = {}
    for k, v in headers.items():
        if k.startswith(KEEP_PREFIX) or k in KEEP_RESP:
            out[k] = v[:160]
    return out


class Reply:
    """What the events of one reply say."""

    def __init__(self, rec):
        self.rec = rec

    def event(self, ev, obj):
        r = self.rec
        r["events"] = r.get("events", 0) + 1
        if ev in TERMINAL or (isinstance(obj, dict) and obj.get("type") in TERMINAL):
            r["terminal"] = ev if ev in TERMINAL else obj.get("type")
        t = obj.get("type") if isinstance(obj, dict) else None
        if ev == "error" or t == "error":
            err = obj.get("error") if isinstance(obj.get("error"), dict) else obj
            r.setdefault("stream_errors", []).append(str(err.get("type") or err.get("code") or "error")[:60])
        if ev == "message_start" or t == "message_start":
            m = obj.get("message") or {}
            r["response_id"] = m.get("id")
            r["model_answered"] = m.get("model")
            u = m.get("usage") or {}
            r["usage"] = {k: u.get(k) for k in ("input_tokens", "output_tokens", "cache_read_input_tokens",
                                                 "cache_creation_input_tokens") if k in u}
        if t == "message_delta" or ev == "message_delta":
            u = obj.get("usage") or {}
            if "output_tokens" in u:
                r.setdefault("usage", {})["output_tokens"] = u["output_tokens"]
            r["stop_reason"] = (obj.get("delta") or {}).get("stop_reason")
        if t and t.startswith("response.") and isinstance(obj.get("response"), dict):
            resp = obj["response"]
            if resp.get("id"):
                r["response_id"] = resp["id"]
            if resp.get("model"):
                r["model_answered"] = resp["model"]
            if resp.get("usage"):
                u = resp["usage"]
                r["usage"] = {"input_tokens": u.get("input_tokens"), "output_tokens": u.get("output_tokens"),
                              "cached_tokens": (u.get("input_tokens_details") or {}).get("cached_tokens")}
            if t in ("response.failed", "response.incomplete"):
                e = resp.get("error") or resp.get("incomplete_details") or {}
                r.setdefault("stream_errors", []).append("%s:%s" % (t, str(e.get("code") or e.get("reason") or e.get("type") or "")[:40]))
        if t and "rate_limit" in t:
            # Codex's own event on its socket: keep its numbers, never more than 600 characters.
            r["rate_limit_event"] = json.dumps(obj, sort_keys=True)[:600]


def ch_cut(rheaders, ended_by_close, ch):
    """A chunked reply whose last chunk never came."""
    return ended_by_close and ch is not None and not ch.done


def abnormal_of(rec):
    """Why a reply is not a normal one, or None."""
    st = rec.get("status")
    why = []
    if rec.get("upstream_error"):
        why.append("upstream-unreachable")
    if st is not None and st != 101 and not 200 <= st < 300:
        why.append("status-%s" % st)
    if rec.get("challenge"):
        why.append("challenge")
    if rec.get("error_type"):
        why.append(rec["error_type"])
    for e in rec.get("stream_errors", []):
        why.append("stream-" + e)
    if rec.get("shape") in ("sse", "ws") and st in (200, 101) and not rec.get("terminal"):
        why.append("client-closed" if rec.get("client_closed") else "cut")
    return ",".join(why) or None


# --- the boundary ------------------------------------------------------------------------------

class Conn:
    """A socket with a read buffer."""

    def __init__(self, sock):
        self.s, self.buf = sock, b""

    def more(self):
        d = self.s.recv(65536)
        if not d:
            raise EOFError
        self.buf += d

    def head(self, limit=256 * 1024):
        while b"\r\n\r\n" not in self.buf:
            if len(self.buf) > limit:
                raise ValueError("head too long")
            self.more()
        i = self.buf.index(b"\r\n\r\n") + 4
        h, self.buf = self.buf[:i], self.buf[i:]
        return h

    def some(self):
        if not self.buf:
            self.more()
        b, self.buf = self.buf, b""
        return b


def parse_head(raw):
    lines = raw.decode("latin-1").split("\r\n")
    first = lines[0]
    headers, dup = {}, []
    for line in lines[1:]:
        if not line or ":" not in line:
            continue
        k, v = line.split(":", 1)
        k, v = k.strip().lower(), v.strip()
        if k in headers:
            dup.append(k)
            headers[k] = headers[k] + ", " + v
        else:
            headers[k] = v
    return first, headers, dup


class Boundary:
    def __init__(self, a):
        self.out = a["dir"]
        os.makedirs(self.out, exist_ok=True)
        self.secrets = Secrets()
        self.lock = threading.Lock()
        self.seq = 0
        self.ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.ctx.load_cert_chain(a["cert"], a["key"])
        self.sni = {}

        def on_sni(sock, name, _ctx):
            self.sni[id(sock)] = name

        self.ctx.sni_callback = on_sni
        ca = a.get("upstream-ca") or ("/etc/ssl/cert.pem" if os.path.exists("/etc/ssl/cert.pem") else None)
        self.up_ctx = ssl.create_default_context(cafile=ca)
        self.up_at = {}
        for item in filter(None, (a.get("upstream-at") or "").split(",")):
            name, target = item.split("=")
            h, p = target.rsplit(":", 1)
            self.up_at[name] = (h, int(p))

    def write(self, name, obj):
        line = self.secrets.clean(json.dumps(obj, sort_keys=True))
        with self.lock:
            with open(os.path.join(self.out, name), "a") as f:
                f.write(line + "\n")

    def next_seq(self):
        with self.lock:
            self.seq += 1
            return self.seq

    def upstream(self, host):
        addr = self.up_at.get(host, (host, 443))
        raw = socket.create_connection(addr, timeout=60)
        raw.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        s = self.up_ctx.wrap_socket(raw, server_hostname=host)
        s.settimeout(None)
        return s

    def handle(self, sock, peer):
        cid = self.next_seq()
        t0 = now()
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        try:
            tls = self.ctx.wrap_socket(sock, server_side=True)
        except (ssl.SSLError, OSError) as e:
            self.write("conns.jsonl", {"conn": cid, "t": round(t0, 3), "handshake_refused": str(e)[:160]})
            sock.close()
            return
        sni = self.sni.pop(id(tls), None)
        self.write("conns.jsonl", {"conn": cid, "t": round(t0, 3), "opened": True, "sni": sni,
                                   "tls": tls.version()})
        c = Conn(tls)
        up, up_host, calls, why = None, None, 0, "closed-by-program"
        try:
            while True:
                try:
                    head = c.head()
                except EOFError:
                    break
                calls += 1
                first, headers, dup = parse_head(head)
                host = (headers.get("host") or sni or "").split(":")[0]
                if up is None or up_host != host:
                    if up is not None:
                        up.s.close()
                    try:
                        up, up_host = Conn(self.upstream(host)), host
                    except (OSError, ssl.SSLError) as e:
                        self.fail_call(cid, first, headers, host, str(e), tls)
                        why = "upstream-unreachable"
                        break
                keep = self.exchange(cid, c, up, first, head, headers, dup, host)
                if not keep:
                    why = "closed-after-call"
                    break
        except (OSError, ssl.SSLError, ValueError) as e:
            why = "error: %s" % str(e)[:120]
        finally:
            for s in (tls, up.s if up else None):
                try:
                    if s:
                        s.close()
                except OSError:
                    pass
            self.write("conns.jsonl", {"conn": cid, "t": round(now(), 3), "ended": why, "calls": calls,
                                       "seconds": round(now() - t0, 3)})

    def fail_call(self, cid, first, headers, host, err, tls):
        method, path = (first.split(" ") + ["", ""])[:2]
        rec = {"seq": self.next_seq(), "conn": cid, "t": round(now(), 3), "host": host, "method": method,
               "path": path.split("?")[0], "status": 502, "upstream_error": err[:160]}
        rec["abnormal"] = abnormal_of(rec)
        self.write("calls.jsonl", rec)
        msg = b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        try:
            tls.sendall(msg)
        except OSError:
            pass

    def exchange(self, cid, c, up, first, head, headers, dup, host):
        """One request and its reply. Returns whether the connection carries another."""
        t_start = now()
        method, path = (first.split(" ") + ["", ""])[:2]
        prov = provider_of(host, path)
        for k in SECRET_REQ:
            if headers.get(k):
                self.secrets.add(headers[k])
        if headers.get("cookie"):
            for part in headers["cookie"].split(";"):
                self.secrets.add(part.split("=", 1)[-1])
        rec = {"seq": self.next_seq(), "conn": cid, "t": round(t_start, 3), "host": host, "provider": prov,
               "method": method, "path": path.split("?")[0], "model_call": is_model_path(prov, method, path),
               "req_headers": {k: v[:200] for k, v in headers.items() if k in KEEP_REQ},
               "req_header_names": sorted(headers), "ids": ids_from_headers(headers)}
        if dup:
            rec["duplicate_headers"] = sorted(set(dup))
        # The request: forwarded as it came, its body read from a copy.
        up.s.sendall(head)
        upgrade = "websocket" in (headers.get("upgrade") or "").lower()
        body = b""
        if "chunked" in (headers.get("transfer-encoding") or "").lower():
            ch = Chunked()
            while not ch.done:
                b = c.some()
                n = ch.feed(b)
                up.s.sendall(b[:n])
                if n < len(b):
                    c.buf = b[n:] + c.buf
                if len(body) < MAX_COPY:
                    body += ch.take()
            rec["req_framing"] = "chunked" + (" malformed" if ch.malformed else "")
        elif headers.get("content-length"):
            left = int(headers["content-length"].split(",")[0])
            while left > 0:
                if not c.buf:
                    c.more()
                b, c.buf = c.buf[:left], c.buf[left:]
                up.s.sendall(b)
                left -= len(b)
                if len(body) < MAX_COPY:
                    body += b
        rec["req_bytes"] = len(head) + len(body)
        if body:
            dec = Decoder(headers.get("content-encoding"))
            plain = dec.feed(body)
            if dec.unreadable:
                rec["req_unreadable"] = dec.unreadable
            else:
                rec["req_body_bytes"] = len(plain)
                try:
                    read_request_body(rec, prov, json.loads(plain), self.secrets)
                except ValueError:
                    rec["req_body"] = "not-json"
        # The reply: its head, then its body, forwarded as each piece arrives.
        while True:
            try:
                rhead = up.head()
            except (EOFError, OSError, ValueError) as e:
                rec.update(status=502, upstream_error="no reply: %s" % (str(e)[:120] or "closed"),
                           ms=round((now() - t_start) * 1000, 1))
                rec["abnormal"] = abnormal_of(rec)
                self.write("calls.jsonl", rec)
                try:
                    c.s.sendall(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                except OSError:
                    pass
                return False
            c.s.sendall(rhead)
            status_line, rheaders, _ = parse_head(rhead)
            try:
                status = int(status_line.split(" ")[1])
            except (IndexError, ValueError):
                status = 0
            if 100 <= status < 200 and status != 101:
                continue
            break
        t_first = now()
        for k in ("set-cookie",):
            if rheaders.get(k):
                for part in rheaders[k].split(", "):
                    self.secrets.add(part.split(";")[0].split("=", 1)[-1])
        rec.update(status=status, ttfb_ms=round((t_first - t_start) * 1000, 1),
                   resp_headers=kept_response_headers(rheaders))
        ctype = (rheaders.get("content-type") or "").lower()
        if status in (403, 429, 503) and ("text/html" in ctype or rheaders.get("cf-mitigated")):
            rec["challenge"] = True
        if status == 101 and upgrade:
            return self.websocket(rec, c, up, headers, rheaders, t_start)
        reply = Reply(rec)
        dec = Decoder(rheaders.get("content-encoding"))
        sse = SSE(reply.event) if "text/event-stream" in ctype else None
        rec["shape"] = "sse" if sse else ("json" if "json" in ctype else (ctype.split(";")[0] or "empty"))
        copy = []
        got = 0

        def take(b):
            plain = dec.feed(b)
            if sse is not None:
                sse.feed(plain.decode("utf-8", "replace"))
            elif sum(len(x) for x in copy) < 4 * 1024 * 1024:
                copy.append(plain)

        client_closed, ended_by_close, rch = False, False, None
        if method == "HEAD" or status in (204, 304):
            pass
        elif "chunked" in (rheaders.get("transfer-encoding") or "").lower():
            rch = Chunked()
            while not rch.done:
                try:
                    b = up.some()
                except (EOFError, OSError):
                    ended_by_close = True
                    break
                n = rch.feed(b)
                got += n
                take(rch.take())
                try:
                    c.s.sendall(b[:n])
                except OSError:
                    client_closed = True
                    break
                if n < len(b):
                    up.buf = b[n:] + up.buf
        elif rheaders.get("content-length"):
            left = int(rheaders["content-length"].split(",")[0])
            while left > 0:
                try:
                    if not up.buf:
                        up.more()
                except (EOFError, OSError):
                    ended_by_close = True
                    break
                b, up.buf = up.buf[:left], up.buf[left:]
                left -= len(b)
                got += len(b)
                take(b)
                try:
                    c.s.sendall(b)
                except OSError:
                    client_closed = True
                    break
            if left > 0 and not client_closed:
                rec["short_body"] = left
        else:
            ended_by_close = True
            while True:
                try:
                    b = up.some()
                except (EOFError, OSError):
                    break
                got += len(b)
                take(b)
                try:
                    c.s.sendall(b)
                except OSError:
                    client_closed = True
                    break
        t_end = now()
        rec.update(resp_bytes=len(rhead) + got, ms=round((t_end - t_start) * 1000, 1))
        if dec.unreadable:
            rec["resp_unreadable"] = dec.unreadable
        if client_closed:
            rec["client_closed"] = True
        if sse is not None:
            rec["sse_events"] = sse.n
        elif copy:
            try:
                o = json.loads(b"".join(copy))
                self.secrets.add_json(o)
                if isinstance(o, dict) and isinstance(o.get("error"), dict):
                    rec["error_type"] = str(o["error"].get("type") or o["error"].get("code") or "error")[:60]
                elif isinstance(o, dict) and o.get("type") == "error":
                    rec["error_type"] = "error"
                if isinstance(o, dict) and o.get("type") == "message":
                    rec["response_id"], rec["model_answered"] = o.get("id"), o.get("model")
            except ValueError:
                pass
        if ch_cut(rheaders, ended_by_close, rch) and not client_closed:
            rec["short_body"] = rec.get("short_body", 0) or 1
        if rec.get("short_body") and not rec.get("client_closed"):
            rec["stream_errors"] = rec.get("stream_errors", []) + ["body-cut"]
        rec["abnormal"] = abnormal_of(rec)
        self.write("calls.jsonl", rec)
        conn_close = "close" in (headers.get("connection") or "").lower() or "close" in (rheaders.get("connection") or "").lower()
        return not (client_closed or ended_by_close or conn_close)

    def websocket(self, rec, c, up, headers, rheaders, t_start):
        """After a 101: both ways byte for byte, each message read from a copy. One calls.jsonl
        line for the upgrade and one for each request message and the events that answer it."""
        ext = (rheaders.get("sec-websocket-extensions") or "").lower()
        deflate = "permessage-deflate" in ext
        rec.update(shape="ws-upgrade", ws_deflate=deflate, ms=round((now() - t_start) * 1000, 1))
        rec["abnormal"] = abnormal_of(rec)
        self.write("calls.jsonl", rec)
        base_ids = dict(rec["ids"])
        lock = threading.Lock()
        state = {"pending": [], "n": 0, "rate": None}

        def finish(call, why=None):
            call["ms"] = round((now() - call["_t"]) * 1000, 1)
            if why == "client":
                call["client_closed"] = True
            del call["_t"]
            call["abnormal"] = abnormal_of(call)
            self.write("calls.jsonl", call)

        def from_client(op, data):
            if op != 1:
                return
            try:
                obj = json.loads(data)
            except ValueError:
                return
            if not isinstance(obj, dict):
                return
            with lock:
                state["n"] += 1
                call = {"seq": self.next_seq(), "conn": rec["conn"], "t": round(now(), 3), "_t": now(),
                        "host": rec["host"], "provider": rec["provider"], "method": "WS", "path": rec["path"],
                        "ws_message": state["n"], "ws_type": str(obj.get("type"))[:40], "shape": "ws",
                        "status": 101, "ids": dict(base_ids), "req_bytes": len(data),
                        "model_call": obj.get("type") == "response.create", "events": 0}
                read_request_body(call, rec["provider"], obj, self.secrets)
                if call["model_call"]:
                    state["pending"].append(call)
                else:
                    call["shape"] = "ws-other"
                    finish(call)

        def from_server(op, data):
            if op != 1:
                return
            try:
                obj = json.loads(data)
            except ValueError:
                return
            if not isinstance(obj, dict):
                return
            with lock:
                if not state["pending"]:
                    if "rate_limit" in str(obj.get("type")):
                        state["rate"] = json.dumps(obj, sort_keys=True)[:600]
                    return
                call = state["pending"][0]
                if state["rate"] and "rate_limit_event" not in call:
                    call["rate_limit_event"], state["rate"] = state["rate"], None
                call["resp_bytes"] = call.get("resp_bytes", 0) + len(data)
                if call["events"] == 0:
                    call["ttfb_ms"] = round((now() - call["_t"]) * 1000, 1)
                Reply(call).event(obj.get("type"), obj)
                if call.get("terminal") or call.get("stream_errors"):
                    finish(state["pending"].pop(0))

        no_takeover_c = "client_no_context_takeover" in ext
        no_takeover_s = "server_no_context_takeover" in ext
        rc = WSReader(deflate, no_takeover_c, from_client)
        rs = WSReader(deflate, no_takeover_s, from_server)
        done = {"client": None, "server": None}

        def pump(src, dst, reader, name):
            try:
                if src.buf:
                    b, src.buf = src.buf, b""
                    dst.s.sendall(b)
                    reader.feed(b)
                while True:
                    b = src.s.recv(65536)
                    if not b:
                        break
                    dst.s.sendall(b)
                    reader.feed(b)
            except (OSError, ssl.SSLError):
                pass
            done[name] = now()
            try:
                dst.s.shutdown(socket.SHUT_WR)
            except OSError:
                pass

        t = threading.Thread(target=pump, args=(c, up, rc, "client"), daemon=True)
        t.start()
        pump(up, c, rs, "server")
        t.join(5)
        with lock:
            closer = "client" if (done["client"] and (not done["server"] or done["client"] <= done["server"])) else None
            for call in state["pending"]:
                finish(call, closer or "cut")
            state["pending"] = []
        return False


def serve(a):
    b = Boundary(a)
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", int(a.get("port") or 0)))
    srv.listen(128)
    with open(a["port-file"], "w") as f:
        f.write(str(srv.getsockname()[1]))
    limit = float(a.get("max-seconds") or 0)
    t_end = now() + limit if limit else None
    req = os.path.join(b.out, "scan-request")

    def watch():
        while True:
            time.sleep(1)
            if os.path.exists(req):
                lines = [x for x in open(req).read().split("\n") if x.strip()]
                with b.secrets.lock:
                    vals = list(b.secrets.values)
                res = find_in_dirs(vals, [x for x in lines if not x.startswith("-")],
                                   [x[1:] for x in lines if x.startswith("-")])
                res["redactions"] = b.secrets.redactions
                with open(os.path.join(b.out, "scan.json.part"), "w") as f:
                    json.dump(res, f)
                os.rename(os.path.join(b.out, "scan.json.part"), os.path.join(b.out, "scan.json"))
                os.unlink(req)
            if t_end and now() > t_end:
                os._exit(0)

    threading.Thread(target=watch, daemon=True).start()
    while True:
        s, peer = srv.accept()
        threading.Thread(target=b.handle, args=(s, peer), daemon=True).start()


# --- the analysis ------------------------------------------------------------------------------

def load(path):
    out = []
    try:
        for line in open(path):
            if line.strip():
                try:
                    out.append(json.loads(line))
                except ValueError:
                    pass
    except OSError:
        pass
    return out


def launches(guest, phase, prog):
    """The guest driver's launches.tsv and what each launch's own output says."""
    d = os.path.join(guest, phase, prog)
    rows = []
    try:
        lines = open(os.path.join(d, "launches.tsv")).read().splitlines()
    except OSError:
        return rows
    for line in lines:
        f = line.split("\t")
        if len(f) < 6:
            continue
        r = {"n": int(f[0]), "start": int(f[1]), "end": int(f[2]), "rc": int(f[3]), "id": f[4], "kind": f[5],
             "errors": 0, "retries": 0, "is_error": False, "events": 0}
        for e in load(os.path.join(d, "launch-%s.jsonl" % f[0])):
            r["events"] += 1
            t, st = str(e.get("type")), str(e.get("subtype") or "")
            text = json.dumps(e)[:2000].lower()
            if prog == "claude":
                if t == "system" and re.search(r"retry", st):
                    r["retries"] += 1
                elif t == "system" and re.search(r"error", st):
                    r["errors"] += 1
                elif t == "result" and (e.get("is_error") or st.startswith("error")):
                    r["is_error"] = True
                    r["errors"] += 1
                elif t == "assistant" and "api error" in text:
                    r["errors"] += 1
            else:
                if t == "error":
                    msg = str(e.get("message") or "").lower()
                    if "reconnect" in msg or "retry" in msg or "retrying" in msg:
                        r["retries"] += 1
                    else:
                        r["errors"] += 1
                elif t in ("turn.failed", "thread.error"):
                    r["is_error"] = True
                    r["errors"] += 1
        rows.append(r)
    return rows


def phase_numbers(rows, hours):
    """A phase's launches, counted against the hours it was given."""
    if not rows:
        return None
    hours = max(1e-6, float(hours))
    failed = [r for r in rows if r["rc"] not in (0, 124) or r["is_error"]]
    return {"launches": len(rows), "failed": len(failed), "errors": sum(r["errors"] for r in rows),
            "retries": sum(r["retries"] for r in rows), "hours": round(hours, 4),
            "per_hour": round((sum(r["errors"] + r["retries"] for r in rows)) / hours, 2),
            "failed_share": round(len(failed) / float(len(rows)), 3),
            "ids": sorted(set(r["id"] for r in rows if r["id"])),
            "time_limited": len([r for r in rows if r["rc"] == 124])}


def analyze(a):
    until = float(a.get("until") or 0) or float("inf")
    calls = [c for c in load(a["calls"]) if c.get("t", 0) <= until]
    conns = [c for c in load(a["conns"]) if c.get("t", 0) <= until]
    guest = a["guest"]
    facts = []
    fact = lambda k, v: facts.append((k, v if isinstance(v, str) else json.dumps(v, sort_keys=True)))
    verdict = {}

    # Who called what.
    by_prog = {"claude": [c for c in calls if c.get("provider") == "anthropic"],
               "codex": [c for c in calls if c.get("provider") == "openai"]}
    hs = [c for c in conns if "handshake_refused" in c]
    fact("calls_through_boundary", "%d calls (%d model calls) on %d connections; %d handshakes refused" % (
        len(calls), len([c for c in calls if c.get("model_call")]), len([c for c in conns if c.get("opened")]), len(hs)))
    verdict["handshakes_refused"] = len(hs)
    if hs:
        fact("handshakes_refused_first", hs[0]["handshake_refused"])

    # Conversations: every model call tied to one id, the ids agreeing inside a call, and each
    # conversation the program itself names (its launches) found under that one id.
    split_reasons = []
    for prog, cs in sorted(by_prog.items()):
        model = [c for c in cs if c.get("model_call")]
        if not model:
            fact("%s.calls" % prog, "no model call through the boundary")
            continue
        paths = sorted(set("%s %s%s" % (c["method"], c["host"], c["path"]) for c in cs))
        fact("%s.paths" % prog, ", ".join(paths)[:900])
        ids_seen, orphans, side_orphans, disagree = {}, [], [], []
        for c in model:
            ids = c.get("ids", {})
            if prog == "claude":
                cand = [ids.get("claude_session"), ids.get("claude_session_meta")]
            else:
                cand = [ids.get("codex_thread"), ids.get("codex_session"), ids.get("codex_conversation"),
                        ids.get("prompt_cache_key")]
            cand = [x for x in cand if x]
            if not cand:
                (orphans if c.get("tools") or c.get("ws_message") else side_orphans).append(c)
                continue
            if len(set(cand)) > 1:
                disagree.append(c)
            ids_seen.setdefault(cand[0], []).append(c)
        own = phase_numbers(launches(guest, "night", prog), a.get("night-hours") or 1) or {"ids": []}
        own_ids = own["ids"]
        unknown = sorted(i for i in ids_seen if i not in own_ids)
        unseen = sorted(i for i in own_ids if i not in ids_seen)
        tools_unknown = sorted(i for i in unknown if any(c.get("tools") or c.get("ws_message") for c in ids_seen[i]))
        fact("%s.conversations" % prog, "model calls %d; ids in traffic %d (%s); the program's own ids %d; "
             "calls with no id %d (side calls %d); ids disagreeing in one call %d; the program's ids not seen %d; "
             "ids with tools the program never named %d" % (
                 len(model), len(ids_seen), ", ".join("%s:%d" % (k[:13], len(v)) for k, v in sorted(ids_seen.items()))[:300],
                 len(own_ids), len(orphans), len(side_orphans), len(disagree), len(unseen), len(tools_unknown)))
        if prog == "claude":
            agents = set(c["ids"].get("claude_agent") for c in model if c.get("ids", {}).get("claude_agent"))
            compactions = [c for c in model if c.get("messages") is not None and c.get("messages") <= 2 and c.get("tools")]
            fact("claude.subagents_and_short_histories", "subagent ids %d; tool calls with at most 2 messages "
                 "(a start, or after a compaction) %d" % (len(agents), len(compactions)))
        else:
            ws = [c for c in model if c.get("ws_message")]
            http = [c for c in model if not c.get("ws_message")]
            ups = [c for c in cs if c.get("shape") == "ws-upgrade"]
            ws_refused = [c for c in cs if "websocket" in (c.get("req_headers", {}).get("upgrade") or "").lower()
                          and c.get("status") != 101]
            chained = [c for c in ws if c.get("ids", {}).get("previous_response_id")]
            windows = sorted(set(c["ids"]["codex_window"] for c in model if c.get("ids", {}).get("codex_window")))
            fact("codex.transport", "WebSocket upgrades %d (refused %d, compressed %d); model messages on a socket %d "
                 "(naming the previous reply %d, warm-ups %d); model requests over HTTP %d; windows %d; compactions %d" % (
                     len(ups), len(ws_refused), len([c for c in ups if c.get("ws_deflate")]), len(ws), len(chained),
                     len([c for c in ws if c.get("warmup")]), len(http), len(windows),
                     len([c for c in model if c.get("compaction")])))
        if orphans:
            split_reasons.append("%s: %d model calls with tools carried no id" % (prog, len(orphans)))
        if disagree:
            split_reasons.append("%s: %d calls named two different ids" % (prog, len(disagree)))
        if tools_unknown and own_ids:
            split_reasons.append("%s: %d ids in the traffic that the program never named" % (prog, len(tools_unknown)))
        if unseen:
            split_reasons.append("%s: %d of the program's conversations never seen in the traffic" % (prog, len(unseen)))
        if len(set(own_ids)) > 1:
            fact("%s.own_ids_changed" % prog, "the program named %d ids for what the test resumed as one conversation" % len(set(own_ids)))
    verdict["split"] = split_reasons

    # The plan's headers.
    missing = []
    a_model = [c for c in by_prog["claude"] if c.get("model_call") and c.get("status")]
    if a_model:
        names = set()
        for c in a_model:
            names.update(k for k in c.get("resp_headers", {}) if k.startswith("anthropic-ratelimit-"))
        unified = sorted(n for n in names if n.startswith("anthropic-ratelimit-unified-"))
        with_h = len([c for c in a_model if any(k.startswith("anthropic-ratelimit-unified-") for k in c.get("resp_headers", {}))])
        fact("claude.plan_headers", "%d of %d model replies carried anthropic-ratelimit-unified-*; names: %s" % (
            with_h, len(a_model), ", ".join(unified)[:700] or "none"))
        last = [c for c in a_model if any(k.startswith("anthropic-ratelimit-unified-") for k in c.get("resp_headers", {}))]
        if last:
            fact("claude.plan_headers_last", json.dumps({k: v for k, v in last[-1]["resp_headers"].items()
                                                         if k.startswith("anthropic-ratelimit-")}, sort_keys=True)[:900])
        if not unified:
            missing.append("Claude Code's replies carried no anthropic-ratelimit-unified-* header")
    o_all = by_prog["codex"]
    if [c for c in o_all if c.get("model_call")]:
        names = set()
        for c in o_all:
            names.update(k for k in c.get("resp_headers", {}) if k.startswith(("x-codex-", "x-ratelimit-")))
        events = [c for c in o_all if c.get("rate_limit_event")]
        fact("codex.plan_headers", "headers: %s; rate-limit events on the socket: %d" % (", ".join(sorted(names))[:700] or "none", len(events)))
        if events:
            fact("codex.rate_limit_event_last", events[-1]["rate_limit_event"])
        if not names and not events:
            missing.append("Codex's replies carried no x-codex-* or x-ratelimit-* header and no rate-limit event")
    verdict["headers_missing"] = missing

    # Every reply that is not a normal one, each a fact (the first 150; all are in calls.jsonl).
    ab = [c for c in calls if c.get("abnormal")]
    kinds = {}
    for c in ab:
        for k in c["abnormal"].split(","):
            key = "%s %s" % (c.get("provider"), k)
            kinds[key] = kinds.get(key, 0) + 1
    fact("not_normal_replies", "%d of %d: %s" % (len(ab), len(calls), ", ".join("%s x%d" % kv for kv in sorted(kinds.items())) or "none"))
    for c in ab[:150]:
        rh = c.get("resp_headers", {})
        fact("not_normal.%d" % c["seq"], "%s %s %s%s status %s: %s; %s ms; retry-count %s; retry-after %s; request-id %s%s" % (
            time.strftime("%H:%M:%S", time.localtime(c.get("t", 0))), c.get("method"), c.get("host"), c.get("path"),
            c.get("status"), c["abnormal"], c.get("ms"), c.get("req_headers", {}).get("x-stainless-retry-count", "-"),
            rh.get("retry-after", "-"), rh.get("request-id") or rh.get("x-request-id") or "-",
            "; unified-status %s" % rh["anthropic-ratelimit-unified-status"] if rh.get("anthropic-ratelimit-unified-status") else ""))
    if len(ab) > 150:
        fact("not_normal_more", "%d more, in calls.jsonl" % (len(ab) - 150))
    provider_kinds = [k for k in kinds if not k.endswith("client-closed")]
    verdict["not_normal"] = len(ab)
    verdict["challenges"] = len([c for c in calls if c.get("challenge")])
    retried = [c for c in calls if c.get("req_headers", {}).get("x-stainless-retry-count") not in (None, "0")]
    fact("retries_seen_in_requests", "%d requests carried x-stainless-retry-count above 0" % len(retried))

    # Timing and sizes of the model calls.
    for prog, cs in sorted(by_prog.items()):
        m = sorted(c["ttfb_ms"] for c in cs if c.get("model_call") and c.get("ttfb_ms") is not None)
        if m:
            sz = sorted(c.get("req_bytes", 0) for c in cs if c.get("model_call"))
            fact("%s.model_call_timing" % prog, "first byte median %.0f ms, p95 %.0f ms; request median %d B, max %d B; "
                 "compressed requests %d; unreadable bodies %d" % (
                     m[len(m) // 2], m[min(len(m) - 1, int(len(m) * 0.95))], sz[len(sz) // 2], sz[-1],
                     len([c for c in cs if c.get("req_headers", {}).get("content-encoding")]),
                     len([c for c in cs if c.get("req_unreadable") or c.get("resp_unreadable")])))

    # The night against the control: the programs' own account of errors and retries.
    differs = []
    for prog in ("claude", "codex"):
        n = phase_numbers(launches(guest, "night", prog), a.get("night-hours") or 1)
        c = phase_numbers(launches(guest, "control", prog), a.get("control-hours") or 1)
        if not n:
            continue
        fact("%s.night" % prog, {k: v for k, v in n.items() if k != "ids"})
        if not c:
            fact("%s.control" % prog, "no control launch")
            continue
        fact("%s.control" % prog, {k: v for k, v in c.items() if k != "ids"})
        if n["per_hour"] > 2 * c["per_hour"] + 2:
            differs.append("%s: %.1f errors and retries an hour through the boundary, %.1f without" % (prog, n["per_hour"], c["per_hour"]))
        if n["failed_share"] > c["failed_share"] + 0.25:
            differs.append("%s: %.0f%% of launches failed through the boundary, %.0f%% without" % (prog, 100 * n["failed_share"], 100 * c["failed_share"]))
    if verdict["challenges"]:
        differs.append("%d replies were challenges (HTML or cf-mitigated)" % verdict["challenges"])
    if hs:
        differs.append("%d handshakes with the boundary were refused" % len(hs))
    verdict["differs"] = differs
    verdict["provider_kinds"] = provider_kinds
    verdict["model_calls"] = len([c for c in calls if c.get("model_call")])
    verdict["control_ran"] = sorted(p for p in ("claude", "codex") if launches(guest, "control", p))
    verdict["night_ran"] = sorted(p for p in ("claude", "codex") if launches(guest, "night", p))
    verdict["summary"] = "model calls through the boundary: Claude Code %d, Codex %d; not normal replies: %s; " \
        "conversations: %s; plan headers: %s; against the control: %s" % (
            len([c for c in by_prog["claude"] if c.get("model_call")]), len([c for c in by_prog["codex"] if c.get("model_call")]),
            ", ".join("%s x%d" % kv for kv in sorted(kinds.items())) or "none",
            "; ".join(split_reasons) or "whole", "; ".join(missing) or "read",
            "; ".join(differs) or "no difference")
    for k, v in facts:
        print("%s\t%s" % (k, v.replace("\n", " ")))
    print("verdict\t%s" % json.dumps(verdict, sort_keys=True))


def scan(dirs):
    """Credential files on stdin; prints which files under dirs hold any of their values."""
    s = Secrets()
    text = sys.stdin.read()
    for doc in text.split("---silokit---"):
        doc = doc.strip()
        if not doc:
            continue
        try:
            o = json.loads(doc)
        except ValueError:
            s.add(doc)
            continue
        stack = [o]
        while stack:
            x = stack.pop()
            if isinstance(x, dict):
                stack.extend(x.values())
            elif isinstance(x, list):
                stack.extend(x)
            elif isinstance(x, str) and len(x) >= 20 and " " not in x:
                s.add(x)
    res = find_in_dirs(sorted(s.values), [d for d in dirs if not d.startswith("-")],
                       [d[1:] for d in dirs if d.startswith("-")])
    print(json.dumps(res))


def parse(argv):
    out, i = {}, 0
    while i < len(argv):
        out[argv[i].lstrip("-")] = argv[i + 1] if i + 1 < len(argv) else ""
        i += 2
    return out


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    cmd = sys.argv[1]
    if cmd == "serve":
        serve(parse(sys.argv[2:]))
    elif cmd == "analyze":
        analyze(parse(sys.argv[2:]))
    elif cmd == "scan":
        scan(sys.argv[2:])
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
