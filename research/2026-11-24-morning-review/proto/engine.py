"""The engine of the prototype: one boundary per computer, the checkpoints, the probe's reports, the
rules and one precaution. It writes the record; it never reads what an agent says about itself.

For each computer it listens on three ports:
  - the model port for Anthropic and the one for OpenAI: the agent programs' base URL points there,
    which stands for Silo ending the encrypted connection at the boundary. Each request is
    forwarded unchanged to the scripted model, and both sides are written down as a model call.
  - the proxy port: every other connection (HTTP and CONNECT). Rules allow, deny or hold it.

It takes a checkpoint of the computer's disk at each model request (so at each turn) and every
CHECKPOINT_EVERY seconds, compares it with the one before from outside, and writes file facts.
It reads the probe's trace (strace, standing in for the probe inside the computer) as reports.
"""
import json, os, re, sys, time, socket, threading, hashlib, shutil, http.server, http.client, select, ssl, urllib.parse
sys.path.insert(0, os.path.dirname(__file__))
import record as R

CFG = json.load(open(os.environ["NIGHT_CONFIG"]))
ROOT = CFG["record"]
PRICES = json.load(open(os.environ.get("SILO_PRICES") or os.path.join(os.path.dirname(__file__), "prices.json")))
MODEL_UPSTREAM = ("127.0.0.1", CFG["model_port"])
CHECKPOINT_EVERY = CFG.get("checkpoint_every", 15)
HOLD_LIMIT = CFG.get("hold_limit", 30)

# A real night (`night.py --real`): the model calls go to each provider's own API. The computers
# only ever hold a placeholder; the boundary puts the real key in, from its own environment
# (`secret-placeholder`). Allowed connections reach the real internet.
UPSTREAM = CFG.get("upstream")
KEYS = {"anthropic": os.environ.get("SILO_KEY_ANTHROPIC"), "openai": os.environ.get("SILO_KEY_OPENAI")}
PLACEHOLDER = "placeholder-not-a-key"
REAL_NET = CFG.get("real_internet", False)
MAX_USD = CFG.get("max_usd")
SPENT = {"usd": 0.0}
SPENT_LOCK = threading.Lock()


def outbound_proxy():
    p = os.environ.get("SILO_OUTBOUND_PROXY") or os.environ.get("HTTPS_PROXY") or os.environ.get("https_proxy")
    return urllib.parse.urlsplit(p) if p else None


def open_upstream(provider):
    """The connection a model call is forwarded on, and the path prefix of the provider's API."""
    if not UPSTREAM:
        return http.client.HTTPConnection(*MODEL_UPSTREAM, timeout=600), ""
    u = urllib.parse.urlsplit(UPSTREAM[provider])
    if u.scheme != "https":
        return http.client.HTTPConnection(u.hostname, u.port or 80, timeout=600), u.path.rstrip("/")
    ctx = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE"))
    px = outbound_proxy()
    if px:
        conn = http.client.HTTPSConnection(px.hostname, px.port or 80, timeout=600, context=ctx)
        conn.set_tunnel(u.hostname, u.port or 443)
    else:
        conn = http.client.HTTPSConnection(u.hostname, u.port or 443, timeout=600, context=ctx)
    return conn, u.path.rstrip("/")


def swap_key(provider, hdrs):
    """The placeholder the program sent becomes the real key, only on its way out."""
    if not UPSTREAM:
        return hdrs
    out = {}
    for k, v in hdrs.items():
        if k.lower() == "x-api-key" and PLACEHOLDER in v:
            v = KEYS[provider] or ""
        elif k.lower() == "authorization" and PLACEHOLDER in v:
            v = f"Bearer {KEYS[provider] or ''}"
        out[k] = v
    return out


def refusal(provider, model):
    """Why a model call is not forwarded, or None. While the night has a budget, a call Silo
    cannot count is refused too (#373, `budget-rule-or-precaution`)."""
    if UPSTREAM and not KEYS.get(provider):
        return f"Silo: no {provider} key was given for this night."
    if MAX_USD is None:
        return None
    if model not in PRICES:
        return f"Silo: {model} has no price, so its spend cannot be counted while the night has a budget of ${MAX_USD}."
    with SPENT_LOCK:
        if SPENT["usd"] >= MAX_USD:
            return f"Silo: the night's budget of ${MAX_USD} is reached (${SPENT['usd']:.2f} counted)."
    return None


def error_body(provider, msg):
    """A 400 in the provider's own error format stops every program and shows the sentence (#369)."""
    if provider == "anthropic":
        return {"type": "error", "error": {"type": "invalid_request_error", "message": msg}}
    return {"error": {"message": msg, "type": "invalid_request_error", "param": None, "code": "silo_refused"}}


# ------------------------------------------------------------------ computers

class Computer:
    def __init__(self, name, spec):
        self.name, self.spec = name, spec
        self.home = spec["home"]
        self.disk = spec["disk"]           # the folder that stands for the computer's disk
        self.w = R.Writer(ROOT, name)
        self.ck_lock = threading.Lock()
        self.cks = []                      # (id, ts, manifest)
        self.last_ck = 0
        self.convs = {}                    # provider -> list of conversations
        self.conv_lock = threading.Lock()
        self.hosts_seen = set(spec.get("hosts_before", []))
        self.alerts = {}                   # (rule) -> last alert fact, for merging
        self.rules = spec.get("rules", [])


COMPUTERS = {}


def ck_dir(c):
    d = os.path.join(ROOT, c.name, "checkpoints"); os.makedirs(d, exist_ok=True)
    os.makedirs(os.path.join(ROOT, "cas"), exist_ok=True)
    return d


def walk(disk):
    m = {}
    os.makedirs(os.path.join(ROOT, "cas"), exist_ok=True)
    for dp, dns, fns in os.walk(disk):
        for fn in fns + [d for d in dns if os.path.islink(os.path.join(dp, d))]:
            p = os.path.join(dp, fn)
            try:
                st = os.lstat(p)
            except FileNotFoundError:
                continue
            if os.path.islink(p):
                m[p] = {"t": "l", "to": os.readlink(p)}
                continue
            try:
                h = hashlib.sha256(open(p, "rb").read()).hexdigest()
            except (FileNotFoundError, PermissionError, IsADirectoryError):
                continue
            cas = os.path.join(ROOT, "cas", h)
            if not os.path.exists(cas):
                try:
                    shutil.copyfile(p, cas)
                except FileNotFoundError:
                    continue
            m[p] = {"t": "f", "size": st.st_size, "sha": h, "mode": st.st_mode & 0o777}
    return m


def checkpoint(c, why, by="silo"):
    """Take a checkpoint of the computer's disk and compare it with the one before."""
    with c.ck_lock:
        ts = time.time()
        m = walk(c.disk)
        n = len(c.cks) + 1
        cid = f"{c.name}@ck{n}"
        json.dump(m, open(os.path.join(ck_dir(c), f"ck{n}.json"), "w"))
        c.w.fact("control", "engine", at=R.at_of(ts), what="checkpoint taken", checkpoint=cid, why=why, by=by, files=len(m))
        changes = []
        if c.cks:
            pid, pts, pm = c.cks[-1]
            for p in sorted(set(pm) | set(m)):
                a, b = pm.get(p), m.get(p)
                if a == b:
                    continue
                if a is None:
                    ch, d = "created", b.get("size", 0)
                elif b is None:
                    ch, d = "deleted", -a.get("size", 0)
                else:
                    ch, d = "changed", b.get("size", 0) - a.get("size", 0)
                f = c.w.fact("file", "checkpoint-compare", at=R.at_of(ts), change=ch, path=p, size_change=d,
                             between=[pid, cid], sha_before=(a or {}).get("sha"), sha_after=(b or {}).get("sha"))
                changes.append(f)
        c.cks.append((cid, ts, m))
        c.last_ck = ts
    if changes:
        file_rules(c, changes, cid)
    return cid


def checkpoint_soon(c, why):
    if time.time() - c.last_ck > 0.5:
        threading.Thread(target=checkpoint, args=(c, why), daemon=True).start()


# ------------------------------------------------------------------ rules (on facts only)

def alert(c, rule, text, about, pins=None, place=None):
    """An alert only on a change, merged with the alert of the same rule within ten minutes."""
    last = c.alerts.get(rule["id"])
    if last and time.time() - R.ts_of(last["at"]) < 600 and rule.get("merge", True):
        return c.w.fact("control", "engine", what="alert merged", into=last["id"], by=f"rule:{rule['id']}@{rule['version']}", about=about, text=text)
    a = c.w.fact("control", "engine", what="alert raised", by=f"{rule.get('kind', 'rule')}:{rule['id']}@{rule['version']}", about=about, text=text, pins=pins, place=place)
    c.alerts[rule["id"]] = a
    return a


def file_rules(c, changes, cid):
    for rule in c.rules:
        if rule["on"] == "files-deleted-outside":
            inside = [os.path.join(c.home, x) for x in rule["inside"]]
            dels = [f for f in changes if f["change"] == "deleted" and not any(f["path"].startswith(i + "/") for i in inside)]
            if len(dels) >= rule["at_least"]:
                top = common_top(c, [f["path"] for f in dels])
                size = -sum(f["size_change"] for f in dels)
                alert(c, rule, f"{len(dels)} files deleted outside ~/{rule['inside'][0]}: {top}",
                      [f["id"] for f in dels], pins=dels[0]["between"][0], place="disk")
        if rule["on"] == "paths-changed":
            watched = [os.path.join(c.home, x) for x in rule["paths"]]
            hit = [f for f in changes if f["path"] in watched]
            for f in hit:
                alert(c, rule, f"{f['change']} {tilde(c, f['path'])}", [f["id"]], place="disk")


def tilde(c, p):
    return "~" + p[len(c.home):] if p.startswith(c.home) else p


def common_top(c, paths):
    parts = os.path.commonpath(paths)
    return tilde(c, parts) + "/"


def connection_rule(c, host):
    for rule in c.rules:
        if rule["on"] == "host" and any(re.fullmatch(h, host) for h in rule["hosts"]):
            return rule
    return None


def new_host(c, host, fact):
    if host in c.hosts_seen:
        return
    c.hosts_seen.add(host)
    for rule in c.rules:
        if rule["on"] == "new-host":
            alert(c, rule, f"new host {host}", [fact["id"]], place="network")


# ------------------------------------------------------------------ who owns a connection (a report)

def owner_of(peer_port, our_port):
    """The process that holds the other end of a local connection, read from /proc. Inside a real
    computer this is the probe's to tell, so it is a report."""
    want_local = f"{peer_port:04X}"
    want_remote = f"{our_port:04X}"
    inode = None
    for f in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            for ln in open(f).read().splitlines()[1:]:
                p = ln.split()
                if p[1].endswith(":" + want_local) and p[2].endswith(":" + want_remote):
                    inode = p[9]; break
        except FileNotFoundError:
            pass
        if inode:
            break
    if not inode:
        return None
    tag = f"socket:[{inode}]"
    for pid in os.listdir("/proc"):
        if not pid.isdigit():
            continue
        try:
            for fd in os.listdir(f"/proc/{pid}/fd"):
                if os.readlink(f"/proc/{pid}/fd/{fd}") == tag:
                    return int(pid)
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
    return None


def owner_report(c, handler, our_port, about):
    pid = owner_of(handler.client_address[1], our_port)
    if pid:
        c.w.report("owner", "probe", pid=pid, about=about, local_port=handler.client_address[1])
    return pid


# ------------------------------------------------------------------ model traffic

def norm(x):
    """A message or an input item as the provider reads it, without the hints a program moves
    from request to request (`recognising-a-conversation`)."""
    if isinstance(x, dict):
        if x.get("type") == "tool_use":
            # Claude Code resends a tool call with its input rewritten (a leading `cd <cwd> &&`
            # dropped), so a call is compared by its id and name, which it keeps.
            return {"type": "tool_use", "id": x.get("id"), "name": x.get("name")}
        return {k: norm(v) for k, v in sorted(x.items()) if k not in ("cache_control", "citations", "id", "status", "signature")}
    if isinstance(x, list):
        return [norm(v) for v in x]
    return x


def hmsg(m):
    return hashlib.sha256(json.dumps(norm(m), sort_keys=True).encode()).hexdigest()[:16]


def find_conversation(c, provider, msgs, model, session=None, tools=None):
    """By content first; failing that, joined to the conversation of the same program session
    with the same tools, and the join is said (`recognising-a-conversation`)."""
    hs = [hmsg(m) for m in msgs]
    with c.conv_lock:
        convs = c.convs.setdefault(provider, [])
        for cv in convs:
            if cv["model"] != model:
                continue
            k = len(cv["req"])
            if hs[:k] == cv["req"] and (len(hs) == k or (cv["reply"] and hs[k:k + len(cv["reply"])] == cv["reply"])):
                cv["req"] = hs
                return cv, None
        if session and len(hs) > 1:
            for cv in reversed(convs):
                if cv["model"] == model and cv.get("session") == session and cv.get("tools") == tools:
                    first = next((i for i, (a, b) in enumerate(zip(hs, cv["req"])) if a != b), min(len(hs), len(cv["req"])))
                    cv["req"] = hs
                    return cv, {"joined_by": "the program's session identifier", "differs_from_message": first + 1}
        cv = {"id": f"{c.name}:c{sum(len(v) for v in c.convs.values()) + 1}", "model": model, "req": hs, "reply": None, "turns": 0, "session": session, "tools": tools}
        convs.append(cv)
        return cv, None


def price(model, tin, tout, cread, cwrite):
    p = PRICES.get(model)
    if not p:
        return None
    return round((tin * p["in"] + tout * p["out"] + cread * p["cache_read"] + cwrite * p["cache_write"]) / 1e6, 6)


def sse_events(raw):
    for chunk in raw.split(b"\n\n"):
        data = [l[5:].strip() for l in chunk.split(b"\n") if l.startswith(b"data:")]
        if data:
            try:
                yield json.loads(b"".join(data))
            except Exception:
                pass


def user_text(provider, first):
    """What you asked: the last plain text of the first user message."""
    texts = []
    if provider == "anthropic":
        cont = first.get("content")
        if isinstance(cont, str):
            texts = [cont]
        else:
            texts = [b.get("text", "") for b in cont if b.get("type") == "text"]
    else:
        for it in first:
            if isinstance(it, dict) and it.get("role") == "user":
                for b in it.get("content", []):
                    if b.get("type") == "input_text":
                        texts.append(b["text"])
    texts = [t for t in texts if t.strip() and not t.lstrip().startswith("<") and not t.startswith("# AGENTS.md")]
    return texts[-1] if texts else None


def model_call(c, provider, req_body, status, resp_raw, ctype, t0, t1, pid_report):
    model = req_body.get("model")
    if provider == "anthropic":
        msgs = req_body.get("messages", [])
        results = []
        if msgs and msgs[-1].get("role") == "user" and isinstance(msgs[-1].get("content"), list):
            for b in msgs[-1]["content"]:
                if b.get("type") == "tool_result":
                    cont = b.get("content")
                    if isinstance(cont, list):
                        cont = "\n".join(x.get("text", "") for x in cont if isinstance(x, dict))
                    results.append({"tool_call": b.get("tool_use_id"), "is_error": bool(b.get("is_error")), "text_ref": c.w.blob(cont)})
        blocks, usage, answered, stop = [], {}, None, None
        if "event-stream" in (ctype or ""):
            cur = {}
            for ev in sse_events(resp_raw):
                t = ev.get("type")
                if t == "message_start":
                    answered = ev["message"].get("model"); usage.update(ev["message"].get("usage", {}))
                elif t == "content_block_start":
                    cur[ev["index"]] = dict(ev["content_block"]); cur[ev["index"]]["_json"] = ""
                elif t == "content_block_delta":
                    d = ev["delta"]
                    if d.get("type") == "text_delta":
                        cur[ev["index"]]["text"] = cur[ev["index"]].get("text", "") + d["text"]
                    elif d.get("type") == "input_json_delta":
                        cur[ev["index"]]["_json"] += d["partial_json"]
                elif t == "content_block_stop":
                    b = cur.pop(ev["index"])
                    if b["type"] == "tool_use":
                        b["input"] = json.loads(b["_json"] or "{}")
                    b.pop("_json", None)
                    blocks.append(b)
                elif t == "message_delta":
                    stop = ev["delta"].get("stop_reason"); usage.update({k: v for k, v in ev.get("usage", {}).items() if v is not None})
        else:
            try:
                j = json.loads(resp_raw)
                blocks, usage, answered, stop = j.get("content", []), j.get("usage", {}), j.get("model"), j.get("stop_reason")
            except Exception:
                pass
        tin, tout = usage.get("input_tokens", 0), usage.get("output_tokens", 0)
        cread, cwrite = usage.get("cache_read_input_tokens", 0) or 0, usage.get("cache_creation_input_tokens", 0) or 0
        reply_msg = {"role": "assistant", "content": blocks}
        hist = msgs
        reply_items = [reply_msg]
        calls = [{"id": b["id"], "tool": b["name"], "input_ref": c.w.blob(b["input"]), "summary": summarize_call(b["name"], b["input"])} for b in blocks if b.get("type") == "tool_use"]
        text = "\n".join(b.get("text", "") for b in blocks if b.get("type") == "text") or None
        first = msgs[0] if msgs else {}
        side = not any(t.get("name") == "Bash" for t in req_body.get("tools", []))
    else:
        inp = req_body.get("input", [])
        if isinstance(inp, str):
            inp = [{"role": "user", "content": [{"type": "input_text", "text": inp}]}]
        results = []
        seen_calls = set()
        for it in inp:
            if isinstance(it, dict) and it.get("type") in ("function_call_output", "custom_tool_call_output"):
                out = it.get("output")
                if isinstance(out, list):
                    out = "\n".join(x.get("text", "") for x in out if isinstance(x, dict))
                results.append({"tool_call": it.get("call_id"), "is_error": None, "text_ref": c.w.blob(out)})
        # only the results new since this conversation's last request are this turn's
        items, usage, answered = [], {}, None
        for ev in sse_events(resp_raw):
            if ev.get("type") == "response.output_item.done":
                items.append(ev["item"])
            if ev.get("type") == "response.completed":
                usage = ev["response"].get("usage", {}); answered = ev["response"].get("model")
        tin = usage.get("input_tokens", 0); cread = (usage.get("input_tokens_details") or {}).get("cached_tokens", 0)
        tin -= cread
        tout, cwrite = usage.get("output_tokens", 0), 0
        hist = inp
        reply_items = items
        calls = []
        for it in items:
            if it.get("type") == "function_call":
                a = json.loads(it.get("arguments") or "{}")
                calls.append({"id": it["call_id"], "tool": it["name"], "input_ref": c.w.blob(a), "summary": summarize_call(it["name"], a)})
        text = "\n".join(b.get("text", "") for it in items if it.get("type") == "message" for b in it.get("content", []) if b.get("type") == "output_text") or None
        stop = "tool_use" if calls else "end_turn"
        first = inp
        side = not any((t.get("name") in ("exec_command", "shell")) for t in req_body.get("tools", []))
    session = (req_body.get("metadata") or {}).get("user_id") if provider == "anthropic" else req_body.get("prompt_cache_key")
    tools_h = hashlib.sha256(json.dumps(sorted(t.get("name") or t.get("type") or "" for t in req_body.get("tools", []))).encode()).hexdigest()[:12]
    cv, joined = find_conversation(c, provider, hist, model, session, tools_h)
    prev_calls = cv.get("calls", set())
    results = [r for r in results if r["tool_call"] in prev_calls] if provider == "openai" else results
    cv["reply"] = [hmsg(m) for m in reply_items]
    cv["turns"] += 1
    cv["calls"] = {x["id"] for x in calls}
    cost = price(answered or model, tin, tout, cread, cwrite) if status == 200 else 0
    if cost:
        with SPENT_LOCK:
            SPENT["usd"] += cost
    asked = user_text(provider, first) if cv["turns"] == 1 else None
    f = c.w.fact("model call", "boundary", at=R.at_of(t1), started=R.at_of(t0), provider=provider, login=CFG["logins"][provider],
                 status=status, model_asked=model, model_answered=answered, side=side or None,
                 tokens={"in": tin, "out": tout, "cache_read": cread, "cache_write": cwrite},
                 cost_usd=cost, price_known=cost is not None,
                 conversation=cv["id"], turn=cv["turns"], stop=stop, joined=joined,
                 you_asked_ref=c.w.blob(asked) if asked else None,
                 reply_text_ref=c.w.blob(text) if text else None,
                 tool_calls=calls or None, results_sent=results or None,
                 messages_ref=[c.w.blob(norm(m)) for m in (hist if isinstance(hist, list) else [])] or None)
    if pid_report:
        c.w.report("owner", "probe", pid=pid_report, about=f["id"])
    return f


def summarize_call(name, inp):
    if not isinstance(inp, dict):
        return name
    for k in ("command", "cmd"):
        if k in inp:
            v = inp[k]
            if isinstance(v, list):
                v = v[-1] if v[:2] == ["bash", "-lc"] else " ".join(v)
            return v
    if "file_path" in inp:
        return f"{name} {inp['file_path']}"
    return name


def model_handler(c, provider, our_port):
    class H(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def do_GET(self):
            self.forward(b"")

        def do_HEAD(self):
            self.forward(b"")

        def do_POST(self):
            n = int(self.headers.get("content-length", 0))
            self.forward(self.rfile.read(n))

        def forward(self, body):
            t0 = time.time()
            pid = owner_of(self.client_address[1], our_port)
            is_call = self.command == "POST" and ("/messages" in self.path or "/responses" in self.path) and "count_tokens" not in self.path
            if is_call:
                checkpoint_soon(c, "turn")
                try:
                    raw = body
                    if (self.headers.get("content-encoding") or "") == "gzip":
                        import gzip; raw = gzip.decompress(body)
                    asked = json.loads(raw).get("model")
                except Exception:
                    asked = None
                why = refusal(provider, asked)
                if why:
                    f = c.w.fact("control", "engine", what="model call refused", by="rule:night-budget@1", provider=provider, model_asked=asked, reason=why)
                    alert(c, {"id": "night-budget", "version": 1}, why, [f["id"]], place="now")
                    b = json.dumps(error_body(provider, why)).encode()
                    self.send_response(400); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b)))
                    self.send_header("connection", "close"); self.end_headers(); self.wfile.write(b)
                    self.close_connection = True
                    return
            up, prefix = open_upstream(provider)
            hdrs = swap_key(provider, {k: v for k, v in self.headers.items() if k.lower() not in ("host", "connection", "accept-encoding")})
            up.request(self.command, prefix + self.path, body=body or None, headers=hdrs)
            r = up.getresponse()
            self.send_response(r.status)
            ctype = r.getheader("content-type")
            for k, v in r.getheaders():
                if k.lower() not in ("transfer-encoding", "connection", "content-length"):
                    self.send_header(k, v)
            stream = "event-stream" in (ctype or "")
            if stream:
                self.send_header("transfer-encoding", "chunked")
            self.send_header("connection", "close")
            data = b""
            if stream:
                self.end_headers()
                while True:
                    chunk = r.read1(65536) if hasattr(r, "read1") else r.read(65536)
                    if not chunk:
                        break
                    data += chunk
                    self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk)); self.wfile.flush()
                self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()
            else:
                data = r.read()
                self.send_header("content-length", str(len(data))); self.end_headers(); self.wfile.write(data)
            self.close_connection = True
            if is_call:
                try:
                    if (self.headers.get("content-encoding") or "") == "gzip":
                        import gzip; body = gzip.decompress(body)
                    req = json.loads(body)
                    model_call(c, provider, req, r.status, data, ctype, t0, time.time(), pid)
                except Exception as e:
                    c.w.fact("gap", "boundary", what="a model call could not be read", error=repr(e)[:200])
    return H


# ------------------------------------------------------------------ the proxy: every other connection

def dial(host, port):
    """A socket to host:port, through this machine's own outbound proxy when it has one."""
    px = outbound_proxy()
    if not px:
        return socket.create_connection((host, port), timeout=30)
    s = socket.create_connection((px.hostname, px.port or 80), timeout=30)
    s.sendall(f"CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n".encode())
    head = b""
    while b"\r\n\r\n" not in head:
        chunk = s.recv(4096)
        if not chunk:
            raise OSError("outbound proxy closed")
        head += chunk
    if b" 200" not in head.split(b"\r\n", 1)[0]:
        s.close()
        raise OSError("outbound proxy refused: " + head.split(b"\r\n", 1)[0].decode(errors="replace"))
    return s


def pipe(a, b):
    """Copy both ways until either side closes. Returns (bytes a->b, bytes b->a)."""
    n = {a: 0, b: 0}
    a.settimeout(None); b.settimeout(None)
    try:
        while True:
            r, _, _ = select.select([a, b], [], [], 600)
            if not r:
                break
            for s in r:
                d = s.recv(65536)
                if not d:
                    return n[a], n[b]
                (b if s is a else a).sendall(d)
                n[s] += len(d)
    except OSError:
        pass
    finally:
        b.close()
    return n[a], n[b]


FAKE_INTERNET = CFG.get("internet", {})


def proxy_handler(c, our_port):
    class H(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def decide(self, host, port, method, path):
            pid = owner_of(self.client_address[1], our_port)
            rule = connection_rule(c, host)
            return pid, rule

        def do_CONNECT(self):
            host, _, port = self.path.partition(":")
            port = int(port or 443)
            t0 = time.time()
            pid, rule = self.decide(host, port, "CONNECT", None)
            if rule and rule["do"] == "deny":
                f = c.w.fact("connection", "boundary", host=host, port=port, decision="denied", decided_by=f"rule:{rule['id']}@{rule['version']}", bytes_out=0, bytes_in=0, opened=False)
                if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
                new_host(c, host, f)
                return self.refuse(f"Silo: {host} is denied by the rule {rule['id']}.")
            if rule and rule["do"] == "hold":
                ask = c.w.fact("control", "engine", what="ask raised", by=f"rule:{rule['id']}@{rule['version']}", text=f"{host}:{port} wants to open a connection", host=host, hold_limit_s=HOLD_LIMIT)
                answer = wait_answer(c, ask["id"], HOLD_LIMIT)
                if answer is None:
                    c.w.fact("control", "engine", what="ask ended", about=[ask["id"]], answer=None, reason=f"nobody answered within the hold limit of {HOLD_LIMIT} s", by="silo")
                    f = c.w.fact("connection", "boundary", at=R.now(), started=R.at_of(t0), host=host, port=port, decision="held, then refused", decided_by=f"rule:{rule['id']}@{rule['version']}", ask=ask["id"], bytes_out=0, bytes_in=0, opened=False)
                    if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
                    new_host(c, host, f)
                    return self.refuse(f"Silo: the request to {host} waited for your answer for {HOLD_LIMIT} s and nobody answered, so it was refused.")
            if REAL_NET:
                return self.tunnel(host, port, pid, t0)
            # allowed: no route to the real internet from this prototype
            f = c.w.fact("connection", "boundary", host=host, port=port, decision="allowed", decided_by=None, bytes_out=0, bytes_in=0, opened=False, note="no fake server for this host")
            if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
            new_host(c, host, f)
            self.refuse("no route in this prototype", 502)

        def tunnel(self, host, port, pid, t0):
            """A real night: an allowed connection is opened for real; its bytes are counted when it closes."""
            try:
                up = dial(host, port)
            except OSError as e:
                f = c.w.fact("connection", "boundary", host=host, port=port, decision="allowed", decided_by=None, bytes_out=0, bytes_in=0, opened=False, note=f"could not connect: {e!r}"[:200])
                if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
                new_host(c, host, f)
                return self.refuse(f"Silo: could not reach {host}:{port}.", 502)
            f = c.w.fact("connection", "boundary", host=host, port=port, decision="allowed", decided_by=None, opened=True, note="bytes in the fact `connection closed` about this one")
            if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
            new_host(c, host, f)
            self.wfile.write(b"HTTP/1.1 200 Connection established\r\n\r\n"); self.wfile.flush()
            sent, got = pipe(self.connection, up)
            c.w.fact("connection closed", "boundary", about=f["id"], host=host, bytes_out=sent, bytes_in=got, seconds=round(time.time() - t0, 1))
            self.close_connection = True

        def refuse(self, msg, code=403):
            b = (msg + "\n").encode()
            self.send_response(code); self.send_header("content-type", "text/plain"); self.send_header("content-length", str(len(b))); self.send_header("connection", "close"); self.end_headers(); self.wfile.write(b)
            self.close_connection = True

        def plain(self):
            u = urllib.parse.urlsplit(self.path)
            host = u.hostname
            n = int(self.headers.get("content-length", 0) or 0)
            body = self.rfile.read(n) if n else b""
            pid, rule = self.decide(host, u.port or 80, self.command, u.path)
            if rule and rule["do"] == "deny":
                f = c.w.fact("connection", "boundary", host=host, port=u.port or 80, decision="denied", decided_by=f"rule:{rule['id']}@{rule['version']}", method=self.command, path=u.path, bytes_out=len(body), bytes_in=0, opened=True)
                if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
                new_host(c, host, f)
                return self.refuse(f"Silo: {host} is denied by the rule {rule['id']}.")
            site = FAKE_INTERNET.get(host, {})
            page = site.get(u.path)
            if page is None:
                status, out = 404, b"not found\n"
            else:
                status, out = 200, open(page, "rb").read()
            f = c.w.fact("connection", "boundary", host=host, port=u.port or 80, decision="allowed", decided_by=None, method=self.command, path=u.path, status=status,
                         bytes_out=len(body) + len(str(self.headers)), bytes_in=len(out), opened=True, sent_ref=c.w.blob(body.decode(errors="replace")) if body else None,
                         received_ref=c.w.blob(out.decode(errors="replace")) if len(out) < 200000 else None)
            if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
            new_host(c, host, f)
            self.send_response(status); self.send_header("content-length", str(len(out))); self.send_header("connection", "close"); self.end_headers(); self.wfile.write(out)
            self.close_connection = True

        do_GET = do_POST = do_PUT = do_HEAD = plain
    return H


def wait_answer(c, ask_id, limit):
    p = os.path.join(ROOT, c.name, "answers", ask_id.replace(":", "_"))
    end = time.time() + limit
    while time.time() < end:
        if os.path.exists(p):
            return open(p).read().strip()
        time.sleep(0.5)
    return None


# ------------------------------------------------------------------ the probe's trace, as reports

STRACE_RE = re.compile(r"^(\d+)\s+(\d+\.\d+)\s+(.*)$")


def str_args(s):
    """The string arguments of one strace call, in order."""
    return [bytes(m, "utf-8").decode("unicode_escape", errors="replace") for m in re.findall(r'"((?:[^"\\]|\\.)*)"', s)]


class Probe:
    """Reads one launch's trace while it is written."""

    def __init__(self, c, path, launch):
        self.c, self.path, self.launch = c, path, launch
        self.pending = {}
        self.cwd = {}
        self.fds = {}
        self.pos = 0
        self.buf = ""

    def poll(self):
        try:
            with open(self.path) as f:
                f.seek(self.pos); data = f.read(); self.pos = f.tell()
        except FileNotFoundError:
            return
        self.buf += data
        *lines, self.buf = self.buf.split("\n")
        for ln in lines:
            self.line(ln)

    def path_of(self, pid, dirfd, p):
        m = re.match(r"/proc/self/fd/(\d+)(/.*)?$", p)
        if m and m.group(1) in self.fds:
            return os.path.normpath(self.fds[m.group(1)] + (m.group(2) or ""))
        if p.startswith("/"):
            return os.path.normpath(p)
        if dirfd and dirfd != "AT_FDCWD":
            m = re.search(r"<(.*)>", dirfd)
            if m:
                return os.path.normpath(os.path.join(m.group(1), p))
        return os.path.normpath(os.path.join(self.cwd.get(pid, self.launch["cwd"]), p))

    def line(self, ln):
        m = STRACE_RE.match(ln)
        if not m:
            return
        pid, ts, rest = int(m.group(1)), float(m.group(2)), m.group(3)
        at = R.at_of(ts)
        if rest.endswith("<unfinished ...>"):
            self.pending[pid] = rest[: -len("<unfinished ...>")]
            return
        if rest.startswith("<..."):
            head = self.pending.pop(pid, "")
            rest = head + rest.split("resumed>", 1)[1]
        w = self.c.w
        if rest.startswith("+++ exited with"):
            code = int(rest.split()[3])
            w.report("process", "probe", at=at, event="exited", pid=pid, exit=code)
            return
        if rest.startswith("+++ killed by"):
            w.report("process", "probe", at=at, event="exited", pid=pid, signal=rest.split()[3])
            return
        call = rest.split("(", 1)[0]
        ret = rest.rsplit("=", 1)[-1].strip().split(" ")[0] if "=" in rest else ""
        if call in ("clone", "clone3", "fork", "vfork"):
            if ret.isdigit() and int(ret) > 0:
                child = int(ret)
                self.cwd[child] = self.cwd.get(pid, self.launch["cwd"])
                w.report("process", "probe", at=at, event="forked", pid=child, parent=pid, thread=True if "CLONE_THREAD" in rest else None)
            return
        if call == "execve":
            if not ret.startswith("0"):
                return
            args = str_args(rest)
            argv = args[1:] if len(args) > 1 else args
            first = self.launch.get("pid") is None
            if first:
                self.launch["pid"] = pid
            w.report("process", "probe", at=at, event="started", pid=pid, program=args[0] if args else None,
                     command=" ".join(argv)[:2000], cwd=self.cwd.get(pid, self.launch["cwd"]),
                     launch=self.launch["id"] if first else None)
            return
        if call in ("chdir",):
            if ret == "0":
                a = str_args(rest)
                if a:
                    self.cwd[pid] = self.path_of(pid, None, a[0])
            return
        if not ret or ret.startswith("-1"):
            return
        a = str_args(rest)
        if call in ("openat", "open", "creat", "openat2"):
            fdm = re.search(r"=\s*(\d+)<(.*)>\s*$", rest)
            if fdm:
                self.fds[fdm.group(1)] = fdm.group(2)   # -y names the file an fd stands for
            if call != "creat" and not re.search(r"O_WRONLY|O_RDWR|O_CREAT|O_TRUNC", rest):
                return
            if fdm:
                p = fdm.group(2)
            else:
                dirfd = rest.split("(", 1)[1].split(",", 1)[0] if call in ("openat", "openat2") else None
                p = self.path_of(pid, dirfd, a[0]) if a else None
            if p and p.startswith(self.c.disk):
                w.report("file", "probe", at=at, op="wrote", pid=pid, path=p)
        elif call in ("link", "linkat", "symlink", "symlinkat"):
            if len(a) >= 2:
                parts = rest.split("(", 1)[1].split(",")
                dst = self.path_of(pid, parts[2].strip() if call == "linkat" else (parts[1].strip() if call == "symlinkat" else None), a[1])
                if dst.startswith(self.c.disk):
                    w.report("file", "probe", at=at, op="linked", pid=pid, path=dst)
        elif call in ("unlink", "unlinkat", "rmdir"):
            dirfd = rest.split("(", 1)[1].split(",", 1)[0] if call == "unlinkat" else None
            p = self.path_of(pid, dirfd, a[0]) if a else None
            if p and p.startswith(self.c.disk):
                w.report("file", "probe", at=at, op="removed", pid=pid, path=p)
        elif call in ("rename", "renameat", "renameat2"):
            parts = rest.split("(", 1)[1].split(",")
            if len(a) >= 2:
                src = self.path_of(pid, parts[0] if call != "rename" else None, a[0])
                dst = self.path_of(pid, parts[2].strip() if call != "rename" else None, a[1])
                if dst.startswith(self.c.disk):
                    w.report("file", "probe", at=at, op="renamed", pid=pid, path=dst, from_path=src)
        elif call in ("mkdir", "mkdirat"):
            pass


PROBES = []


def probe_loop():
    while True:
        for p in list(PROBES):
            p.poll()
        time.sleep(0.3)


# ------------------------------------------------------------------ control: launches, forks, put back, answers

def control_handler():
    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_POST(self):
            n = int(self.headers.get("content-length", 0)); req = json.loads(self.rfile.read(n) or b"{}")
            out = control(req)
            b = json.dumps(out).encode()
            self.send_response(200); self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)
    return H


def control(req):
    op = req["op"]
    if op == "probe":
        c = COMPUTERS[req["computer"]]
        launch = {"id": req["launch"], "cwd": req["cwd"], "pid": None}
        PROBES.append(Probe(c, req["trace"], launch))
        return {"ok": True}
    if op == "checkpoint":
        c = COMPUTERS[req["computer"]]
        return {"checkpoint": checkpoint(c, req.get("why", "asked"), by=req.get("by", "you"))}
    if op == "fork":
        src = COMPUTERS[req["from"]]
        ck = checkpoint(src, "before a fork", by="you")
        spec = CFG["computers"][req["name"]]
        shutil.copytree(src.disk, spec["disk"], symlinks=True)
        # the fork's disk is a copy of the parent's at that checkpoint; its paths move with it
        for dp, dns, fns in os.walk(spec["disk"]):
            pass
        nc = start_computer(req["name"], spec, hosts_before=src.hosts_seen)
        src.w.fact("control", "engine", what="fork made", by="you", fork=req["name"], from_checkpoint=ck)
        nc.w.fact("control", "engine", what="made as a fork", by="you", parent=src.name, from_checkpoint=ck)
        checkpoint(nc, "first", by="silo")
        return {"ok": True, "checkpoint": ck}
    if op == "stop":
        for c in COMPUTERS.values():
            checkpoint(c, "end of the night", by="silo")
        return {"ok": True}
    return {"error": "unknown op"}


# ------------------------------------------------------------------ the one precaution: stuck

def precaution_loop():
    sys.path.insert(0, os.path.dirname(__file__))
    import classify
    done = set()
    while True:
        time.sleep(5)
        for c in list(COMPUTERS.values()):
            for rule in c.rules:
                if rule["on"] != "stuck":
                    continue
                try:
                    view = classify.View(ROOT, live=True)
                except Exception:
                    continue
                for run in view.runs_of(c.name):
                    st = view.state(run)
                    if st["state"] == "stuck" and st["for_s"] >= rule["after_s"] and run["id"] not in done:
                        done.add(run["id"])
                        o = c.w.opinion(f"{classify.HOW_GOING}", [run["id"]], {"state": "stuck", "since": st["since"]}, st["rests_on"])
                        alert(c, {**rule, "merge": False}, f"{run['name']} stuck: no model call for {int(st['for_s'])} s while `{st['command']}` runs", [o["id"]], place="now")


def interval_loop():
    while True:
        time.sleep(CHECKPOINT_EVERY)
        for c in list(COMPUTERS.values()):
            if time.time() - c.last_ck >= CHECKPOINT_EVERY - 0.5:
                checkpoint(c, "interval")


class TS(http.server.ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def serve(port, handler):
    s = TS(("127.0.0.1", port), handler)
    threading.Thread(target=s.serve_forever, daemon=True).start()


def start_computer(name, spec, hosts_before=()):
    c = Computer(name, spec)
    c.hosts_seen |= set(hosts_before)
    COMPUTERS[name] = c
    p = spec["ports"]
    serve(p["anthropic"], model_handler(c, "anthropic", p["anthropic"]))
    serve(p["openai"], model_handler(c, "openai", p["openai"]))
    serve(p["proxy"], proxy_handler(c, p["proxy"]))
    return c


if __name__ == "__main__":
    os.makedirs(ROOT, exist_ok=True)
    for name, spec in CFG["computers"].items():
        if not spec.get("fork_of"):
            c = start_computer(name, spec)
            c.w.fact("control", "engine", what="computer started", by="you")
            checkpoint(c, "first")
    for f in (probe_loop, precaution_loop, interval_loop):
        threading.Thread(target=f, daemon=True).start()
    s = TS(("127.0.0.1", CFG["control_port"]), control_handler())
    print("engine ready", flush=True)
    s.serve_forever()
