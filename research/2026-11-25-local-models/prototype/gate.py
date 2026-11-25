"""The boundary's route to a model server on the Mac: what a computer reaches when it is granted
"a model on your Mac". It runs on the Mac, in the boundary, beside the network stack.

One listener per grant. In Silo, the network stack would map an address inside the computer (here
named models.silo.internal) to the grant's listener, so the gate knows the computer from the
listener and not from anything the computer says. Here each grant is a port on 127.0.0.1.

For each request it:
  1. reads it strictly (one Content-Length or chunked, never both; no folded or duplicate framing
     headers; bounded sizes), or refuses it;
  2. lets through only the routes that run or list a model, per server, and refuses every route
     that downloads, deletes, creates, loads or pushes one;
  3. reads the JSON body strictly (no duplicate keys), refuses a cloud model of Ollama's (it would
     run on ollama.com under the Mac's own sign-in, outside the computer's rules), and caps how long
     a model stays loaded and how large a context it is given;
  4. sends it to the server on the Mac's loopback with the Host that server expects and no Origin;
  5. streams the reply back unchanged while it reads the model's usage from it, in whichever of the
     four formats the route speaks, and writes one record per call.

A refusal is a 400 in the shape of the route's format, with Silo's sentence, because that is the
status every program #369 tried stops on and shows.

Settings, from the environment:
  GRANTS  a JSON file: {"<listen port>": {"computer": "c1", "server": "ollama", "to": 11434,
          "models": ["qwen3-coder:30b"] or null for any local model}}
  LOG     the record, JSON lines
usage: python3 gate.py"""
import asyncio, json, os, re, time

GRANTS = json.load(open(os.environ["GRANTS"]))
LOG = os.environ.get("LOG", "/dev/null")
MAX_HEAD, MAX_BODY = 64 * 1024, 32 * 1024 * 1024
KEEP_ALIVE_MAX_S = 300          # a model a computer asked for stays loaded at most 5 minutes
NUM_CTX_MAX = 65536             # and gets at most this context, whatever the request says

# Per server: the routes that run or list a model, and the format each one speaks.
ROUTES = {
    "ollama": {("POST", "/api/chat"): "ollama", ("POST", "/api/generate"): "ollama",
               ("POST", "/api/embed"): "ollama", ("POST", "/api/embeddings"): "ollama",
               ("POST", "/api/show"): "info", ("GET", "/api/tags"): "info", ("GET", "/api/version"): "info",
               ("GET", "/api/ps"): "info",
               ("POST", "/v1/chat/completions"): "chat", ("POST", "/v1/completions"): "chat",
               ("POST", "/v1/embeddings"): "chat", ("GET", "/v1/models"): "info",
               ("POST", "/v1/responses"): "responses", ("POST", "/v1/messages"): "messages"},
    "lmstudio": {("POST", "/v1/chat/completions"): "chat", ("POST", "/v1/completions"): "chat",
                 ("POST", "/v1/embeddings"): "chat", ("GET", "/v1/models"): "info",
                 ("POST", "/v1/responses"): "responses", ("POST", "/v1/messages"): "messages",
                 ("GET", "/api/v0/models"): "info", ("POST", "/api/v0/chat/completions"): "chat",
                 ("GET", "/api/v1/models"): "info", ("POST", "/api/v1/chat"): "chat"},
}
SENTENCE = "Silo: {why}"


def record(**d):
    d["at"] = round(time.time(), 3)
    with open(LOG, "a") as f:
        f.write(json.dumps(d) + "\n")


def refusal(fmt, why):
    """A 400 in the shape of the route's format."""
    msg = SENTENCE.format(why=why)
    if fmt == "messages":
        body = {"type": "error", "error": {"type": "invalid_request_error", "message": msg}}
    elif fmt in ("chat", "responses"):
        body = {"error": {"message": msg, "type": "invalid_request_error", "param": None, "code": None}}
    else:
        body = {"error": msg}
    b = json.dumps(body).encode()
    return (b"HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\nconnection: close\r\n"
            b"content-length: %d\r\n\r\n%s" % (len(b), b))


class Refused(Exception):
    pass


async def read_request(r):
    head = await r.readuntil(b"\r\n\r\n")
    if len(head) > MAX_HEAD:
        raise Refused("the request's headers are too large")
    lines = head[:-4].split(b"\r\n")
    m = re.fullmatch(rb"(GET|POST|DELETE|HEAD|PUT) (/[!-~]*) HTTP/1\.1", lines[0])
    if not m:
        raise Refused("the request line is not one Silo reads")
    hs = []
    for ln in lines[1:]:
        if ln[:1] in (b" ", b"\t") or b":" not in ln:
            raise Refused("a header is folded or malformed")
        k, v = ln.split(b":", 1)
        if not re.fullmatch(rb"[!#$%&'*+.^_`|~0-9A-Za-z-]+", k):
            raise Refused("a header name is malformed")
        hs.append((k.decode().lower(), v.strip().decode("latin-1")))
    names = [k for k, _ in hs]
    for k in ("content-length", "transfer-encoding", "host"):
        if names.count(k) > 1:
            raise Refused(f"the request has {k} twice")
    h = dict(hs)
    te, cl = h.get("transfer-encoding"), h.get("content-length")
    if te is not None and cl is not None:
        raise Refused("the request has both a length and chunks")
    if te is not None and te.lower() != "chunked":
        raise Refused("the request's transfer-encoding is not chunked")
    body = b""
    if cl is not None:
        if not re.fullmatch(r"\d{1,9}", cl) or int(cl) > MAX_BODY:
            raise Refused("the request's length is not one Silo reads")
        body = await r.readexactly(int(cl))
    elif te is not None:
        while True:
            size_line = await r.readuntil(b"\r\n")
            if not re.fullmatch(rb"[0-9a-fA-F]{1,8}\r\n", size_line):
                raise Refused("a chunk size is malformed")
            n = int(size_line[:-2], 16)
            if len(body) + n > MAX_BODY:
                raise Refused("the request is too large")
            body += await r.readexactly(n)
            if await r.readexactly(2) != b"\r\n":
                raise Refused("a chunk is malformed")
            if n == 0:
                break
    return m[1].decode(), m[2].decode(), h, body


def strict_json(b):
    def pairs(ps):
        keys = [k for k, _ in ps]
        if len(keys) != len(set(keys)):
            raise Refused("the request's JSON has a key twice")
        return dict(ps)
    try:
        return json.loads(b, object_pairs_hook=pairs) if b else {}
    except ValueError:
        raise Refused("the request's body is not JSON")


def seconds(v):
    """Ollama's keep_alive: a number of seconds, or a duration such as "10m", "1h", "-1"."""
    if isinstance(v, (int, float)):
        return v
    m = re.fullmatch(r"(-?\d+(?:\.\d+)?)(ms|s|m|h)?", str(v).strip())
    if not m:
        return None
    return float(m[1]) * {"ms": 0.001, "s": 1, "m": 60, "h": 3600, None: 1}[m[2]]


REMOTE = {}  # model name -> where Ollama on the Mac runs it, from its own /api/show; None when local


async def remote_host(grant, model):
    """Ollama says in /api/show when a model runs elsewhere (remote_host). A model made with
    `ollama create` FROM a cloud model has a name of its own, so the name alone is not enough."""
    if grant["server"] != "ollama" or not model:
        return None
    if model not in REMOTE:
        try:
            r, w = await asyncio.open_connection("127.0.0.1", grant["to"])
            b = json.dumps({"model": model}).encode()
            w.write(b"POST /api/show HTTP/1.1\r\nhost: 127.0.0.1:%d\r\ncontent-type: application/json\r\n"
                    b"connection: close\r\ncontent-length: %d\r\n\r\n%s" % (grant["to"], len(b), b))
            await w.drain()
            resp = await r.read()
            w.close()
            body = resp.split(b"\r\n\r\n", 1)[1]
            info = json.loads(body) if body.startswith(b"{") else {}
            REMOTE[model] = info.get("remote_host") if isinstance(info, dict) else None
        except (OSError, ValueError, IndexError):
            return None
    return REMOTE[model]


def police(grant, fmt, body):
    """What the gate changes in the body, as a list of (field, from, to); raises Refused."""
    changes = []
    model = body.get("model")
    if fmt != "info" or model is not None:
        if model is not None and not isinstance(model, str):
            raise Refused("the model is not named")
        if model and re.search(r"(^|[:-])cloud$", model):
            raise Refused(f"{model} is a cloud model: it would run on ollama.com under your Mac's own sign-in, "
                          "outside this computer's rules")
        if model and grant.get("models") is not None and model not in grant["models"]:
            raise Refused(f"{model} is not among the models this computer may use on your Mac")
    if "keep_alive" in body:
        s = seconds(body["keep_alive"])
        if s is None or s < 0 or s > KEEP_ALIVE_MAX_S:
            changes.append(("keep_alive", body["keep_alive"], f"{KEEP_ALIVE_MAX_S}s"))
            body["keep_alive"] = f"{KEEP_ALIVE_MAX_S}s"
    if "ttl" in body:  # LM Studio's idle time for a model it loads on demand
        if not isinstance(body["ttl"], (int, float)) or body["ttl"] < 0 or body["ttl"] > KEEP_ALIVE_MAX_S:
            changes.append(("ttl", body["ttl"], KEEP_ALIVE_MAX_S))
            body["ttl"] = KEEP_ALIVE_MAX_S
    opts = body.get("options")
    if isinstance(opts, dict) and "num_ctx" in opts:
        if not isinstance(opts["num_ctx"], int) or opts["num_ctx"] > NUM_CTX_MAX:
            changes.append(("options.num_ctx", opts["num_ctx"], NUM_CTX_MAX))
            opts["num_ctx"] = NUM_CTX_MAX
    return changes


class Usage:
    """Reads the model's usage out of the reply as it streams, without holding it back."""
    def __init__(self, fmt):
        self.fmt, self.buf, self.raw, self.chunked, self.head = fmt, b"", b"", None, None
        self.input = self.output = None

    def feed(self, data):
        self.raw += data
        if self.head is None:
            if b"\r\n\r\n" not in self.raw:
                return
            self.head, self.raw = self.raw.split(b"\r\n\r\n", 1)
            self.chunked = b"transfer-encoding: chunked" in self.head.lower()
        if self.chunked:
            while True:
                i = self.raw.find(b"\r\n")
                if i < 0:
                    return
                n = int(self.raw[:i] or b"0", 16)
                if len(self.raw) < i + 2 + n + 2:
                    return
                self.lines(self.raw[i + 2:i + 2 + n])
                self.raw = self.raw[i + 2 + n + 2:]
                if n == 0:
                    return
        else:
            self.lines(self.raw)
            self.raw = b""

    def lines(self, data):
        self.buf += data
        *done, self.buf = self.buf.split(b"\n")
        for ln in done:
            self.line(ln.strip())

    def end(self):
        if self.buf.strip():
            self.line(self.buf.strip())

    def line(self, ln):
        if ln.startswith(b"data:"):
            ln = ln[5:].strip()
        if not ln.startswith(b"{"):
            return
        try:
            o = json.loads(ln)
        except ValueError:
            return
        if not isinstance(o, dict):
            return
        if "prompt_eval_count" in o or "eval_count" in o:                      # Ollama's own lines
            self.input, self.output = o.get("prompt_eval_count"), o.get("eval_count")
        d = lambda v: v if isinstance(v, dict) else {}
        u = o.get("usage") or d(o.get("response")).get("usage") or d(o.get("message")).get("usage")
        if isinstance(u, dict):
            if "prompt_tokens" in u:                                            # Chat Completions
                self.input, self.output = u["prompt_tokens"], u.get("completion_tokens")
            if "input_tokens" in u:                                             # Responses, Messages
                self.input = u["input_tokens"]
            if "output_tokens" in u:
                self.output = u["output_tokens"]


def grant_handler(port, grant):
    async def handle(r, w):
        t0 = time.time()
        fmt = "info"
        try:
            method, target, h, body = await read_request(r)
            path = target.split("?")[0]
            fmt = ROUTES.get(grant["server"], {}).get((method, path))
            if fmt is None:
                raise Refused(f"{method} {path} is not a route that runs a model; a computer may only use the models "
                              "already on your Mac")
            req = strict_json(body) if body else {}
            changes = police(grant, fmt, req) if isinstance(req, dict) else []
            if fmt != "info" and isinstance(req, dict) and (where := await remote_host(grant, req.get("model"))):
                raise Refused(f"{req.get('model')} runs on {where}, not on your Mac: it would use your Mac's own sign-in, "
                              "outside this computer's rules")
        except Refused as e:
            record(computer=grant["computer"], server=grant["server"], refused=str(e),
                   route=locals().get("method", "?") + " " + locals().get("target", "?"))
            w.write(refusal(fmt or "chat", str(e)))
            await w.drain()
            return w.close()
        except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, ConnectionError):
            return w.close()
        out = json.dumps(req).encode() if body else b""
        try:
            ur, uw = await asyncio.open_connection("127.0.0.1", grant["to"])
        except OSError:
            w.write(refusal(fmt, "the model server on your Mac is not running"))
            await w.drain()
            return w.close()
        keep = [(k, v) for k, v in h.items() if k not in ("host", "origin", "referer", "content-length", "transfer-encoding",
                                                          "connection", "keep-alive", "proxy-connection", "cookie")]
        uw.write((f"{method} {target} HTTP/1.1\r\nhost: 127.0.0.1:{grant['to']}\r\nconnection: close\r\n"
                  + "".join(f"{k}: {v}\r\n" for k, v in keep)
                  + (f"content-length: {len(out)}\r\n" if body else "") + "\r\n").encode("latin-1") + out)
        await uw.drain()
        u = Usage(fmt)
        first = None
        try:
            while d := await ur.read(65536):
                first = first or time.time()
                u.feed(d)
                w.write(d)
                await w.drain()
        except ConnectionError:
            pass
        u.end()
        status = (u.head or b" ").split(b" ")[1].decode() if u.head else "none"
        if fmt != "info":
            record(computer=grant["computer"], server=grant["server"], route=f"{method} {path}", format=fmt,
                   model=req.get("model"), status=status, input_tokens=u.input, output_tokens=u.output,
                   stream=bool(req.get("stream", grant["server"] == "ollama" and path.startswith("/api/"))),
                   changed=changes or None, first_byte_ms=round(1000 * ((first or t0) - t0), 1),
                   ms=round(1000 * (time.time() - t0), 1))
        w.close()
        uw.close()
    return handle


async def main():
    for port, g in GRANTS.items():
        await asyncio.start_server(grant_handler(int(port), g), "127.0.0.1", int(port), limit=MAX_HEAD + 1)
    print("ready", flush=True)
    await asyncio.Event().wait()

asyncio.run(main())
