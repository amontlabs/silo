"""A Postgres boundary: the computer's DATABASE_URL holds a placeholder password, this process
holds the real one.

Client leg (inside the computer -> here): TLS with a certificate signed by a test "Silo CA",
SCRAM-SHA-256 or SCRAM-SHA-256-PLUS (tls-server-end-point) checked against the placeholder.
Server leg (here -> the real server): TLS verified against the server's CA, SCRAM-SHA-256-PLUS
with the real password.
Then every message is relayed. Simple Query (Q) and Parse/Bind/Execute (P/B/E) are sorted by
classify.py; an Execute or a Query whose kind the route holds waits for a file
holds/<id>.allow or holds/<id>.deny, or the hold limit. A denied or unanswered statement never
reaches the server: the boundary sends the server a token that cannot parse, so the server's
transaction ends up in exactly the state a failed statement leaves, and the boundary replaces
that error's words with its own.

Run: python pgboundary.py config.json
"""

from __future__ import annotations

import asyncio
import base64
import hashlib
import hmac
import json
import os
import secrets
import ssl
import struct
import sys
import time
import uuid

from cryptography import x509
from cryptography.hazmat.primitives.serialization import Encoding

import classify

SSL_REQUEST, GSS_REQUEST, CANCEL_REQUEST, PROTO_3 = 80877103, 80877104, 80877102, 196608
PASS_PARAMS = {"application_name", "client_encoding", "DateStyle", "TimeZone",
               "extra_float_digits", "IntervalStyle", "search_path"}

SESSIONS: dict[tuple[int, int], "Session"] = {}


def record(cfg, **fields):
    fields["t"] = round(time.time(), 3)
    with open(cfg["record"], "a") as f:
        f.write(json.dumps(fields) + "\n")


# ---------------------------------------------------------------- wire helpers

async def read_msg(reader):
    head = await reader.readexactly(5)
    typ, ln = head[:1], struct.unpack("!I", head[1:])[0]
    return typ, await reader.readexactly(ln - 4)


def msg(typ: bytes, body: bytes = b"") -> bytes:
    return typ + struct.pack("!I", len(body) + 4) + body


def error_body(code: str, message: str, detail: str | None = None, hint: str | None = None,
               severity="ERROR") -> bytes:
    f = [b"S" + severity.encode(), b"V" + severity.encode(), b"C" + code.encode(),
         b"M" + message.encode()]
    if detail:
        f.append(b"D" + detail.encode())
    if hint:
        f.append(b"H" + hint.encode())
    return b"\0".join(f) + b"\0\0"


def parse_error(body: bytes) -> dict:
    out = {}
    for part in body.split(b"\0"):
        if part:
            out[chr(part[0])] = part[1:].decode(errors="replace")
    return out


def cstr(buf: bytes, i: int):
    j = buf.index(b"\0", i)
    return buf[i:j].decode(), j + 1


def cert_end_point_hash(der: bytes) -> bytes:
    """RFC 5929 tls-server-end-point: the certificate hashed with its signature's hash,
    SHA-256 when that is MD5 or SHA-1."""
    algo = x509.load_der_x509_certificate(der).signature_hash_algorithm
    name = algo.name if algo else "sha256"
    if name in ("md5", "sha1"):
        name = "sha256"
    return hashlib.new(name, der).digest()


# ---------------------------------------------------------------- SCRAM

def hi(password: str, salt: bytes, it: int) -> bytes:
    return hashlib.pbkdf2_hmac("sha256", password.encode(), salt, it)


def _hmac(k, m):
    return hmac.new(k, m, hashlib.sha256).digest()


def scram_keys(password: str, salt: bytes, it: int):
    salted = hi(password, salt, it)
    client_key = _hmac(salted, b"Client Key")
    return hashlib.sha256(client_key).digest(), _hmac(salted, b"Server Key"), client_key


class ScramVerifier:
    """Server side of SCRAM, for the placeholder. Like Postgres, it keeps only StoredKey and
    ServerKey, made once per route."""

    def __init__(self, placeholder: str):
        self.salt, self.it = secrets.token_bytes(16), 4096
        self.stored_key, self.server_key, _ = scram_keys(placeholder, self.salt, self.it)


async def auth_client(reader, writer, route, tls_cb: bytes | None):
    """Returns None when the client proved it holds the placeholder, else an error string."""
    v: ScramVerifier = route["_verifier"]
    mechs = (b"SCRAM-SHA-256-PLUS\0" if tls_cb else b"") + b"SCRAM-SHA-256\0\0"
    writer.write(msg(b"R", struct.pack("!I", 10) + mechs))
    await writer.drain()
    typ, body = await read_msg(reader)
    if typ != b"p":
        return "expected SASLInitialResponse"
    mech, i = cstr(body, 0)
    ln = struct.unpack("!i", body[i:i + 4])[0]
    first = body[i + 4:i + 4 + ln].decode()
    gs2_flag, _, rest = first.partition(",")
    authzid, _, bare = rest.partition(",")
    gs2_header = f"{gs2_flag},{authzid},"
    if mech == "SCRAM-SHA-256-PLUS":
        if gs2_flag != "p=tls-server-end-point" or not tls_cb:
            return "channel binding type not supported"
        cb_data = tls_cb
    else:
        if gs2_flag == "y" and tls_cb:
            # The client could bind and thinks we cannot: someone stripped -PLUS on the way.
            return "SCRAM channel binding downgrade detected"
        cb_data = b""
    attrs = dict(a.split("=", 1) for a in bare.split(","))
    nonce = attrs["r"] + base64.b64encode(secrets.token_bytes(18)).decode()
    server_first = f"r={nonce},s={base64.b64encode(v.salt).decode()},i={v.it}"
    writer.write(msg(b"R", struct.pack("!I", 11) + server_first.encode()))
    await writer.drain()
    typ, body = await read_msg(reader)
    final = body.decode()
    without_proof, _, proof_b64 = final.rpartition(",p=")
    fattrs = dict(a.split("=", 1) for a in without_proof.split(","))
    if fattrs.get("r") != nonce:
        return "SCRAM nonce mismatch"
    if base64.b64decode(fattrs["c"]) != gs2_header.encode() + cb_data:
        return "SCRAM channel binding check failed"
    auth_message = f"{bare},{server_first},{without_proof}".encode()
    proof = base64.b64decode(proof_b64)
    client_key = bytes(a ^ b for a, b in zip(proof, _hmac(v.stored_key, auth_message)))
    if not hmac.compare_digest(hashlib.sha256(client_key).digest(), v.stored_key):
        return "password authentication failed"
    sig = base64.b64encode(_hmac(v.server_key, auth_message)).decode()
    writer.write(msg(b"R", struct.pack("!I", 12) + f"v={sig}".encode()))
    return None


async def auth_server(reader, writer, password: str, server_cb: bytes | None):
    """Client side of SCRAM against the real server, with the real password."""
    typ, body = await read_msg(reader)
    if typ == b"E":
        raise ConnectionError(parse_error(body).get("M"))
    code = struct.unpack("!I", body[:4])[0]
    if code == 0:
        return
    if code != 10:
        raise ConnectionError(f"upstream asked for auth method {code}; this prototype does SCRAM")
    mechs = body[4:].split(b"\0")
    plus = b"SCRAM-SHA-256-PLUS" in mechs and server_cb is not None
    mech = "SCRAM-SHA-256-PLUS" if plus else "SCRAM-SHA-256"
    gs2 = "p=tls-server-end-point,," if plus else ("y,," if server_cb else "n,,")
    cnonce = base64.b64encode(secrets.token_bytes(18)).decode()
    bare = f"n=,r={cnonce}"
    first = (gs2 + bare).encode()
    writer.write(msg(b"p", mech.encode() + b"\0" + struct.pack("!i", len(first)) + first))
    await writer.drain()
    typ, body = await read_msg(reader)
    if typ == b"E":
        raise ConnectionError(parse_error(body).get("M"))
    server_first = body[4:].decode()
    a = dict(x.split("=", 1) for x in server_first.split(","))
    if not a["r"].startswith(cnonce):
        raise ConnectionError("upstream SCRAM nonce mismatch")
    stored, server_key, client_key = scram_keys(password, base64.b64decode(a["s"]), int(a["i"]))
    cb = gs2.encode() + (server_cb if plus else b"")
    without_proof = f"c={base64.b64encode(cb).decode()},r={a['r']}"
    auth_message = f"{bare},{server_first},{without_proof}".encode()
    proof = bytes(x ^ y for x, y in zip(client_key, _hmac(stored, auth_message)))
    writer.write(msg(b"p", f"{without_proof},p={base64.b64encode(proof).decode()}".encode()))
    await writer.drain()
    typ, body = await read_msg(reader)
    if typ == b"E":
        raise ConnectionError(parse_error(body).get("M"))
    expect = base64.b64encode(_hmac(server_key, auth_message)).decode()
    if body[4:].decode() != f"v={expect}":
        raise ConnectionError("upstream server signature wrong: not the real server")
    typ, body = await read_msg(reader)  # AuthenticationOk
    if typ != b"R" or struct.unpack("!I", body[:4])[0] != 0:
        raise ConnectionError("upstream did not accept authentication")
    return mech


# ---------------------------------------------------------------- holds

async def wait_for_answer(cfg, route, sess, verdict, sql):
    """A hold: waits for holds/<id>.allow or .deny, the client closing, a cancel, or the
    limit. Returns ('allowed'|'denied'|'not answered'|'closed'|'cancelled', id)."""
    hid = uuid.uuid4().hex[:12]
    d = cfg["holds_dir"]
    with open(os.path.join(d, hid + ".json"), "w") as f:
        json.dump({"id": hid, "route": route["name"], "kind": verdict.kind,
                   "reason": verdict.reason(), "sql": sql[:2000]}, f)
    record(cfg, ev="held", session=sess.id, hold=hid, kind=verdict.kind,
           reason=verdict.reason(), sql=sql[:500])
    print(f"[hold {hid}] {route['name']}: {verdict.kind} ({verdict.reason()}): {sql[:120]!r}",
          flush=True)
    limit = route.get("hold_limit_s", 90)
    t0 = time.monotonic()
    sess.cancelled.clear()
    outcome = "not answered"
    while time.monotonic() - t0 < limit:
        if os.path.exists(os.path.join(d, hid + ".allow")):
            outcome = "allowed"
            break
        if os.path.exists(os.path.join(d, hid + ".deny")):
            outcome = "denied"
            break
        if sess.client_closed.is_set():
            outcome = "closed"
            break
        if sess.cancelled.is_set():
            outcome = "cancelled"
            break
        await asyncio.sleep(0.05)
    record(cfg, ev="hold ended", session=sess.id, hold=hid, outcome=outcome,
           waited_s=round(time.monotonic() - t0, 2))
    return outcome, hid


def refusal_text(route, verdict, outcome, hid):
    what = verdict.reason()
    if outcome == "denied":
        head = f"Silo denied this statement ({what}) on {route['name']}."
    elif outcome == "cancelled":
        head = f"Silo held this statement ({what}) on {route['name']}; the client cancelled it."
    else:
        head = (f"Silo held this statement ({what}) on {route['name']} and nobody answered "
                f"within {route.get('hold_limit_s', 90)} s. It did not run.")
    detail = f"rule of {route['name']}" if hid == "rule" else f"hold {hid}"
    return head, detail, "Ask the user to allow it in Silo, or change the statement."


# ---------------------------------------------------------------- session

class Session:
    def __init__(self, cfg, route, creader, cwriter, sreader, swriter):
        self.cfg, self.route = cfg, route
        self.c_r, self.c_w, self.s_r, self.s_w = creader, cwriter, sreader, swriter
        self.id = uuid.uuid4().hex[:8]
        self.prepared: dict[str, classify.Verdict] = {}
        self.portals: dict[str, tuple[classify.Verdict, str]] = {}
        self.stmt_sql: dict[str, str] = {}
        self.rewrites: dict[str, tuple] = {}  # token -> (code, message, detail, hint)
        self.client_closed = asyncio.Event()
        self.cancelled = asyncio.Event()
        self.queue: asyncio.Queue = asyncio.Queue()
        self.skip_to_sync = False
        self.key = None

    def held(self, verdict):
        return verdict.kind in self.route.get("hold", [])

    def denied(self, verdict):
        return verdict.kind in self.route.get("deny", [])

    async def refuse(self, verdict, outcome, hid, extended):
        """Make the server fail exactly where the statement would have run, then reword."""
        head, detail, hint = refusal_text(self.route, verdict, outcome, hid)
        token = f"silo_held_{uuid.uuid4().hex}"
        code = "57014" if outcome == "cancelled" else "42501"
        self.rewrites[token] = (code, head, detail, hint)
        if extended:
            self.s_w.write(msg(b"P", b"\0" + token.encode() + b"\0" + struct.pack("!H", 0)))
            self.skip_to_sync = True
        else:
            self.s_w.write(msg(b"Q", token.encode() + b"\0"))
        await self.s_w.drain()

    async def gate(self, verdict, sql, extended):
        """True: forward the message. False: refused (already handled)."""
        if verdict.kind != "read" or self.route.get("record_reads"):
            record(self.cfg, ev="statement", session=self.id, kind=verdict.kind,
                   reason=verdict.reason(), sql=sql[:500])
        if self.denied(verdict):
            await self.refuse(verdict, "denied", "rule", extended)
            record(self.cfg, ev="denied", session=self.id, kind=verdict.kind, sql=sql[:500])
            return False
        if not self.held(verdict):
            return True
        outcome, hid = await wait_for_answer(self.cfg, self.route, self, verdict, sql)
        if outcome == "allowed":
            return True
        if outcome == "closed":
            return False
        await self.refuse(verdict, outcome, hid, extended)
        return False

    def note_created_functions(self, sql):
        names = classify.created_function_names(sql)
        if names:
            self.route["_catalog"].user_defined |= names

    async def client_reader(self):
        try:
            while True:
                m = await read_msg(self.c_r)
                await self.queue.put(m)
                if m[0] == b"X":
                    break
        except (asyncio.IncompleteReadError, ConnectionError, ssl.SSLError):
            pass
        self.client_closed.set()
        await self.queue.put(None)

    async def client_to_server(self):
        cat = self.route["_catalog"]
        while True:
            m = await self.queue.get()
            if m is None:
                break
            typ, body = m
            if self.skip_to_sync:
                if typ != b"S":
                    continue
                self.skip_to_sync = False
            if self.route.get("relay_only"):  # for measuring: no parse, no hold
                pass
            elif typ == b"Q":
                sql = body[:-1].decode(errors="replace")
                v = classify.classify(sql, cat, self.prepared)
                if not await self.gate(v, sql, extended=False):
                    continue
                self.note_created_functions(sql)
            elif typ == b"P":
                name, i = cstr(body, 0)
                sql, _ = cstr(body, i)
                self.prepared[name] = classify.classify(sql, cat, self.prepared)
                self.stmt_sql[name] = sql
            elif typ == b"B":
                portal, i = cstr(body, 0)
                stmt, _ = cstr(body, i)
                if stmt in self.prepared:
                    self.portals[portal] = (self.prepared[stmt], self.stmt_sql[stmt])
            elif typ == b"E":
                portal, _ = cstr(body, 0)
                v, sql = self.portals.get(portal, (None, ""))
                if v is None:
                    v = classify.Verdict("opaque", [classify.Finding(
                        "opaque", "Execute of a portal the boundary did not see bound")], 0)
                if not await self.gate(v, sql, extended=True):
                    continue
                self.note_created_functions(sql)
            elif typ == b"F":
                v = classify.Verdict("opaque", [classify.Finding(
                    "opaque", "a fast-path function call (by OID)")], 0)
                if not await self.gate(v, "<fast-path call>", extended=True):
                    continue
            self.s_w.write(msg(typ, body))
            await self.s_w.drain()
            if typ == b"X":
                break
        try:
            self.s_w.close()
        except Exception:
            pass

    async def server_to_client(self):
        try:
            while True:
                typ, body = await read_msg(self.s_r)
                if typ == b"E" and self.rewrites:
                    err = parse_error(body)
                    for token, (code, head, detail, hint) in list(self.rewrites.items()):
                        if token in err.get("M", ""):
                            del self.rewrites[token]
                            body = error_body(code, head, detail, hint)
                            break
                self.c_w.write(msg(typ, body))
                await self.c_w.drain()
        except (asyncio.IncompleteReadError, ConnectionError, ssl.SSLError):
            pass
        try:
            self.c_w.close()
        except Exception:
            pass


# ---------------------------------------------------------------- connection setup

def pick_route(cfg, sni, params):
    for r in cfg["routes"]:
        if sni and r.get("sni") and r["sni"] != sni:
            continue
        if r["user"] == params.get("user") and r["database"] == params.get("database",
                                                                          params.get("user")):
            return r
    return None


async def open_upstream(route, params):
    up = route["upstream"]
    r, w = await asyncio.open_connection(up["host"], up["port"])
    w.write(struct.pack("!II", 8, SSL_REQUEST))
    await w.drain()
    if await r.readexactly(1) != b"S":
        raise ConnectionError("upstream refused TLS; the boundary never sends a password in clear")
    ctx = ssl.create_default_context(cafile=up["ca"])
    await w.start_tls(ctx, server_hostname=up["sni"])
    der = w.get_extra_info("ssl_object").getpeercert(binary_form=True)
    startup = {"user": up["user"], "database": up.get("database", route["database"])}
    for k in PASS_PARAMS:
        if k in params:
            startup[k] = params[k]
    if route.get("read_only_session"):
        startup["options"] = "-c default_transaction_read_only=on"
    body = struct.pack("!I", PROTO_3) + b"".join(
        k.encode() + b"\0" + v.encode() + b"\0" for k, v in startup.items()) + b"\0"
    w.write(struct.pack("!I", len(body) + 4) + body)
    await w.drain()
    with open(up["password_file"]) as f:
        password = f.read().strip()
    mech = await auth_server(r, w, password, cert_end_point_hash(der))
    del password
    return r, w, mech


async def load_catalog(route, r, w):
    """One query on the fresh server session, before the client's first message."""
    cat = classify.Catalog()
    w.write(msg(b"Q", classify.Catalog.SQL.encode() + b"\0"))
    await w.drain()
    rows = []
    while True:
        typ, body = await read_msg(r)
        if typ == b"D":
            n = struct.unpack("!H", body[:2])[0]
            i, vals = 2, []
            for _ in range(n):
                ln = struct.unpack("!i", body[i:i + 4])[0]
                i += 4
                vals.append(body[i:i + ln].decode())
                i += ln
            rows.append(vals)
        elif typ == b"Z":
            break
    cat.load(rows)
    route["_catalog"] = cat


async def handle(cfg, sslctx, reader, writer):
    try:
        ln, code = struct.unpack("!II", await reader.readexactly(8))
        if code == GSS_REQUEST:
            writer.write(b"N")
            await writer.drain()
            ln, code = struct.unpack("!II", await reader.readexactly(8))
        if code == SSL_REQUEST:
            writer.write(b"S")
            await writer.drain()
            await writer.start_tls(sslctx)
            ln, code = struct.unpack("!II", await reader.readexactly(8))
        if code == CANCEL_REQUEST:
            pid, key = struct.unpack("!II", await reader.readexactly(8))
            sess = SESSIONS.get((pid, key))
            if sess:
                sess.cancelled.set()
                await forward_cancel(sess.route, pid, key)
            writer.close()
            return
        if code != PROTO_3:
            writer.close()
            return
        raw = await reader.readexactly(ln - 8)
        parts = raw.split(b"\0")
        params = {parts[i].decode(): parts[i + 1].decode() for i in range(0, len(parts) - 2, 2)
                  if parts[i]}
        tls = writer.get_extra_info("ssl_object")
        sni = getattr(tls, "silo_sni", None)
        route = pick_route(cfg, sni, params)
        if route is None:
            writer.write(msg(b"E", error_body("28000", f"Silo has no database rule for user "
                                              f"{params.get('user')!r} on this host")))
            await writer.drain()
            writer.close()
            return
        if "options" in params:
            record(cfg, ev="startup options dropped", options=params["options"])
        if "replication" in params:
            writer.write(msg(b"E", error_body("28000", "Silo does not pass replication "
                                              "connections")))
            await writer.drain()
            writer.close()
            return
        cb = cert_end_point_hash(cfg["_cert_der"]) if tls is not None else None
        err = await auth_client(reader, writer, route, cb)
        if err:
            record(cfg, ev="client auth failed", route=route["name"], why=err)
            writer.write(msg(b"E", error_body("28P01", err)))
            await writer.drain()
            writer.close()
            return
        try:
            sr, sw, mech = await open_upstream(route, params)
            startup_msgs = []
            while True:  # the server's ParameterStatus, BackendKeyData, ReadyForQuery
                typ, body = await read_msg(sr)
                if typ == b"E":
                    raise ConnectionError(parse_error(body).get("M"))
                startup_msgs.append((typ, body))
                if typ == b"Z":
                    break
            if "_catalog" not in route:
                await load_catalog(route, sr, sw)
        except Exception as e:
            writer.write(msg(b"E", error_body("08006", f"Silo could not reach the database: {e}")))
            await writer.drain()
            writer.close()
            return
        writer.write(msg(b"R", struct.pack("!I", 0)))  # AuthenticationOk
        sess = Session(cfg, route, reader, writer, sr, sw)
        record(cfg, ev="connected", session=sess.id, route=route["name"],
               client_tls=tls is not None, sni=sni, upstream_auth=mech,
               client=params.get("application_name"))
        for typ, body in startup_msgs:
            if typ == b"K":
                sess.key = struct.unpack("!II", body)
                SESSIONS[sess.key] = sess
            writer.write(msg(typ, body))
        await writer.drain()
        await asyncio.gather(sess.client_reader(), sess.client_to_server(),
                             sess.server_to_client())
        SESSIONS.pop(sess.key, None)
    except (asyncio.IncompleteReadError, ConnectionError, ssl.SSLError) as e:
        try:
            writer.close()
        except Exception:
            pass


async def forward_cancel(route, pid, key):
    up = route["upstream"]
    r, w = await asyncio.open_connection(up["host"], up["port"])
    w.write(struct.pack("!II", 8, SSL_REQUEST))
    await w.drain()
    if await r.readexactly(1) == b"S":
        await w.start_tls(ssl.create_default_context(cafile=up["ca"]), server_hostname=up["sni"])
    w.write(struct.pack("!IIII", 16, CANCEL_REQUEST, pid, key))
    await w.drain()
    w.close()


async def main(path):
    with open(path) as f:
        cfg = json.load(f)
    os.makedirs(cfg["holds_dir"], exist_ok=True)
    for r in cfg["routes"]:
        r["_verifier"] = ScramVerifier(r["placeholder"])
    sslctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    sslctx.load_cert_chain(cfg["tls"]["cert"], cfg["tls"]["key"])

    def on_sni(sslobj, name, ctx):
        sslobj.silo_sni = name  # the name the program gave in the handshake

    sslctx.sni_callback = on_sni
    with open(cfg["tls"]["cert"], "rb") as f:
        cfg["_cert_der"] = x509.load_pem_x509_certificate(f.read()).public_bytes(Encoding.DER)
    host, port = cfg["listen"].rsplit(":", 1)
    srv = await asyncio.start_server(lambda r, w: handle(cfg, sslctx, r, w), host, int(port))
    print(f"pgboundary listening on {cfg['listen']}", flush=True)
    async with srv:
        await srv.serve_forever()


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1]))
