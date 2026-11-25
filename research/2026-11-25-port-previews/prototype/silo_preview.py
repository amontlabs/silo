"""Silo's preview of a computer's port, as one component: the forwarder, the Silo page around the
app, and one rule proxy per computer for the browser Silo starts for that computer.

  forwarder  :7000   <port>.<computer id>.localhost  ->  that port in that computer
                     silo.<computer id>.localhost/<port>  ->  the Silo page, which holds the app's
                     origin in a sandboxed frame. Each computer is a site of its own.
  rule proxy :7100+  one per computer. The browser Silo starts for that computer sends every request
                     through it. It lets through that computer's own previews and the hosts the
                     computer's rules allow, by name, without resolving any name it denies, and
                     records what it denies.

It is HTTP/1.1 only and opens one upstream connection per request (it answers with
`connection: close`), which is enough to measure and to run real dev servers through.

Settings, from the environment:
  CONFIG      a JSON file: {"computers": {"<id>": {"name": "c1", "ports": {"3000": "127.0.0.1:3100"},
              "allow": ["host:port", ...]}}}
  LOG         a JSON-lines file where every decision is written
  PROXY       "per-computer" (default) or "shared": one proxy that lets every computer's previews
              through, as a single browser for all computers would need
  NAMES       "per-computer" (default) or "one-site" (#363's names, under silo.localhost)
  SANDBOX     the frame's sandbox tokens (default: no allow-modals, no popups, no downloads)
  CSP_WS      "1" adds ws: and wss: to the app's connect-src, as policies written for hot reload do
  HOST        "keep" (default): the app sees Host: <port>.<id>.localhost:7000
              "rewrite": the app sees Host: localhost:<port>, and Location headers are rewritten back
  SET_COOKIE  "keep" (default) or "host-only": the forwarder removes Domain= from the app's cookies
"""
import asyncio, json, os, re, sys, urllib.parse

CONFIG = json.load(open(os.environ["CONFIG"]))
LOG = os.environ.get("LOG", "/dev/null")
PROXY = os.environ.get("PROXY", "per-computer")
HOST = os.environ.get("HOST", "keep")
SET_COOKIE = os.environ.get("SET_COOKIE", "keep")
FWD_PORT = 7000
# NAMES="per-computer" (default): port 3000 of computer c1a7 is 3000.c1a7.localhost, and its Silo
#   page is silo.c1a7.localhost/3000. Each computer is a site of its own (c1a7.localhost).
# NAMES="one-site": as #363 had it, 3000.c1a7.silo.localhost and preview.silo.localhost/c1a7/3000.
#   Every computer's previews are one site, silo.localhost.
NAMES = os.environ.get("NAMES", "per-computer")
COMPUTERS = CONFIG["computers"]


def log(**d):
    with open(LOG, "a") as f:
        f.write(json.dumps(d) + "\n")


def app_host(cid, port):
    return f"{port}.{cid}.localhost" if NAMES == "per-computer" else f"{port}.{cid}.silo.localhost"


def shell_host(cid):
    return f"silo.{cid}.localhost" if NAMES == "per-computer" else "preview.silo.localhost"


def origin(cid, port):
    return f"http://{app_host(cid, port)}:{FWD_PORT}"


def parse_shell(host, path):
    """(computer id, port) of a Silo page, or None."""
    if NAMES == "per-computer":
        m = re.fullmatch(r"silo\.([a-z0-9]+)\.localhost", host)
        p = re.fullmatch(r"/(\d+)", path)
        return (m[1], p[1]) if m and p else None
    m = host == "preview.silo.localhost" and re.fullmatch(r"/([a-z0-9]+)/(\d+)", path)
    return (m[1], m[2]) if m else None


def parse_app(host):
    m = re.fullmatch(r"(\d+)\.([a-z0-9]+)" + (r"\.localhost" if NAMES == "per-computer" else r"\.silo\.localhost"), host)
    return (m[2], m[1]) if m else None


CSP_APP = ("default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; "
           "connect-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; font-src 'self' data:; "
           "frame-src 'self'; child-src 'self' blob:; worker-src 'self' blob:; form-action 'self'; base-uri 'self'; "
           "object-src 'none'; manifest-src 'self'")
PERMISSIONS = ("camera=(), microphone=(), geolocation=(), usb=(), serial=(), hid=(), payment=(), "
               "clipboard-read=(), clipboard-write=(), display-capture=(), fullscreen=(), publickey-credentials-get=(), "
               "publickey-credentials-create=(), local-fonts=(), window-management=()")


if os.environ.get("CSP_WS"):  # what a policy written for dev servers' hot reload tends to say
    CSP_APP = CSP_APP.replace("connect-src 'self'", "connect-src 'self' ws: wss:")


def shell(cid, port, query):
    c = COMPUTERS[cid]
    src = origin(cid, port) + "/" + (("?" + query) if query else "")
    # No script in Silo's page: it only frames the app. allow-modals stays out: a modal from the app
    # is drawn by the browser over Silo's page, so it could pass for Silo asking.
    sandbox = os.environ.get("SANDBOX", "allow-scripts allow-same-origin allow-forms")
    body = f"""<!doctype html><meta charset=utf-8><title>{c['name']} · port {port} · Silo</title>
<style>body{{margin:0;font:13px system-ui}}header{{padding:6px 10px;background:#222;color:#eee}}iframe{{border:0;width:100%;height:90vh}}</style>
<header>Port {port} of computer {c['name']}. What is below comes from the computer.</header>
<iframe src="{src}" sandbox="{sandbox}" allow="" referrerpolicy="no-referrer"></iframe>"""
    csp = (f"default-src 'none'; style-src 'unsafe-inline'; frame-src {origin(cid, port)}; "
           "form-action 'none'; base-uri 'none'; frame-ancestors 'none'")
    return body.encode(), [("content-security-policy", csp), ("x-frame-options", "DENY"),
                           ("cross-origin-opener-policy", "same-origin"), ("referrer-policy", "no-referrer")]


async def read_head(r):
    head = await r.readuntil(b"\r\n\r\n")
    lines = head.decode("latin-1").split("\r\n")[:-2]
    return lines[0], [tuple(x.split(":", 1)) for x in lines[1:]]


class BadHead(Exception):
    """An answer head the forwarder cannot read exactly as the browser will."""


TOKEN = r"[!#$%&'*+.^_`|~0-9A-Za-z-]+"


async def read_answer_head(r):
    """The app's answer head, read strictly, so that the browser reads the same head the forwarder
    adds Silo's policy to. Lines end in CRLF only: a bare LF ends a line, or the head, for Chromium
    and not for a reader of CRLF. No folded line, no line without a name and a colon, no bare CR,
    no NUL, at most 64 KiB. An interim answer (1xx other than 101) is read and dropped: Chromium
    drops it, with any policy written on it, and reads the next head. Anything else raises BadHead."""
    while True:
        try:
            head = await r.readuntil(b"\r\n\r\n")
        except (asyncio.LimitOverrunError, asyncio.IncompleteReadError):
            raise BadHead()
        text = head[:-4]
        bare = text.replace(b"\r\n", b"")
        if b"\n" in bare or b"\r" in bare or b"\0" in text:
            raise BadHead()
        lines = text.decode("latin-1").split("\r\n")
        if not re.fullmatch(r"HTTP/1\.[01] [1-5]\d\d( [^\x00-\x08\x0a-\x1f\x7f]*)?", lines[0]):
            raise BadHead()
        hs = []
        for line in lines[1:]:
            m = re.fullmatch(f"({TOKEN}):(.*)", line)
            if not m:
                raise BadHead()
            hs.append((m[1], m[2]))
        code = int(lines[0][9:12])
        if 100 <= code < 200 and code != 101:
            continue
        return lines[0], hs


def hget(hs, k):
    for a, b in hs:
        if a.strip().lower() == k:
            return b.strip()
    return ""


async def pipe(r, w):
    try:
        while d := await r.read(65536):
            w.write(d)
            await w.drain()
    except (ConnectionError, asyncio.IncompleteReadError):
        pass
    finally:
        try:
            w.close()
        except Exception:
            pass


async def reply(w, code, body=b"", ctype="text/html", hs=()):
    w.write(f"HTTP/1.1 {code} X\r\ncontent-type: {ctype}\r\ncontent-length: {len(body)}\r\nconnection: close\r\n".encode()
            + b"".join(f"{k}: {v}\r\n".encode() for k, v in hs) + b"\r\n" + body)
    await w.drain()
    w.close()


async def forward(r, w):
    try:
        start, hs = await read_head(r)
    except Exception:
        return w.close()
    method, target, _ = start.split(" ", 2)
    host = hget(hs, "host").split(":")[0]
    u = urllib.parse.urlparse(target)
    sh = parse_shell(host, u.path)
    if sh:
        if sh[0] in COMPUTERS and sh[1] in COMPUTERS[sh[0]]["ports"]:
            body, extra = shell(sh[0], sh[1], u.query)
            return await reply(w, 200, body, hs=extra)
        return await reply(w, 404)
    ap = parse_app(host)
    if not ap or ap[0] not in COMPUTERS or ap[1] not in COMPUTERS[ap[0]]["ports"]:
        log(at="forwarder", refused=host)
        return await reply(w, 421, b"not a preview")
    cid, port = ap
    up_host, up_port = COMPUTERS[cid]["ports"][port].rsplit(":", 1)
    try:
        ur, uw = await asyncio.open_connection(up_host, int(up_port))
    except OSError:
        return await reply(w, 502, b"the computer does not answer on this port")
    upgrade = hget(hs, "upgrade").lower() == "websocket"
    out = []
    for k, v in hs:
        kl = k.strip().lower()
        if kl in ("connection", "keep-alive", "proxy-connection"):
            continue
        if HOST == "rewrite" and kl == "host":
            v = f" localhost:{port}"
        if HOST == "rewrite" and kl == "origin":
            v = f" http://localhost:{port}"
        out.append((k, v))
    out.append(("connection", " upgrade" if upgrade else " close"))
    uw.write((start + "\r\n" + "".join(f"{k}:{v}\r\n" for k, v in out) + "\r\n").encode("latin-1"))
    await uw.drain()
    asyncio.ensure_future(pipe(r, uw))
    try:
        rstart, rhs = await read_answer_head(ur)
    except BadHead:
        uw.close()
        return await reply(w, 502, b"the computer's answer could not be read")
    except Exception:
        return w.close()
    keep = []
    for k, v in rhs:
        kl = k.strip().lower()
        if kl in ("connection", "keep-alive", "content-security-policy", "content-security-policy-report-only",
                  "permissions-policy", "x-frame-options"):
            continue
        if kl == "set-cookie" and SET_COOKIE == "host-only":
            v = re.sub(r";\s*domain=[^;]*", "", v, flags=re.I)
        if kl == "location" and HOST == "rewrite":
            v = v.replace(f"http://localhost:{port}", origin(cid, port))
        keep.append((k, v))
    keep += [("connection", " upgrade" if rstart.split(" ")[1] == "101" else " close"),
             ("content-security-policy", " " + CSP_APP),
             ("content-security-policy", f" frame-ancestors http://{shell_host(cid)}:{FWD_PORT}"),
             ("permissions-policy", " " + PERMISSIONS), ("referrer-policy", " no-referrer"),
             ("cross-origin-resource-policy", " same-origin")]
    w.write((rstart + "\r\n" + "".join(f"{k}:{v}\r\n" for k, v in keep) + "\r\n").encode("latin-1"))
    await w.drain()
    await pipe(ur, w)


def allowed_for(cid):
    """What the browser of computer `cid` may reach: its own previews, the Silo page, and the hosts its
    rules allow. With PROXY=shared, every computer's previews."""
    ids = list(COMPUTERS) if cid is None else [cid]
    a = set()
    for i in ids:
        a.add(f"{shell_host(i)}:{FWD_PORT}")
        a |= {f"{app_host(i, p)}:{FWD_PORT}" for p in COMPUTERS[i]["ports"]}
        a |= set(COMPUTERS[i].get("allow", []))
    return a


def rule_proxy(cid):
    allow = allowed_for(cid)

    async def handle(r, w):
        try:
            start, hs = await read_head(r)
        except Exception:
            return w.close()
        method, target, _ = start.split(" ", 2)
        if method == "CONNECT":
            hostport = target
        else:
            u = urllib.parse.urlparse(target)
            hostport = f"{u.hostname}:{u.port or 80}"
        if hostport not in allow:
            # Denied by name. The name is never resolved: a lookup would itself carry what is in it.
            log(at="rule-proxy", computer=cid or "shared", denied=hostport, method=method,
                target=target[:200], ua=hget(hs, "user-agent")[:40])
            return await reply(w, 403, f"denied by the rules of computer {cid}".encode())
        h, p = hostport.rsplit(":", 1)
        dial = ("127.0.0.1", FWD_PORT) if h.endswith(".localhost") else (h, int(p))
        log(at="rule-proxy", computer=cid or "shared", allowed=hostport, method=method)
        try:
            ur, uw = await asyncio.open_connection(*dial)
        except OSError:
            return await reply(w, 502)
        if method == "CONNECT":
            w.write(b"HTTP/1.1 200 OK\r\n\r\n")
            await w.drain()
        else:
            path = urllib.parse.urlparse(target)
            path = (path.path or "/") + ("?" + path.query if path.query else "")
            hs2 = [(k, v) for k, v in hs if k.strip().lower() not in ("proxy-connection", "proxy-authorization")]
            uw.write((f"{method} {path} HTTP/1.1\r\n" + "".join(f"{k}:{v}\r\n" for k, v in hs2) + "\r\n").encode("latin-1"))
            await uw.drain()
        await asyncio.gather(pipe(r, uw), pipe(ur, w))
    return handle


async def main():
    await asyncio.start_server(forward, "127.0.0.1", FWD_PORT)
    if PROXY == "shared":
        await asyncio.start_server(rule_proxy(None), "127.0.0.1", 7100)
    else:
        for i, cid in enumerate(COMPUTERS):
            await asyncio.start_server(rule_proxy(cid), "127.0.0.1", 7100 + i + 1)
    print("ready", flush=True)
    await asyncio.Event().wait()

asyncio.run(main())
