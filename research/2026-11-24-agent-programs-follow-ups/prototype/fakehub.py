"""A fake of every model provider at once, for agent programs run in a private network namespace.

- DNS on 127.0.0.1:53 answers 127.0.0.1 for every name and logs the name asked.
- TLS on :443 makes a certificate for whatever name the client sends (SNI), signed by a test
  authority. It peeks at the ClientHello first and logs SNI and the ALPN list the client offers.
  It speaks HTTP/1.1 only, so a client that insists on h2 shows up as a failed handshake.
- Plain HTTP on :80 and :11434 (Ollama's port).
- Each request is logged to $OUT/requests.jsonl: host, method, path, headers (credentials reduced
  to their kind and length), body (decompressed), and what was answered.
- Model formats answered: Anthropic Messages, OpenAI Chat Completions and Responses, Gemini
  generateContent, Ollama /api/chat. Anything else gets a 404 that is logged.

SCENARIO (env): ok | deny | hold:<seconds> | err:<status>
  ok    the model asks once for the program's shell tool with `echo silo-ok > proof.txt`, then ends.
  deny  the model asks for `rm existing.txt`; the stand-in probe (sprobe) is armed before the reply
        and disarmed on the next model request.
  hold  the first model request is held N seconds before any byte; the client giving up is logged.
  err   the first model request gets that status with a body naming Silo; later ones go through.
"""
import gzip, json, os, re, select, socket, ssl, struct, sys, threading, time, uuid, zlib, datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from socketserver import ThreadingMixIn, TCPServer
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
CA_DIR = os.environ["CA_DIR"]
SCEN = os.environ.get("SCENARIO", "ok")
WORK = os.environ["WORK"]
FIFO = os.environ.get("PROBE_FIFO")
lock = threading.Lock()
T0 = time.time()
REQF = open(f"{OUT}/requests.jsonl", "a"); LOGF = open(f"{OUT}/hub.log", "a")
state = {"main": 0, "armed": False, "held": False, "erred": False}


def log(s):
    with lock:
        LOGF.write(f"{time.time()-T0:8.3f} {s}\n"); LOGF.flush()


def rec(d):
    d["t"] = round(time.time() - T0, 3)
    with lock:
        REQF.write(json.dumps(d) + "\n"); REQF.flush()


# ---------- the test authority and per-name certificates ----------
def load_ca():
    key = serialization.load_pem_private_key(open(f"{CA_DIR}/ca.key", "rb").read(), None)
    cert = x509.load_pem_x509_certificate(open(f"{CA_DIR}/ca.crt", "rb").read())
    return key, cert


CA_KEY, CA_CERT = load_ca()
LEAF_KEY = ec.generate_private_key(ec.SECP256R1())
LEAF_KEY_PEM = LEAF_KEY.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())
ctx_cache = {}


def ctx_for(name):
    name = name or "localhost"
    if name in ctx_cache:
        return ctx_cache[name]
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (x509.CertificateBuilder().subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, name)]))
            .issuer_name(CA_CERT.subject).public_key(LEAF_KEY.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(days=1)).not_valid_after(now + datetime.timedelta(days=30))
            .add_extension(x509.SubjectAlternativeName([x509.DNSName(name)]), False)
            .add_extension(x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.SERVER_AUTH]), False)
            .sign(CA_KEY, hashes.SHA256()))
    p = f"{OUT}/.leaf-{name}.pem"
    open(p, "wb").write(cert.public_bytes(serialization.Encoding.PEM) + CA_CERT.public_bytes(serialization.Encoding.PEM))
    kp = f"{OUT}/.leaf.key"; open(kp, "wb").write(LEAF_KEY_PEM)
    c = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); c.load_cert_chain(p, kp); c.set_alpn_protocols(["http/1.1"])
    ctx_cache[name] = c
    return c


def parse_client_hello(b):
    """SNI and ALPN offered, from the first TLS record."""
    sni, alpn = None, []
    try:
        if b[0] != 0x16:
            return None, None
        p = 5 + 4 + 2 + 32
        p += 1 + b[p]
        p += 2 + struct.unpack(">H", b[p:p+2])[0]
        p += 1 + b[p]
        end = p + 2 + struct.unpack(">H", b[p:p+2])[0]; p += 2
        while p + 4 <= end and p + 4 <= len(b):
            t, l = struct.unpack(">HH", b[p:p+4]); d = b[p+4:p+4+l]; p += 4 + l
            if t == 0:
                sni = d[5:5+struct.unpack(">H", d[3:5])[0]].decode()
            elif t == 16:
                q = 2
                while q < len(d):
                    n = d[q]; alpn.append(d[q+1:q+1+n].decode()); q += 1 + n
    except Exception:
        pass
    return sni, alpn


# ---------- what the model says ----------
SHELL_RE = re.compile(r"bash|shell|exec|run_command|terminal|execute|run_terminal|command", re.I)


def find_shell_tool(tools):
    """(name, schema) of the program's shell tool, from any provider's tool list."""
    flat = []
    for t in tools or []:
        if "functionDeclarations" in t:
            flat += [(f["name"], f.get("parameters") or f.get("parametersJsonSchema") or {}) for f in t["functionDeclarations"]]
        elif "function" in t:
            flat.append((t["function"]["name"], t["function"].get("parameters") or {}))
        elif "name" in t:
            flat.append((t["name"], t.get("input_schema") or t.get("parameters") or {}))
    named = [f for f in flat if SHELL_RE.search(f[0])]
    pref = [f for f in named if re.search(r"bash|shell|exec_command|run_command|run_shell", f[0], re.I)]
    return (pref or named or [None])[0], [f[0] for f in flat]


def shell_input(schema, cmd):
    props = schema.get("properties", {}) if isinstance(schema, dict) else {}
    out = {}
    key = next((k for k in props if k.lower() in ("command", "cmd", "commandline", "command_line", "script")), None)
    key = key or next((k for k in props if "command" in k.lower()), None) or "command"
    out[key] = cmd
    for k in (schema.get("required") or []):
        if k in out:
            continue
        ty = (props.get(k) or {}).get("type")
        ty = ty[0] if isinstance(ty, list) else ty
        if ty == "boolean": out[k] = False
        elif ty in ("integer", "number"): out[k] = 30000 if "timeout" in k.lower() else 1
        elif ty == "array": out[k] = [cmd] if "cmd" in k.lower() or "command" in k.lower() else []
        elif ty == "object": out[k] = {}
        else: out[k] = WORK if "dir" in k.lower() or "cwd" in k.lower() or "path" in k.lower() else "run it"
    if key.lower() == "command" and (props.get(key) or {}).get("type") == "array":
        out[key] = ["bash", "-lc", cmd]
    return out


def decide(body_text, tools):
    """What the model does on this request: ('text', s) or ('call', name, input)."""
    tool, names = find_shell_tool(tools)
    if not tool:
        if "SEARCH/REPLACE" in body_text and not state.get("aider"):
            state["aider"] = True
            return ("text", AIDER_DENY if SCEN == "deny" else AIDER_OK), names
        return ("text", "ok"), names
    if "silo_call_1" in body_text or "functionResponse" in body_text or '"role": "tool"' in body_text or '"role":"tool"' in body_text:
        done = next((n for n in names if n in ("attempt_completion", "task_complete", "finish", "submit", "complete_task", "submit_and_exit")), None)
        if done and "silo_call_2" not in body_text:
            return ("call2", done, {"result": "done"}), names
        return ("text", "done"), names
    cmd = f"rm {WORK}/existing.txt && echo removed" if SCEN == "deny" else f"echo silo-ok > {WORK}/proof.txt && echo written"
    return ("call", tool[0], shell_input(tool[1], cmd)), names


AIDER_OK = "Creating the file.\n\nproof.txt\n```\n<<<<<<< SEARCH\n=======\nsilo-ok\n>>>>>>> REPLACE\n```\n\n```bash\necho silo-ok >> proof.txt\n```\n"
AIDER_DENY = "Removing the line.\n\nexisting.txt\n```\n<<<<<<< SEARCH\nold line\n=======\nnew line\n>>>>>>> REPLACE\n```\n\n```bash\nrm existing.txt\n```\n"


SILO_WORDS = "Silo: this computer's budget is reached. Raise it in the Silo app."


def probe(cmd):
    if not FIFO:
        return
    with open(FIFO, "w") as f:
        if cmd == "arm":
            f.write("reset\n" + "".join(f"allow {p}\n" for p in os.environ.get("ALLOW", "").split(":") if p) + "".join(f"{x}\n" for x in (open(os.environ["PROBE_EXTRA_FILE"]).read() if os.environ.get("PROBE_EXTRA_FILE") else "").split("|") if x) + "arm\n")
        else:
            f.write("disarm\n")
    time.sleep(0.05); log(f"probe {cmd}")


# ---------- one handler for every host ----------
class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def body(self):
        if self.headers.get("transfer-encoding", "").lower() == "chunked":
            raw = b""
            while True:
                n = int(self.rfile.readline().strip().split(b";")[0], 16)
                if n == 0:
                    self.rfile.readline(); break
                raw += self.rfile.read(n); self.rfile.readline()
        else:
            raw = self.rfile.read(int(self.headers.get("content-length", 0) or 0))
        enc = self.headers.get("content-encoding", "")
        try:
            if enc == "gzip": raw = gzip.decompress(raw)
            elif enc == "deflate": raw = zlib.decompress(raw)
            elif enc == "zstd":
                import zstandard; raw = zstandard.ZstdDecompressor().decompress(raw, max_output_size=64 << 20)
            elif enc == "br":
                raw = raw  # logged as is
        except Exception as e:
            log(f"decode {enc} failed: {e}")
        return raw

    def hdrs(self):
        out = {}
        for k, v in self.headers.items():
            kl = k.lower()
            if kl in ("authorization", "x-api-key", "x-goog-api-key", "api-key", "cookie", "x-amz-security-token", "proxy-authorization") or "token" in kl:
                parts = v.split(" ", 1)
                out[k] = (parts[0] + " " if len(parts) == 2 else "") + f"<{len(parts[-1])} chars, starts {parts[-1][:6]!r}>"
            else:
                out[k] = v[:300]
        return out

    def handle_one(self):
        host = self.headers.get("host", "")
        b = self.body() if self.command in ("POST", "PUT", "PATCH") else b""
        text = b.decode("utf-8", "replace")
        try: j = json.loads(text) if text.strip().startswith(("{", "[")) else None
        except Exception: j = None
        r = {"host": host, "sni": getattr(self.connection, "sni", None), "tls": getattr(self.server, "tls", False),
             "method": self.command, "path": self.path, "headers": self.hdrs(), "body_len": len(b),
             "content_encoding": self.headers.get("content-encoding"), "upgrade": self.headers.get("upgrade")}
        if j is not None and isinstance(j, dict):
            r["body_keys"] = list(j.keys())[:40]
            r["model"] = j.get("model")
            r["stream"] = j.get("stream")
        r["body_head"] = text[:600]
        p = self.path.split("?")[0]
        if self.headers.get("upgrade", "").lower() == "websocket":
            r["answer"] = "404 websocket refused"; rec(r); return self.send_json(404, {"error": "no websocket here"})
        tools = (j or {}).get("tools") if isinstance(j, dict) else None
        if isinstance(j, dict) and not tools and isinstance(j.get("request"), dict):
            tools = j["request"].get("tools")
        kind = None
        if p.endswith("/messages") and self.command == "POST" and not p.endswith("count_tokens"): kind = "anthropic"
        elif p.endswith("/chat/completions") and self.command == "POST": kind = "chat"
        elif p.endswith("/responses") and self.command == "POST": kind = "responses"
        elif ":streamGenerateContent" in p or ":generateContent" in p: kind = "gemini"
        elif p == "/api/chat" and self.command == "POST": kind = "ollama"
        r["kind"] = kind
        if kind:
            (what, names) = decide(text, tools)
            r["tools"] = names[:60]; r["decided"] = what[0] if what[0] == "text" else f"call {what[1]} {json.dumps(what[2])}"
            if what[0] == "call2":
                what = ("call", what[1], what[2], "silo_call_2")
            main = what[0] == "call" or what == ("text", "done") or what[1] in (AIDER_OK, AIDER_DENY)
            if main:
                state["main"] += 1
                if state["armed"]:
                    state["armed"] = False; probe("disarm")
            if main and SCEN.startswith("keep:") and not state["held"]:
                state["held"] = True; _, secs, mode = SCEN.split(":"); secs = float(secs)
                st0 = (j or {}).get("stream", False) if kind != "gemini" else ":streamGenerateContent" in p
                if kind == "ollama": st0 = (j or {}).get("stream", True)
                r["answer"] = f"kept {secs}s {mode} stream={st0}"; rec(dict(r))
                ctype = "text/event-stream" if st0 else ("application/x-ndjson" if kind == "ollama" else "application/json")
                self.send_response(200); self.send_header("content-type", ctype); self.send_header("transfer-encoding", "chunked"); self.end_headers(); self.wfile.flush()
                self._started = True; t = time.time(); gone = None
                try:
                    while time.time() - t < secs:
                        time.sleep(5)
                        if mode == "ping":
                            if st0 and kind == "anthropic": self.chunk(b"event: ping\ndata: {\"type\": \"ping\"}\n\n")
                            elif st0: self.chunk(b": silo is holding this reply\n\n")
                            else: self.chunk(b" ")
                        elif mode == "unk":
                            self.chunk(b"event: silo.hold\ndata: {\"type\": \"silo.hold\"}\n\n" if st0 else b" ")
                        elif mode == "comment":
                            self.chunk(b": silo is holding this reply\n\n" if st0 else b" ")
                        rr, _, _ = select.select([self.connection], [], [], 0)
                        if rr:
                            try: d = self.connection.recv(1)
                            except Exception: d = b""
                            if not d: gone = round(time.time() - t, 1); break
                except (BrokenPipeError, ConnectionResetError, ssl.SSLError, OSError):
                    gone = round(time.time() - t, 1)
                rec({"host": host, "path": self.path, "event": "keep-end", "client_gave_up_after": gone})
                if gone is not None:
                    return
                return getattr(self, "reply_" + kind)(what, (j or {}).get("model") or "fake-model", st0)
            if main and SCEN.startswith("hold:") and not state["held"]:
                state["held"] = True; secs = float(SCEN.split(":")[1]); r["answer"] = f"held {secs}s"; rec(dict(r))
                gone = self.hold(secs)
                rec({"host": host, "path": self.path, "event": "hold-end", "client_gave_up_after": gone})
                if gone is not None:
                    return
            if kind and SCEN.startswith("refuse:"):
                parts = SCEN.split(":"); st = int(parts[1]); extra = parts[2] if len(parts) > 2 else ""
                r["answer"] = f"refused {st} {extra}"; rec(r)
                if st == 200:
                    return getattr(self, "reply_" + kind)(("text", SILO_WORDS), (j or {}).get("model") or "fake-model", (j or {}).get("stream", False) if kind != "gemini" else ":streamGenerateContent" in p)
                hdr = {"x-should-retry": "false"} if extra == "noretry" else {"retry-after": "86400"} if extra == "ra" else {}
                return self.send_json(st, {"error": {"message": SILO_WORDS}} if extra == "generic" else self.error_body(kind, st), hdr)
            if main and SCEN.startswith("err:") and not state["erred"]:
                state["erred"] = True; st = int(SCEN.split(":")[1]); r["answer"] = f"{st}"; rec(r)
                return self.send_json(st, self.error_body(kind, st))
            if (what[0] == "call" and len(what) == 3 or what[1] == AIDER_DENY) and SCEN == "deny" and not state["armed"]:
                state["armed"] = True; probe("arm")
            r["answer"] = "200 " + kind; rec(r)
            stream = (j or {}).get("stream", False) if kind != "gemini" else ":streamGenerateContent" in p
            if kind == "ollama": stream = (j or {}).get("stream", True)
            return getattr(self, "reply_" + kind)(what, (j or {}).get("model") or "fake-model", stream)
        # side calls
        ans = self.side(p, host)
        r["answer"] = f"{ans[0]} side"; rec(r)
        return self.send_json(*ans)

    def error_body(self, kind, st):
        msg = SILO_WORDS
        if kind == "anthropic": return {"type": "error", "error": {"type": "rate_limit_error" if st == 429 else "permission_error", "message": msg}}
        if kind == "gemini": return {"error": {"code": st, "message": msg, "status": "PERMISSION_DENIED" if st == 403 else "RESOURCE_EXHAUSTED"}}
        return {"error": {"message": msg, "type": "silo_refused", "code": st}}

    def side(self, p, host):
        if p == "/api.json" and host in ("models.dev", "models.opencode.ai"):
            mm = lambda i: {"id": i, "name": i, "attachment": True, "reasoning": False, "temperature": True, "tool_call": True, "cost": {"input": 3, "output": 15}, "limit": {"context": 200000, "output": 32000}, "modalities": {"input": ["text"], "output": ["text"]}, "release_date": "2025-09-29", "last_updated": "2025-09-29", "open_weights": False}
            return 200, {"anthropic": {"id": "anthropic", "name": "Anthropic", "env": ["ANTHROPIC_API_KEY"], "npm": "@ai-sdk/anthropic", "doc": "x", "models": {"claude-sonnet-4-5": mm("claude-sonnet-4-5")}},
                         "openai": {"id": "openai", "name": "OpenAI", "env": ["OPENAI_API_KEY"], "npm": "@ai-sdk/openai", "doc": "x", "models": {"gpt-5": mm("gpt-5")}},
                         "google": {"id": "google", "name": "Google", "env": ["GEMINI_API_KEY", "GOOGLE_GENERATIVE_AI_API_KEY"], "npm": "@ai-sdk/google", "doc": "x", "models": {"gemini-2.5-pro": mm("gemini-2.5-pro")}}}
        if "githubcopilot.com" in host and p.endswith("/models"):
            m = lambda i, v, eps: {"id": i, "name": i, "vendor": v, "version": i, "object": "model", "preview": False, "model_picker_enabled": True, "is_chat_default": i.startswith("claude"),
                                   "capabilities": {"type": "chat", "family": i, "object": "model_capabilities", "tokenizer": "o200k_base", "limits": {"max_context_window_tokens": 200000, "max_output_tokens": 16000, "max_prompt_tokens": 128000}, "supports": {"tool_calls": True, "parallel_tool_calls": True, "streaming": True, "vision": False}},
                                   "policy": {"state": "enabled"}, "billing": {"is_premium": True, "multiplier": 1}, "supported_endpoints": eps}
            return 200, {"object": "list", "data": [m("claude-sonnet-4.5", "Anthropic", ["/chat/completions", "/v1/messages"]), m("gpt-5", "OpenAI", ["/chat/completions", "/responses"])]}
        if p.endswith("/models") or p == "/v1/models":
            return 200, {"object": "list", "data": [{"id": "fake-model", "object": "model", "created": 0, "owned_by": "x", "type": "model", "display_name": "Fake"}], "models": [{"name": "models/gemini-2.5-pro"}]}
        if p == "/api/tags":
            return 200, {"models": [{"name": "fake-model:latest", "model": "fake-model:latest", "size": 1, "digest": "0" * 64, "details": {"family": "x", "parameter_size": "1B", "quantization_level": "Q4"}}]}
        if p == "/api/show":
            return 200, {"modelfile": "", "parameters": "", "template": "", "details": {"family": "x"}, "model_info": {"general.architecture": "x", "x.context_length": 32768}, "capabilities": ["completion", "tools"]}
        if "githubcopilot.com" in host and p == "/meta":
            m = lambda i, v, eps: {"id": i, "name": i, "vendor": v, "version": i, "object": "model", "preview": False, "model_picker_enabled": True, "is_chat_default": i.startswith("claude"),
                                   "capabilities": {"type": "chat", "family": i, "object": "model_capabilities", "tokenizer": "o200k_base", "limits": {"max_context_window_tokens": 200000, "max_output_tokens": 16000, "max_prompt_tokens": 128000}, "supports": {"tool_calls": True, "parallel_tool_calls": True, "streaming": True, "vision": False}},
                                   "policy": {"state": "enabled"}, "billing": {"is_premium": True, "multiplier": 1}, "supported_endpoints": eps}
            return 200, {"models": [m("claude-sonnet-4.5", "Anthropic", ["/chat/completions", "/v1/messages"]), m("gpt-5", "OpenAI", ["/chat/completions", "/responses"])]}
        if p.startswith("/copilot_internal/team_customizations") or p.startswith("/copilot_internal/managed_settings"):
            return 200, {}
        if p == "/copilot_internal/user":
            return 200, {"login": "fake", "access_type_sku": "free_limited_copilot", "copilot_plan": "individual", "chat_enabled": True, "is_mcp_enabled": True,
                         "endpoints": {"api": "https://api.individual.githubcopilot.com", "proxy": "https://proxy.individual.githubcopilot.com", "telemetry": "https://telemetry.individual.githubcopilot.com", "origin-tracker": "https://origin-tracker.individual.githubcopilot.com"}}
        if p == "/copilot_internal/v2/token":
            return 200, {"token": "tid=fake;exp=9999999999;sku=free;proxy-ep=proxy.individual.githubcopilot.com", "expires_at": int(time.time()) + 1800, "refresh_in": 1500, "endpoints": {"api": "https://api.individual.githubcopilot.com"}}
        if p == "/auth/exchange_user_api_key":
            import base64
            b64 = lambda o: base64.urlsafe_b64encode(json.dumps(o).encode()).rstrip(b"=").decode()
            jwt = b64({"alg": "HS256", "typ": "JWT"}) + "." + b64({"sub": "auth0|user_fake", "exp": int(time.time()) + 86400, "iat": int(time.time()), "iss": "https://authentication.cursor.sh", "scope": "openid profile email offline_access", "aud": "https://cursor.com"}) + ".c2ln"
            return 200, {"accessToken": jwt, "refreshToken": jwt, "authId": "auth0|user_fake"}
        if p == "/api/version":
            return 200, {"version": "0.40.18"}
        if p.endswith(":countTokens"):
            return 200, {"totalTokens": 10}
        if p.endswith("/count_tokens"):
            return 200, {"input_tokens": 10}
        return 404, {"error": "fake hub: not known"}

    def hold(self, secs):
        """Hold the request with no answer. Return seconds until the client closed, or None."""
        s = self.connection; t = time.time()
        while time.time() - t < secs:
            rr, _, _ = select.select([s], [], [], 0.5)
            if rr:
                try:
                    d = s.recv(1, socket.MSG_PEEK) if not isinstance(s, ssl.SSLSocket) else s.recv(1)
                except Exception:
                    d = b""
                if not d:
                    g = round(time.time() - t, 1); log(f"client gave up after {g}s"); return g
        log(f"hold of {secs}s ran out with the client still waiting"); return None

    def send_json(self, st, obj, extra=None):
        b = json.dumps(obj).encode()
        if getattr(self, "_started", False):
            self.chunk(b); self.end(); return
        self.send_response(st); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(b)))
        for k, v in (extra or {}).items(): self.send_header(k, v)
        self.end_headers(); self.wfile.write(b)

    def sse_start(self, ctype="text/event-stream"):
        if getattr(self, "_started", False):
            return
        self.send_response(200); self.send_header("content-type", ctype); self.send_header("transfer-encoding", "chunked"); self.send_header("cache-control", "no-cache"); self.end_headers()

    def chunk(self, b):
        self.wfile.write(b"%x\r\n%s\r\n" % (len(b), b)); self.wfile.flush()

    def end(self):
        self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()

    # --- Anthropic Messages ---
    def reply_anthropic(self, what, model, stream):
        mid = "msg_" + uuid.uuid4().hex[:20]
        usage = {"input_tokens": 50, "output_tokens": 20, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
        blocks = [{"type": "text", "text": what[1]}] if what[0] == "text" else [{"type": "tool_use", "id": (what[3] if len(what) > 3 else "silo_call_1"), "name": what[1], "input": what[2]}]
        stop = "end_turn" if what[0] == "text" else "tool_use"
        if not stream:
            return self.send_json(200, {"id": mid, "type": "message", "role": "assistant", "model": model, "content": blocks, "stop_reason": stop, "stop_sequence": None, "usage": usage})
        self.sse_start()
        ev = lambda n, d: self.chunk(f"event: {n}\ndata: {json.dumps(d)}\n\n".encode())
        ev("message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "stop_sequence": None, "usage": dict(usage, output_tokens=1)}})
        b = blocks[0]
        if b["type"] == "text":
            ev("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
            ev("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": b["text"]}})
        else:
            ev("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}})
            ev("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": json.dumps(b["input"])}})
        ev("content_block_stop", {"type": "content_block_stop", "index": 0})
        ev("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": 20}})
        ev("message_stop", {"type": "message_stop"}); self.end()

    # --- OpenAI Chat Completions ---
    def reply_chat(self, what, model, stream):
        cid = "chatcmpl-" + uuid.uuid4().hex[:20]
        usage = {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}
        if what[0] == "text":
            msg = {"role": "assistant", "content": what[1]}; fin = "stop"
        else:
            msg = {"role": "assistant", "content": None, "tool_calls": [{"id": (what[3] if len(what) > 3 else "silo_call_1"), "type": "function", "function": {"name": what[1], "arguments": json.dumps(what[2])}}]}; fin = "tool_calls"
        if not stream:
            return self.send_json(200, {"id": cid, "object": "chat.completion", "created": int(time.time()), "model": model, "choices": [{"index": 0, "message": msg, "finish_reason": fin}], "usage": usage})
        self.sse_start()
        d = lambda delta, fr=None, u=None: self.chunk(("data: " + json.dumps({"id": cid, "object": "chat.completion.chunk", "created": int(time.time()), "model": model, "choices": [{"index": 0, "delta": delta, "finish_reason": fr}] if u is None else [], **({"usage": u} if u else {})}) + "\n\n").encode())
        d({"role": "assistant", "content": ""})
        if what[0] == "text":
            d({"content": what[1]})
        else:
            tc = msg["tool_calls"][0]
            d({"tool_calls": [{"index": 0, "id": tc["id"], "type": "function", "function": {"name": tc["function"]["name"], "arguments": ""}}]})
            d({"tool_calls": [{"index": 0, "function": {"arguments": tc["function"]["arguments"]}}]})
        d({}, fin); d({}, None, usage)
        self.chunk(b"data: [DONE]\n\n"); self.end()

    # --- OpenAI Responses ---
    def reply_responses(self, what, model, stream):
        rid = "resp_" + uuid.uuid4().hex[:20]
        if what[0] == "text":
            item = {"type": "message", "id": "msg_" + uuid.uuid4().hex[:12], "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": what[1], "annotations": []}]}
        else:
            item = {"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:12], "status": "completed", "call_id": (what[3] if len(what) > 3 else "silo_call_1"), "name": what[1], "arguments": json.dumps(what[2])}
        resp = {"id": rid, "object": "response", "created_at": int(time.time()), "status": "completed", "model": model, "output": [item], "usage": {"input_tokens": 50, "output_tokens": 20, "total_tokens": 70, "input_tokens_details": {"cached_tokens": 0}, "output_tokens_details": {"reasoning_tokens": 0}}}
        if not stream:
            return self.send_json(200, resp)
        self.sse_start(); n = [0]
        def ev(t, d):
            n[0] += 1; self.chunk(f"event: {t}\ndata: {json.dumps(dict(type=t, sequence_number=n[0], **d))}\n\n".encode())
        ev("response.created", {"response": dict(resp, status="in_progress", output=[])})
        ev("response.output_item.added", {"output_index": 0, "item": dict(item, status="in_progress")})
        ev("response.output_item.done", {"output_index": 0, "item": item})
        ev("response.completed", {"response": resp}); self.end()

    # --- Gemini ---
    def reply_gemini(self, what, model, stream):
        part = {"text": what[1]} if what[0] == "text" else {"functionCall": {"name": what[1], "args": what[2]}}
        obj = {"candidates": [{"content": {"role": "model", "parts": [part]}, "finishReason": "STOP", "index": 0}], "usageMetadata": {"promptTokenCount": 50, "candidatesTokenCount": 20, "totalTokenCount": 70}, "modelVersion": "gemini-2.5-pro", "responseId": uuid.uuid4().hex[:16]}
        if not stream:
            return self.send_json(200, obj)
        self.sse_start(); self.chunk(f"data: {json.dumps(obj)}\r\n\r\n".encode()); self.end()

    # --- Ollama ---
    def reply_ollama(self, what, model, stream):
        msg = {"role": "assistant", "content": what[1]} if what[0] == "text" else {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": what[1], "arguments": what[2]}}]}
        fin = {"model": model, "created_at": "2026-11-24T00:00:00Z", "message": msg, "done": True, "done_reason": "stop", "prompt_eval_count": 50, "eval_count": 20}
        if not stream:
            return self.send_json(200, fin)
        self.sse_start("application/x-ndjson"); self.chunk((json.dumps(fin) + "\n").encode()); self.end()

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = do_OPTIONS = handle_one


class TLSServer(ThreadingHTTPServer):
    tls = True
    daemon_threads = True

    def get_request(self):
        sock, addr = self.socket.accept()
        sock.settimeout(10)
        try:
            head = sock.recv(16384, socket.MSG_PEEK)
        except Exception:
            head = b""
        sni, alpn = parse_client_hello(head)
        if sni is None and alpn is None:
            sock.settimeout(None); return sock, addr
        c = ctx_for(sni)
        try:
            s = c.wrap_socket(sock, server_side=True)
        except Exception as e:
            rec({"event": "tls-failed", "sni": sni, "alpn_offered": alpn, "error": str(e)[:200]}); log(f"TLS failed for {sni} alpn={alpn}: {e}")
            raise OSError("tls")
        s.settimeout(None)
        rec({"event": "tls", "sni": sni, "alpn_offered": alpn, "alpn": s.selected_alpn_protocol(), "version": s.version()})
        s.sni = sni
        return s, addr

    def handle_error(self, request, client_address):
        log(f"handler error: {sys.exc_info()[1]!r}")


class PlainServer(ThreadingHTTPServer):
    daemon_threads = True


def dns():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind(("127.0.0.1", 53))
    while True:
        q, a = s.recvfrom(2048)
        try:
            qid = q[:2]; p = 12; labels = []
            while q[p]:
                labels.append(q[p+1:p+1+q[p]].decode()); p += 1 + q[p]
            name = ".".join(labels); qtype = struct.unpack(">H", q[p+1:p+3])[0]; qend = p + 5
            rec({"event": "dns", "name": name, "type": qtype})
            hdr = qid + b"\x81\x80" + q[4:6] + (b"\x00\x01" if qtype == 1 else b"\x00\x00") + b"\x00\x00\x00\x00"
            ans = (b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04" + socket.inet_aton("127.0.0.1")) if qtype == 1 else b""
            s.sendto(hdr + q[12:qend] + ans, a)
        except Exception as e:
            log(f"dns error {e}")


if __name__ == "__main__":
    threading.Thread(target=dns, daemon=True).start()
    for port in (80, 11434, 1234):
        srv = PlainServer(("127.0.0.1", port), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
    ts = TLSServer(("127.0.0.1", 443), H)
    log(f"hub up, scenario {SCEN}")
    ts.serve_forever()
