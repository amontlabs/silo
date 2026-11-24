"""A stand-in for Silo's boundary, for credential slots only. Throwaway code.

A forward HTTP proxy (absolute-form requests and CONNECT). Inside CONNECT it ends TLS with a
certificate for the name, or reads plain HTTP if the first byte is not a TLS handshake. For each
request it:
  - swaps a secret's placeholder for the real value only in that secret's slots, on its hosts;
  - re-signs AWS SigV4 requests made with a placeholder secret key;
  - forwards every other byte as it came (bodies are copied raw, never re-encoded);
  - writes facts to a record (JSON lines) that holds placeholders and never a real value;
  - scrubs the real value, in its plain form, its base64 forms and runs of 12 or more of its
    characters, out of reply headers and bodies.

Simplifications: HTTP/1.1 only; one upstream connection per request; a compressed reply from a
host that holds a secret is read whole before it is forwarded (the doc describes the streaming
design); names resolve through the config's "hosts" map, never DNS.

Real values come from environment variables named in the config, never from the config file.
"""
import base64, gzip, hashlib, json, os, re, socket, ssl, sys, threading, time, zlib
from urllib.parse import unquote
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sigv4

CFG = json.load(open(sys.argv[1]))
RECORD = open(CFG["record"], "a")
LOCK = threading.Lock()
PARTIAL_RUN = CFG.get("partial_run", 12)
TOKEN = re.compile(rb"[!#$%&'*+\-.^_`|~0-9A-Za-z]+")


def fact(**kw):
    kw["t"] = round(time.time(), 3)
    line = json.dumps(kw)
    for s in SECRETS:  # the record never holds a real value: a last check, and loud
        for real in s["reals"]:
            if real in line:
                raise SystemExit("BUG: a real value reached the record")
    with LOCK:
        RECORD.write(line + "\n"); RECORD.flush()


# secrets -------------------------------------------------------------------------------------

SECRETS = []
for s in CFG["secrets"]:
    s = dict(s)
    s["real"] = os.environ[s["value_env"]]
    if s.get("kind") == "sigv4":
        s["real_akid"] = os.environ.get(s.get("akid_env", ""), s["placeholder_akid"])
    # A real value goes into a header: refuse one that could break the header (CR, LF, NUL, non-ASCII).
    if not re.fullmatch(r"[\x21-\x7e]+", s["real"]):
        raise SystemExit(f"secret {s['name']}: value is not visible ASCII")
    if len(s["real"]) != len(s["placeholder"]):
        raise SystemExit(f"secret {s['name']}: placeholder and value must have the same length")
    s["reals"] = [s["real"]]
    SECRETS.append(s)


def b64_forms(real, ph):
    """base64 of the value at the three byte alignments, cut to the characters that depend only on
    the value. Same length real and placeholder give same length forms."""
    out = []
    for pad in range(3):
        a = base64.b64encode(b"\0" * pad + real.encode())
        b = base64.b64encode(b"\0" * pad + ph.encode())
        start = {0: 0, 1: 2, 2: 3}[pad]  # first char that holds no padding bits
        tail = len(real) + pad
        stop = (tail // 3) * 4  # drop the last, partial group
        out.append((a[start:stop], b[start:stop]))
    return out


SCRUB_FORMS = []  # (real bytes, placeholder bytes, partial runs allowed?)
for s in SECRETS:
    SCRUB_FORMS.append((s["real"].encode(), s["placeholder"].encode(), True, s["name"], "plain"))
    for i, (a, b) in enumerate(b64_forms(s["real"], s["placeholder"])):
        SCRUB_FORMS.append((a, b, False, s["name"], f"base64/{i}"))
MAXLEN = max(len(f[0]) for f in SCRUB_FORMS)


def find_hits(buf):
    """Every place a form of a real value appears. Returns [(start, end, replacement, name, form)]."""
    hits = []
    for real, ph, partial, name, form in SCRUB_FORMS:
        if not partial:
            i = buf.find(real)
            while i >= 0:
                hits.append((i, i + len(real), ph, name, form)); i = buf.find(real, i + 1)
            continue
        k = min(PARTIAL_RUN, len(real))
        grams = {}
        for j in range(len(real) - k + 1):
            grams.setdefault(real[j:j + k], j)
        i = 0
        while i <= len(buf) - k:
            j = grams.get(bytes(buf[i:i + k]))
            if j is None:
                i += 1; continue
            m = k
            while i + m < len(buf) and j + m < len(real) and buf[i + m] == real[j + m]:
                m += 1
            hits.append((i, i + m, ph[j:j + m], name, "plain" if m == len(real) else f"run of {m}"))
            i += m
    hits.sort()
    merged, last = [], -1
    for h in hits:
        if h[0] >= last:
            merged.append(h); last = h[1]
    return merged


class Scrubber:
    """Streaming: holds back the last MAXLEN-1 bytes, so a value split across reads is still seen.
    Replacements have the same length, so lengths and chunk sizes stay valid."""

    def __init__(self, where):
        self.buf = bytearray(); self.where = where; self.hits = []

    def feed(self, data, final=False):
        self.buf += data
        hits = find_hits(self.buf)
        cut = len(self.buf) if final else max(0, len(self.buf) - (MAXLEN - 1))
        for h in hits:
            if h[1] > cut and not final:   # may grow with the next read: hold it back
                cut = min(cut, h[0]); break
        for start, end, rep, name, form in hits:
            if end <= cut:
                self.buf[start:end] = rep
                self.hits.append({"secret": name, "form": form, "at": self.where})
        out = bytes(self.buf[:cut]); del self.buf[:cut]
        return out


def scrub_bytes(data, where):
    s = Scrubber(where); out = s.feed(data, final=True); return out, s.hits


# HTTP parsing --------------------------------------------------------------------------------

class Bad(Exception):
    pass


def read_head(rf):
    lines = []
    total = 0
    while True:
        line = rf.readline(65537)
        if not line:
            if not lines:
                return None
            raise Bad("connection closed inside a head")
        total += len(line)
        if total > 65536:
            raise Bad("head too large")
        if not line.endswith(b"\r\n") or b"\r" in line[:-2] or b"\0" in line:
            raise Bad("a line not ended by CRLF, or a bare CR or NUL in it")
        if line == b"\r\n":
            if not lines:
                continue
            return lines
        lines.append(line[:-2])


def parse_headers(lines):
    hs = []
    for l in lines:
        if l[:1] in (b" ", b"\t"):
            raise Bad("obs-fold line")  # RFC 9112 5.2: a proxy may reject it; Silo does
        name, sep, value = l.partition(b":")
        if not sep or not TOKEN.fullmatch(name):
            raise Bad("bad header name")
        hs.append([name.decode(), value.strip(b" \t").decode("latin-1")])
    return hs


def get(hs, name):
    return [v for n, v in hs if n.lower() == name]


def body_iter(rf, hs, is_response=False, method=None, status=None):
    """Yields the body's raw bytes as they came, framing included."""
    if is_response and (method == "HEAD" or status in (204, 304) or 100 <= status < 200):
        return
    te = ",".join(get(hs, "transfer-encoding")).lower()
    cl = get(hs, "content-length")
    if "chunked" in te:
        if cl:
            raise Bad("both Transfer-Encoding and Content-Length")  # request smuggling shape
        while True:
            line = rf.readline(4096)
            if not line.endswith(b"\r\n"):
                raise Bad("bad chunk line")
            yield line
            size = int(line.split(b";")[0], 16)
            if size == 0:
                while True:
                    t = rf.readline(8192); yield t
                    if t == b"\r\n" or not t:
                        return
            left = size + 2
            while left:
                d = rf.read(min(left, 65536))
                if not d:
                    raise Bad("closed inside a chunk")
                left -= len(d); yield d
    elif cl:
        if len(set(cl)) != 1:
            raise Bad("two different Content-Length values")
        left = int(cl[0])
        while left:
            d = rf.read(min(left, 65536))
            if not d:
                raise Bad("closed inside a body")
            left -= len(d); yield d
    elif is_response:
        while True:
            d = rf.read1(65536) if hasattr(rf, "read1") else rf.read(65536)
            if not d:
                return
            yield d


def dechunk(raw):
    out, i = bytearray(), 0
    while True:
        eol = raw.index(b"\r\n", i); size = int(raw[i:eol].split(b";")[0], 16)
        if size == 0:
            return bytes(out)
        out += raw[eol + 2:eol + 2 + size]; i = eol + 2 + size + 2


def decoded_copy(raw, hs):
    """For reading only: the body without chunking and compression. Never forwarded."""
    try:
        if "chunked" in ",".join(get(hs, "transfer-encoding")).lower():
            raw = dechunk(raw)
        enc = ",".join(get(hs, "content-encoding")).lower().strip()
        if enc == "gzip":
            raw = gzip.decompress(raw)
        elif enc == "deflate":
            raw = zlib.decompress(raw)
        elif enc not in ("", "identity", "aws-chunked"):
            return None
        return raw
    except Exception:
        return None


# the swap ------------------------------------------------------------------------------------

def host_of(authority):
    h = authority.strip().lower()
    if h.startswith("["):
        return h
    return h.rsplit(":", 1)[0] if ":" in h else h


def swap_header(name, value, host):
    """Returns (new_value, secret_name, slot) or None. Exact slot forms only."""
    lname = name.lower()
    for s in SECRETS:
        if host not in s["hosts"] or s.get("kind") == "sigv4":
            continue
        ph = s["placeholder"]
        for slot in s["slots"]:
            if slot.get("header") != lname:
                continue
            scheme = slot.get("scheme")
            if scheme is None and value == ph:
                return s["real"], s["name"], lname
            if scheme in ("bearer", "token"):
                m = re.fullmatch(r"(?i)(%s) +(\S+)" % scheme, value)
                if m and m.group(2) == ph:
                    return m.group(1) + " " + s["real"], s["name"], f"{lname}: {scheme}"
            if scheme == "basic-password":
                m = re.fullmatch(r"(?i)(basic) +([A-Za-z0-9+/=]+)", value)
                if m:
                    try:
                        user, sep, pw = base64.b64decode(m.group(2), validate=True).decode().partition(":")
                    except Exception:
                        continue
                    if sep and pw == ph and (slot.get("user") in (None, user)):
                        return m.group(1) + " " + base64.b64encode(f"{user}:{s['real']}".encode()).decode(), s["name"], f"{lname}: basic password"
            if scheme == "cookie":
                parts = value.split("; ")
                for i, p in enumerate(parts):
                    k, _, v = p.partition("=")
                    if k == slot["cookie"] and v == ph:
                        parts[i] = k + "=" + s["real"]
                        return "; ".join(parts), s["name"], f"cookie {k}"
    return None


def swap_query(target, host):
    path, q, query = target.partition("?")
    if not q:
        return target, []
    parts = query.split("&"); done = []
    for s in SECRETS:
        if host not in s["hosts"]:
            continue
        for slot in s["slots"]:
            if "query" not in slot:
                continue
            hits = [i for i, p in enumerate(parts) if p.partition("=")[0] == slot["query"] and unquote(p.partition("=")[2]) == s["placeholder"]]
            if len(hits) == 1:
                parts[hits[0]] = slot["query"] + "=" + s["real"]
                done.append((s["name"], "query " + slot["query"]))
    return path + "?" + "&".join(parts), done


def sightings(blob, where):
    for s in SECRETS:
        for ph in [s["placeholder"]] + ([s["placeholder_akid"]] if s.get("kind") == "sigv4" else []):
            if ph.encode() in blob:
                yield {"secret": s["name"], "where": where}


def sigv4_resign(method, target, hs, body, host):
    """Returns (new_target, new_headers, new_body, facts) or None if not a SigV4 request of ours."""
    path, _, query = target.partition("?")
    for s in SECRETS:
        if s.get("kind") != "sigv4" or host not in s["hosts"]:
            continue
        auth_i = [i for i, (n, v) in enumerate(hs) if n.lower() == "authorization"]
        if "Signature=" in query and f"AWSAccessKeyId={s['placeholder_akid']}" in query:
            from urllib.parse import quote
            old, q = sigv4.v2_query_signature(s["placeholder"], method, path, query)
            valid = old == q["Signature"]
            new, _ = sigv4.v2_query_signature(s["real"], method, path, query)
            parts = []
            for p in query.split("&"):
                k = p.partition("=")[0]
                parts.append("AWSAccessKeyId=" + s["real_akid"] if k == "AWSAccessKeyId" else "Signature=" + quote(new, safe="") if k == "Signature" else p)
            return path + "?" + "&".join(parts), hs, body, [{"secret": s["name"], "slot": "sigv2 presigned query", "placeholder_signature_valid": valid}]
        if "X-Amz-Signature=" in query:
            if f"X-Amz-Credential={s['placeholder_akid']}%2F" not in query and f"X-Amz-Credential={s['placeholder_akid']}/" not in query:
                continue
            old, q = sigv4.query_signature(s["placeholder"], method, path, query, hs)
            valid = old == q["X-Amz-Signature"]
            nq = query.replace(s["placeholder_akid"], s["real_akid"], 1)
            new, _ = sigv4.query_signature(s["real"], method, path, nq, hs)
            nq = nq.replace("X-Amz-Signature=" + q["X-Amz-Signature"], "X-Amz-Signature=" + new)
            return path + "?" + nq, hs, body, [{"secret": s["name"], "slot": "sigv4 presigned query", "placeholder_signature_valid": valid}]
        if len(auth_i) != 1:
            continue
        a = sigv4.parse_auth(hs[auth_i[0]][1])
        if not a or a["akid"] != s["placeholder_akid"]:
            continue
        if s.get("scopes") and f"{a['region']}/{a['service']}" not in s["scopes"]:
            return None  # outside the scopes you allowed: not signed, forwarded as it came
        old_sig, old_key, amz_date, scope = sigv4.header_signature(s["placeholder"], method, path, query, hs, a)
        valid = old_sig == a["sig"]
        new_sig, new_key, _, _ = sigv4.header_signature(s["real"], method, path, query, hs, a)
        nv = hs[auth_i[0]][1].replace(f"Credential={s['placeholder_akid']}/", f"Credential={s['real_akid']}/").replace("Signature=" + a["sig"], "Signature=" + new_sig)
        hs = [list(h) for h in hs]; hs[auth_i[0]][1] = nv
        f = [{"secret": s["name"], "slot": "sigv4 authorization", "scope": f"{a['region']}/{a['service']}", "placeholder_signature_valid": valid}]
        payload = (get(hs, "x-amz-content-sha256") or [""])[0]
        if payload.startswith("STREAMING-AWS4-HMAC-SHA256-PAYLOAD"):
            nb, n, ok = sigv4.resign_aws_chunked(body, amz_date, scope, old_key, a["sig"], new_key, new_sig)
            f.append({"secret": s["name"], "slot": "sigv4 aws-chunked", "chunks": n, "placeholder_chain_valid": ok, "length_kept": len(nb) == len(body)})
            body = nb
        return target, hs, body, f
    return None


# connections ---------------------------------------------------------------------------------

def upstream(host, port_tls):
    addr = CFG["hosts"].get(host)
    if not addr:
        return None
    h, p = addr.split(":")
    sock = socket.create_connection((h, int(p)), timeout=30)
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    if host in CFG.get("upstream_tls", []):
        cx = ssl.create_default_context(cafile=CFG["upstream_ca"])  # Silo's own store, not the computer's
        sock = cx.wrap_socket(sock, server_hostname=host)
    return sock


def handle_requests(rf, wf, tunnel_host, conn_id):
    while True:
        try:
            head = read_head(rf)
        except Bad as e:
            fact(event="refused", conn=conn_id, why=str(e)); send_simple(wf, 400, str(e)); return
        if head is None:
            return
        method, target, version = head[0].decode("latin-1").split(" ", 2)
        try:
            hs = parse_headers(head[1:])
        except Bad as e:
            fact(event="refused", conn=conn_id, why=str(e)); send_simple(wf, 400, str(e)); return
        if method == "CONNECT":
            return ("CONNECT", target)
        if tunnel_host is None:
            m = re.match(r"http://([^/]+)(.*)", target)
            if not m:
                send_simple(wf, 400, "absolute-form expected"); return
            host = host_of(m.group(1)); target = m.group(2) or "/"
        else:
            host = tunnel_host
        hosth = get(hs, "host")
        host_ok = len(hosth) == 1 and host_of(hosth[0]) == host
        rec = {"event": "request", "conn": conn_id, "host": host, "method": method, "path": target.split("?")[0]}

        # body: copied raw; a copy is read for placeholders. A client that waits for 100 Continue
        # gets it from the boundary, which reads the whole body before it connects upstream.
        if any(v.lower() == "100-continue" for v in get(hs, "expect")):
            wf.write(b"HTTP/1.1 100 Continue\r\n\r\n"); wf.flush()
        raw = b"".join(body_iter(rf, hs))
        rec["body_sha256"] = hashlib.sha256(raw).hexdigest(); rec["body_len"] = len(raw)

        swaps, seen = [], []
        if host_ok:
            out = []
            for n, v in hs:
                r = swap_header(n, v, host)
                if r and sum(1 for n2, _ in hs if n2.lower() == n.lower()) > 1:
                    seen.append({"secret": r[1], "where": f"header {n} (sent more than once: not swapped)"}); out.append([n, v]); continue
                if r:
                    out.append([n, r[0]]); swaps.append({"secret": r[1], "slot": r[2]})
                else:
                    out.append([n, v])
            hs = out
            target, qd = swap_query(target, host)
            swaps += [{"secret": a, "slot": b} for a, b in qd]
            sv = sigv4_resign(method, target, hs, raw, host)
            if sv:
                target, hs, raw, f = sv; swaps += f
        else:
            rec["host_header_mismatch"] = hosth
        swapped_names = {s["secret"] for s in swaps}
        for n, v in hs:
            for x in sightings(v.encode(), f"header {n}"):
                if x["secret"] not in swapped_names or n.lower() not in ("authorization", "x-api-key", "cookie"):
                    seen.append(x)
        seen += list(sightings(target.encode(), "request target"))
        seen += list(sightings(raw, "body (as sent)"))
        dec = decoded_copy(raw, hs)
        if dec is not None and dec is not raw:
            seen += [dict(x, where="body (decoded copy)") for x in sightings(dec, "")]
        rec["swapped"] = swaps; rec["placeholder_seen"] = seen
        fact(**rec)

        up = upstream(host, True)
        if not up:
            fact(event="no route", conn=conn_id, host=host); send_simple(wf, 502, "no route in this test"); return
        lines = [f"{method} {target} {version}".encode("latin-1")] + [f"{n}: {v}".encode("latin-1") for n, v in hs]
        up.sendall(b"\r\n".join(lines) + b"\r\n\r\n" + raw)
        close = relay_response(up, wf, method, host, conn_id)
        up.close()
        if close:
            return


def relay_response(up, wf, method, host, conn_id):
    must_close = False
    urf = up.makefile("rb")
    while True:  # informational replies (100 Continue) were already answered by the boundary
        head = read_head(urf)
        status = int(head[0].split(b" ")[1])
        if not (100 <= status < 200 and status != 101):
            break
    hs = parse_headers(head[1:])
    hits = []
    out_hs = []
    for n, v in hs:
        nv, h = scrub_bytes(v.encode("latin-1"), f"reply header {n}")
        hits += h; out_hs.append([n, nv.decode("latin-1")])
    enc = ",".join(get(hs, "content-encoding")).lower().strip()
    chunked = "chunked" in ",".join(get(hs, "transfer-encoding")).lower()
    status_line = head[0]
    if enc not in ("", "identity"):
        # compressed: read whole (prototype), scan a decoded copy, forward the original bytes if clean
        raw = b"".join(body_iter(urf, hs, True, method, status))
        dec = decoded_copy(raw, hs)
        if dec is None:
            fact(event="reply unread", conn=conn_id, host=host, why="encoding " + enc)
            body = raw
        else:
            clean, h = scrub_bytes(dec, "reply body (decoded)")
            if h:
                hits += h
                body = gzip.compress(clean) if enc == "gzip" else zlib.compress(clean)
                out_hs = [x for x in out_hs if x[0].lower() not in ("content-length", "transfer-encoding")] + [["Content-Length", str(len(body))]]
            else:
                body = raw
        wf.write(b"\r\n".join([status_line] + [f"{n}: {v}".encode("latin-1") for n, v in out_hs]) + b"\r\n\r\n" + body); wf.flush()
    else:
        wf.write(b"\r\n".join([status_line] + [f"{n}: {v}".encode("latin-1") for n, v in out_hs]) + b"\r\n\r\n"); wf.flush()
        sc = Scrubber("reply body")
        if chunked:
            # keep the server's chunk sizes: data is scrubbed as one stream, a chunk leaves when whole
            sizes, pending, trailers = [], bytearray(), b""
            for kind, val in chunk_pieces(urf):
                if kind == "size" and val:
                    sizes.append(val)
                elif kind == "data":
                    pending += sc.feed(val)
                elif kind == "end":
                    pending += sc.feed(b"", final=True); trailers = val
                while sizes and len(pending) >= sizes[0]:
                    n = sizes.pop(0)
                    wf.write(b"%x\r\n" % n + bytes(pending[:n]) + b"\r\n"); del pending[:n]
                wf.flush()
            wf.write(b"0\r\n" + trailers + b"\r\n")
        else:
            for d in body_iter(urf, hs, True, method, status):
                o = sc.feed(d)
                if o:
                    wf.write(o); wf.flush()
            wf.write(sc.feed(b"", final=True))
            if not get(hs, "content-length") and status >= 200 and status not in (204, 304) and method != "HEAD":
                must_close = True
        hits += sc.hits
        wf.flush()
    fact(event="reply", conn=conn_id, host=host, status=status, scrubbed=hits)
    return must_close


def chunk_pieces(rf):
    while True:
        line = rf.readline(4096); size = int(line.split(b";")[0], 16)
        yield ("size", size)
        if size == 0:
            tr = b""
            while True:
                t = rf.readline(8192)
                if t == b"\r\n" or not t:
                    break
                tr += t
            yield ("end", tr); return
        yield ("data", rf.read(size)); rf.read(2)


def send_simple(wf, code, msg):
    b = msg.encode()
    try:
        wf.write(b"HTTP/1.1 %d x\r\ncontent-length: %d\r\nconnection: close\r\n\r\n%s" % (code, len(b), b)); wf.flush()
    except Exception:
        pass


CERTS = {}


def tls_context_for(name):
    d = CFG["certs_dir"]
    if name not in CERTS:
        cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        cx.minimum_version = ssl.TLSVersion.TLSv1_2
        cx.set_alpn_protocols(["http/1.1"])  # this prototype speaks HTTP/1.1 only
        cx.load_cert_chain(f"{d}/{name}.pem", f"{d}/{name}.key")
        CERTS[name] = cx
    return CERTS[name]


def serve(conn, n):
    conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    try:
        rf, wf = conn.makefile("rb"), conn.makefile("wb")
        r = handle_requests(rf, wf, None, n)
        if r and r[0] == "CONNECT":
            host = host_of(r[1])
            if host not in CFG["hosts"]:
                fact(event="refused", conn=n, host=host, why="no route in this test")
                send_simple(wf, 403, "no route"); return
            wf.write(b"HTTP/1.1 200 Connection established\r\n\r\n"); wf.flush()
            first = conn.recv(1, socket.MSG_PEEK)
            if first == b"\x16":
                base = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                seen_sni = {}

                def sni(sock, name, ctx):
                    seen_sni["name"] = name
                    if name != host:
                        return ssl.ALERT_DESCRIPTION_UNRECOGNIZED_NAME
                    sock.context = tls_context_for(name)
                base.sni_callback = sni
                base.load_cert_chain(f"{CFG['certs_dir']}/{host}.pem", f"{CFG['certs_dir']}/{host}.key")
                tconn = base.wrap_socket(conn, server_side=True)
                handle_requests(tconn.makefile("rb"), tconn.makefile("wb"), host, n)
            else:
                handle_requests(rf, wf, host, n)
    except Exception as e:
        import traceback
        fact(event="error", conn=n, why=f"{type(e).__name__}: {e}", where=" | ".join(l.strip() for l in traceback.format_exc().splitlines() if l.strip().startswith("File") or "line" in l)[-900:])
    finally:
        conn.close()


def main():
    ls = socket.socket(); ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    ls.bind(("127.0.0.1", CFG["port"])); ls.listen(64)
    n = 0
    while True:
        c, _ = ls.accept(); n += 1
        threading.Thread(target=serve, args=(c, n), daemon=True).start()


if __name__ == "__main__":
    main()
