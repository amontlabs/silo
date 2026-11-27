"""A login vault at the boundary: a subscription's sign-in never lands in the computer.

The program signs in and refreshes the vendor's own way, unmodified. The boundary, which already
ends TLS to the provider's hosts (`ending-the-encrypted-connection`), does four things:

1. Sign-in. A reply of the provider's token endpoint to an authorization-code exchange carries the
   real tokens. The vault keeps them and rewrites the reply with placeholders made for this
   computer: same length, same public prefix, and for a JWT the same header and claims with a
   signature that is not one. The program stores the placeholders.
2. Use. A request to the provider's API host whose `Authorization: Bearer` is a placeholder of this
   computer gets the real access token in its place. Nothing else is touched.
3. Refresh. A refresh with a placeholder refresh token is answered by the vault. One real refresh
   chain serves every computer that shares the login, so computers never sign each other out. If
   the real access token is still valid the vault answers alone; otherwise it refreshes upstream
   once (one at a time per login) and hands out new placeholders.
   The vault also refreshes on its own when a request arrives with a valid placeholder and the real
   access token is about to expire, so a program never sees an expired token.
4. Refusals. A placeholder used from another computer, or a retired one, is refused locally and
   recorded. A placeholder copied out of the computer is worth nothing anywhere: no provider has
   ever seen it.

A login Silo did not see being made (a real token already in the computer) passes unchanged, and
the record says so: Silo stays out of it.

Env: LISTEN (0.0.0.0:443), CERT, KEY (Silo's leaf for the providers' names), UPSTREAM (host:port of
the provider, for the test), UPSTREAM_CA, COMPUTERS ("10.77.1.2=a,10.77.2.2=b"), RECORD (jsonl),
STATE (json: real tokens; in Silo this is the Keychain), MODE=vault|pass."""
import asyncio, base64, json, os, secrets, ssl, sys, time

LISTEN = os.environ.get("LISTEN", "0.0.0.0:443")
UP = os.environ.get("UPSTREAM", "127.0.0.1:8443")
MODE = os.environ.get("MODE", "vault")
COMPUTERS = dict(kv.split("=") for kv in os.environ.get("COMPUTERS", "127.0.0.1=a").split(","))
RECORD = open(os.environ.get("RECORD", "record.jsonl"), "a")
TOKEN_HOSTS = {"platform.claude.com": "anthropic", "console.anthropic.com": "anthropic", "auth.openai.com": "openai"}
API_HOSTS = {"api.anthropic.com": "anthropic", "chatgpt.com": "openai", "api.openai.com": "openai", "claude.ai": "anthropic"}
REFRESH_MARGIN = int(os.environ.get("REFRESH_MARGIN", "120"))
ALPH = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-"

LOGINS = {}       # login id -> {"provider", "access", "refresh", "id_token", "exp", "lock"}
PH = {}           # placeholder -> {"login", "computer", "kind", "retired_at"}
CUR = {}          # (login, computer) -> {"access": ph, "refresh": ph, "id_token": ph}
CAPN = [0]
SNI = {}            # TLS object -> the name in its handshake
STATS = {"swapped": 0, "refused": 0, "upstream_refresh": 0, "local_refresh": 0}


def secret_values():
    for L in LOGINS.values():
        for k in ("access", "refresh", "id_token", "api_key"):
            if L.get(k): yield L[k]


def fact(**kw):
    kw["t"] = round(time.time(), 3)
    line = json.dumps(kw)
    for v in secret_values():          # the record never holds a real value: stop if it would
        assert v[-24:] not in line, "a real token reached the record"
    RECORD.write(line + "\n"); RECORD.flush()
    print("fact", line, file=sys.stderr, flush=True)


def jwt_exp(tok):
    try:
        p = tok.split(".")[1]; return json.loads(base64.urlsafe_b64decode(p + "=" * (-len(p) % 4)))["exp"]
    except Exception:
        return None


def make_ph(real):
    """Same length and public shape. A JWT keeps its header and claims (the program reads them: the
    account id, the plan, the expiry), and gets a signature of the same length that is not valid."""
    if real.count(".") == 2 and real.startswith("eyJ"):
        h, p, s = real.split(".")
        return f"{h}.{p}.{''.join(secrets.choice(ALPH) for _ in s)}"
    for pre in ("sk-ant-oat01-", "sk-ant-ort01-", "rt_"):
        if real.startswith(pre):
            body = real[len(pre):]
            return pre + "".join(c if c == "." else secrets.choice(ALPH) for c in body)
    return "".join(secrets.choice(ALPH) for _ in real)


def placeholders_for(lid, comp):
    """New placeholders for this computer; the previous ones stay usable for 60 s (requests in flight)."""
    L = LOGINS[lid]; old = CUR.get((lid, comp))
    if old:
        for v in old.values():
            if v in PH: PH[v]["retired_at"] = time.time()
    cur = {k: make_ph(L[k]) for k in ("access", "refresh", "id_token") if L.get(k)}
    for k, v in cur.items(): PH[v] = {"login": lid, "computer": comp, "kind": k, "retired_at": None}
    CUR[(lid, comp)] = cur
    return cur


def lookup(ph, comp, kind):
    """-> (login id, None) or (None, reason). Reasons are facts."""
    e = PH.get(ph)
    if not e: return None, None
    if e["kind"] != kind: return None, f"a {e['kind']} placeholder used as {kind}"
    if e["computer"] != comp: return None, f"placeholder of computer {e['computer']} used from computer {comp}"
    if e["retired_at"] and time.time() - e["retired_at"] > 60: return None, "retired placeholder"
    return e["login"], None


def save_state():
    if os.environ.get("STATE"):
        json.dump({k: {kk: vv for kk, vv in v.items() if kk != "lock"} for k, v in LOGINS.items()}, open(os.environ["STATE"], "w"))


def token_reply_with(cur, L, extra):
    out = dict(extra)
    out["access_token"] = cur["access"]; out["refresh_token"] = cur["refresh"]
    if "id_token" in cur: out["id_token"] = cur["id_token"]
    out["expires_in"] = max(1, int(L["exp"] - time.time()))
    return out


# ---------------------------------------------------------------- HTTP/1.1, strict enough for this
class Bad(Exception): pass


async def read_head(r):
    head = await r.readuntil(b"\r\n\r\n")
    lines = head[:-4].split(b"\r\n")
    hs = []
    for l in lines[1:]:
        if b":" not in l or l[:1] in (b" ", b"\t"): raise Bad("malformed header line")
        k, v = l.split(b":", 1)
        if k != k.strip(): raise Bad("whitespace in a header name")
        hs.append([k.decode(), v.strip().decode("latin-1")])
    return lines[0].decode("latin-1"), hs


def hget(hs, name):
    vs = [v for k, v in hs if k.lower() == name]
    if len(vs) > 1 and name in ("content-length", "authorization", "host", "transfer-encoding"): raise Bad(f"two {name} headers")
    return vs[0] if vs else None


def hset(hs, name, val):
    for h in hs:
        if h[0].lower() == name: h[1] = val; return
    hs.append([name, val])


async def read_body(r, hs):
    te = hget(hs, "transfer-encoding"); cl = hget(hs, "content-length")
    if te and cl: raise Bad("both Content-Length and Transfer-Encoding")
    if te:
        if te.lower() != "chunked": raise Bad("transfer-encoding other than chunked")
        out = b""
        while True:
            n = int((await r.readuntil(b"\r\n"))[:-2].split(b";")[0], 16)
            if n == 0:
                await r.readuntil(b"\r\n"); return out, "chunked"
            out += await r.readexactly(n); await r.readexactly(2)
    if cl:
        if not cl.isdigit(): raise Bad("bad content-length")
        return await r.readexactly(int(cl)), "cl"
    return b"", "none"


def build(start, hs, body, framing):
    if framing == "chunked":
        hs = [h for h in hs if h[0].lower() != "transfer-encoding"]; framing = "cl"
    if framing == "cl" or body:
        hs = [h for h in hs if h[0].lower() != "content-length"] + [["content-length", str(len(body))]]
    return (start + "\r\n" + "".join(f"{k}: {v}\r\n" for k, v in hs) + "\r\n").encode("latin-1") + body


# ---------------------------------------------------------------- the upstream leg
def up_ctx():
    cx = ssl.create_default_context(cafile=os.environ["UPSTREAM_CA"]); cx.set_alpn_protocols(["http/1.1"]); return cx


async def upstream_once(host, raw, writer=None, scrub=None):
    """Send one request on a fresh upstream connection. With `writer`, stream the reply to it
    (scrubbing real values); without, return (status, headers, body)."""
    h, p = UP.split(":")
    ur, uw = await asyncio.open_connection(h, int(p), ssl=up_ctx(), server_hostname=host)
    uw.write(raw); await uw.drain()
    status, hs = await read_head(ur)
    code = int(status.split()[1])
    te = (hget(hs, "transfer-encoding") or "").lower(); cl = hget(hs, "content-length")
    if writer is None:
        body, _ = await read_body(ur, hs); uw.close(); return code, hs, body
    writer.write((status + "\r\n" + "".join(f"{k}: {v}\r\n" for k, v in hs) + "\r\n").encode("latin-1"))
    async def fwd(b):
        if scrub: b = scrub(b)
        writer.write(b); await writer.drain()
    if te == "chunked":
        while True:
            line = await ur.readuntil(b"\r\n"); n = int(line[:-2].split(b";")[0], 16)
            data = await ur.readexactly(n + 2)
            await fwd(line + data)
            if n == 0: break
    elif cl:
        left = int(cl)
        while left:
            b = await ur.read(min(left, 65536))
            if not b: break
            left -= len(b); await fwd(b)
    else:
        while (b := await ur.read(65536)): await fwd(b)
    uw.close()
    return code, hs, None


def scrubber(b):
    """A reply never brings a real token back into the computer."""
    for lid, L in LOGINS.items():
        for k in ("access", "refresh", "id_token"):
            v = L.get(k)
            if v and v.encode() in b:
                ph = next((p for (l, c), cur in CUR.items() if l == lid for kk, p in cur.items() if kk == k), None)
                b = b.replace(v.encode(), (ph or "x" * len(v)).encode()); fact(event="scrubbed from a reply", login=lid, kind=k)
    return b


async def refresh_upstream(lid, host_for_token):
    L = LOGINS[lid]
    async with L["lock"]:
        if L["exp"] - time.time() > REFRESH_MARGIN:
            return True
        body = {"grant_type": "refresh_token", "refresh_token": L["refresh"], "client_id": L.get("client_id", "")}
        if L.get("scope"): body["scope"] = L["scope"]
        raw = json.dumps(body).encode()
        req = build(f"POST {L['token_path']} HTTP/1.1", [["host", L["token_host"]], ["content-type", "application/json"], ["accept", "application/json"]], raw, "cl")
        code, hs, rb = await upstream_once(L["token_host"], req)
        STATS["upstream_refresh"] += 1
        if code != 200:
            fact(event="refresh refused by the provider", login=lid, status=code, reason=rb.decode()[:200])
            return False
        j = json.loads(rb)
        L["access"] = j["access_token"]; L["refresh"] = j.get("refresh_token", L["refresh"])
        if j.get("id_token"): L["id_token"] = j["id_token"]
        L["exp"] = time.time() + int(j.get("expires_in", 3600))
        save_state()
        fact(event="refreshed by Silo", login=lid, expires_in=j.get("expires_in"))
        return True


# ---------------------------------------------------------------- one request from a computer
async def handle_token(comp, host, start, hs, body, w):
    ctype = (hget(hs, "content-type") or "").lower()
    try:
        j = json.loads(body) if "json" in ctype or body[:1] == b"{" else None
    except Exception:
        j = None
    form = None
    if j is None:
        from urllib.parse import parse_qsl
        form = dict(parse_qsl(body.decode())); j = form
    grant = j.get("grant_type")
    path = start.split()[1]
    if grant == "authorization_code":
        code, rhs, rb = await upstream_once(host, build(start, hs, body, "cl"))
        if code == 200:
            r = json.loads(rb); lid = "login-" + secrets.token_hex(4)
            exp = time.time() + int(r.get("expires_in", 3600))
            LOGINS[lid] = {"provider": TOKEN_HOSTS[host], "access": r["access_token"], "refresh": r.get("refresh_token"),
                           "id_token": r.get("id_token"), "exp": exp, "lock": asyncio.Lock(), "token_host": host,
                           "token_path": path, "client_id": j.get("client_id", ""), "scope": r.get("scope") if TOKEN_HOSTS[host] == "anthropic" else None}
            save_state()
            cur = placeholders_for(lid, comp)
            rb = json.dumps(token_reply_with(cur, LOGINS[lid], r)).encode()
            fact(event="sign-in kept by Silo", computer=comp, login=lid, provider=TOKEN_HOSTS[host])
        w.write(build(f"HTTP/1.1 {code} {'OK' if code == 200 else 'Error'}", [h for h in rhs if h[0].lower() not in ("content-length", "transfer-encoding")], rb, "cl"))
        return
    if grant == "refresh_token":
        rt = j.get("refresh_token", "")
        lid, why = lookup(rt, comp, "refresh")
        if why:
            STATS["refused"] += 1; fact(event="refused", computer=comp, reason=why, where="refresh")
            err = json.dumps({"error": "invalid_grant", "error_description": f"Silo: {why}"}).encode()
            w.write(build("HTTP/1.1 400 Bad Request", [["content-type", "application/json"]], err, "cl")); return
        if lid is None:
            fact(event="refresh of a login Silo does not hold, passed unchanged", computer=comp, host=host)
            await upstream_once(host, build(start, hs, body, "cl"), w); return
        L = LOGINS[lid]
        if L["exp"] - time.time() > REFRESH_MARGIN:
            STATS["local_refresh"] += 1; how = "answered by Silo, the real token is still valid"
        else:
            if not await refresh_upstream(lid, host):
                err = json.dumps({"error": "invalid_grant", "error_description": "Silo: the provider refused the refresh; sign in again"}).encode()
                w.write(build("HTTP/1.1 400 Bad Request", [["content-type", "application/json"]], err, "cl")); return
            how = "refreshed upstream by Silo"
        cur = placeholders_for(lid, comp)
        extra = {"token_type": "Bearer"}
        if L["provider"] == "anthropic" and L.get("scope"): extra["scope"] = L["scope"]
        rb = json.dumps(token_reply_with(cur, L, extra)).encode()
        fact(event="refresh", computer=comp, login=lid, how=how)
        w.write(build("HTTP/1.1 200 OK", [["content-type", "application/json"]], rb, "cl")); return
    if grant == "urn:ietf:params:oauth:grant-type:token-exchange":
        # A credential minted from the login (Codex's API key). The subject is swapped like any
        # placeholder; what comes back is kept by the vault too, and the computer gets a placeholder.
        lid, why = lookup(j.get("subject_token", ""), comp, "id_token")
        if why:
            STATS["refused"] += 1; fact(event="refused", computer=comp, reason=why, where="token exchange")
            w.write(build("HTTP/1.1 400 Bad Request", [["content-type", "application/json"]], b'{"error":"invalid_grant"}', "cl")); return
        if lid is None:
            await upstream_once(host, build(start, hs, body, "cl"), w); return
        from urllib.parse import urlencode
        j = dict(j, subject_token=LOGINS[lid]["id_token"])
        nb = urlencode(j).encode() if form is not None else json.dumps(j).encode()
        code, rhs, rb = await upstream_once(host, build(start, hs, nb, "cl"))
        if code == 200:
            r = json.loads(rb); L = LOGINS[lid]
            L["api_key"] = r["access_token"]; save_state()
            ph = make_ph(r["access_token"]); PH[ph] = {"login": lid, "computer": comp, "kind": "api_key", "retired_at": None}
            r["access_token"] = ph; rb = json.dumps(r).encode()
            fact(event="minted credential kept by Silo", computer=comp, login=lid, kind="api_key")
        w.write(build(f"HTTP/1.1 {code} {'OK' if code == 200 else 'Error'}", [["content-type", "application/json"]], rb, "cl")); return
    await upstream_once(host, build(start, hs, body, "cl"), w)


async def handle_api(comp, host, start, hs, body, framing, w):
    auth = hget(hs, "authorization") or ""
    if auth[:7].lower() == "bearer ":
        tok = auth[7:]
        kind = PH.get(tok, {}).get("kind", "access")
        if kind not in ("access", "api_key"): kind = "access"
        lid, why = lookup(tok, comp, kind)
        if why:
            STATS["refused"] += 1; fact(event="refused", computer=comp, reason=why, host=host, path=start.split()[1])
            err = json.dumps({"type": "error", "error": {"type": "authentication_error", "message": f"Silo: {why}"}}).encode()
            w.write(build("HTTP/1.1 401 Unauthorized", [["content-type", "application/json"]], err, "cl")); return
        if lid:
            L = LOGINS[lid]
            if L["exp"] - time.time() < REFRESH_MARGIN:
                await refresh_upstream(lid, L["token_host"])
            hset(hs, "authorization", "Bearer " + (L["api_key"] if kind == "api_key" else L["access"]))
            STATS["swapped"] += 1
            fact(event="swapped", computer=comp, login=lid, host=host, path=start.split()[1].split("?")[0])
        elif tok:
            fact(event="credential Silo does not hold, passed unchanged", computer=comp, host=host)
    code, _, _ = await upstream_once(host, build(start, hs, body, framing), w, scrub=scrubber)
    if lid and code == 401:
        # The provider ended the real token before its time. The next refresh goes upstream.
        LOGINS[lid]["exp"] = 0; fact(event="the provider refused the real token; it will be refreshed", login=lid)


async def conn(r, w):
    peer = w.get_extra_info("peername")[0]; comp = COMPUTERS.get(peer, "unknown:" + peer)
    sni = SNI.pop(id(w.get_extra_info("ssl_object")), None)
    try:
        while True:
            try:
                start, hs = await read_head(r)
            except (asyncio.IncompleteReadError, ConnectionError):
                return
            host = (hget(hs, "host") or "").split(":")[0]
            if host != sni:
                fact(event="refused", computer=comp, reason=f"Host {host} is not the name of the connection {sni}")
                w.write(build("HTTP/1.1 421 Misdirected Request", [], b"", "cl")); await w.drain(); return
            body, framing = await read_body(r, hs)
            if os.environ.get("CAPTURE"):   # the request as the program framed it, for the parser tests
                CAPN[0] += 1
                raw_body = body if framing != "chunked" else b"%x\r\n%s\r\n0\r\n\r\n" % (len(body), body) if body else b"0\r\n\r\n"
                with open(f"{os.environ['CAPTURE']}/{comp}-{CAPN[0]:04d}-{sni}.bin", "wb") as f:
                    f.write((start + "\r\n" + "".join(f"{k}: {v}\r\n" for k, v in hs) + "\r\n").encode("latin-1") + raw_body)
            path = start.split()[1].split("?")[0]
            if MODE == "pass":
                await upstream_once(host, build(start, hs, body, framing), w)
            elif host in TOKEN_HOSTS and path.endswith("/oauth/token"):
                await handle_token(comp, host, start, hs, body, w)
            elif host in API_HOSTS:
                await handle_api(comp, host, start, hs, body, framing, w)
            else:
                await upstream_once(host, build(start, hs, body, framing), w)
            await w.drain()
    except Bad as e:
        fact(event="refused", computer=comp, reason=str(e))
        w.write(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
    except Exception as e:
        print("error", repr(e), file=sys.stderr, flush=True)
    finally:
        try: w.close()
        except Exception: pass


async def admin(r, w):
    """Silo's own side, on the host only: give a computer the login (a fork the user lets keep it)."""
    try:
        start, hs = await read_head(r); body, _ = await read_body(r, hs)
        j = json.loads(body or b"{}")
        if start.startswith("POST /grant"):
            lid = j.get("login") or next(iter(LOGINS))
            cur = placeholders_for(lid, j["computer"]); L = LOGINS[lid]
            fact(event="login given to a computer", login=lid, computer=j["computer"])
            out = dict(cur, exp=L["exp"], login=lid, provider=L["provider"])
        elif start.startswith("GET /stats"):
            out = STATS
        else:
            out = {"logins": list(LOGINS)}
        b = json.dumps(out).encode()
        w.write(build("HTTP/1.1 200 OK", [["content-type", "application/json"]], b, "cl")); await w.drain()
    finally:
        w.close()


async def main():
    await asyncio.start_server(admin, "127.0.0.1", int(os.environ.get("ADMIN_PORT", "9900")))
    cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); cx.load_cert_chain(os.environ["CERT"], os.environ["KEY"]); cx.set_alpn_protocols(["http/1.1"])
    def on_sni(obj, name, ctx):
        if not name: return ssl.ALERT_DESCRIPTION_UNRECOGNIZED_NAME   # no name, no reading
        SNI[id(obj)] = name
    cx.sni_callback = on_sni
    h, p = LISTEN.rsplit(":", 1)
    srv = await asyncio.start_server(conn, h, int(p), ssl=cx)
    print("vault listening", LISTEN, MODE, file=sys.stderr, flush=True)
    async with srv: await srv.serve_forever()

if __name__ == "__main__":
    asyncio.run(main())
