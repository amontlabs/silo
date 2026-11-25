"""Fake providers for the login tests: Anthropic's sign-in and Messages API, OpenAI's sign-in and
ChatGPT's Codex backend, on one TLS port, chosen by Host.

They behave as a strict real provider would on the points the tests need:
- an access token works only while it is not expired, and only the exact token issued;
- a refresh token can be used once: using it returns a new pair and retires it (rotation);
- a retired refresh token used again, more than 10 s after it was retired, revokes the whole family, every token of that login
  (reuse detection, OAuth 2.0 Security BCP section 4.14.2);
- every request is written to requests.jsonl, with its credential, so the tests can show what
  reached the provider.

Env: PORT, BIND, TLS_CERT, TLS_KEY, OUT, ACCESS_TTL (seconds, default 3600).
`touch OUT/expire` expires every access token issued so far, as time passing would."""
import base64, http.server, json, os, secrets, ssl, threading, time, uuid

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
TTL = int(os.environ.get("ACCESS_TTL", "3600"))
REQF = open(f"{OUT}/requests.jsonl", "a"); lock = threading.Lock()
ALPH = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-"

def rnd(n): return "".join(secrets.choice(ALPH) for _ in range(n))
def b64u(b): return base64.urlsafe_b64encode(b).rstrip(b"=").decode()

# family id -> {"access": {token: exp}, "refresh": current, "retired": set, "revoked": bool, "who": str}
FAM = {}; ACCESS = {}; REFRESH = {}; IDT = {}; ISSUED = {}

def new_family(who):
    f = str(uuid.uuid4()); FAM[f] = {"access": {}, "refresh": None, "retired": set(), "revoked": False, "who": who}; return f

def jwt(claims):
    h = b64u(json.dumps({"alg": "RS256", "typ": "JWT", "kid": "fake"}).encode())
    p = b64u(json.dumps(claims).encode())
    return f"{h}.{p}.{rnd(342)}"

ACCT = "acc-" + str(uuid.uuid4())

def issue(fam, who):
    now = int(time.time()); exp = now + TTL
    if who == "anthropic":
        a = "sk-ant-oat01-" + rnd(95); r = "sk-ant-ort01-" + rnd(95)
        out = {"token_type": "Bearer", "access_token": a, "refresh_token": r, "expires_in": TTL,
               "scope": "user:inference user:profile user:sessions:claude_code",
               "account": {"uuid": str(uuid.uuid4()), "email_address": "user@example.com"},
               "organization": {"uuid": str(uuid.uuid4()), "name": "Example"}}
    else:
        auth = {"chatgpt_account_id": ACCT, "chatgpt_plan_type": "pro", "chatgpt_user_id": "user-x", "user_id": "user-x",
                "chatgpt_account_user_id": "user-x__" + ACCT}
        prof = {"https://api.openai.com/profile": {"email": "user@example.com", "email_verified": True}}
        a = jwt({"iss": "https://auth.openai.com", "aud": ["https://api.openai.com/v1"], "exp": exp, "iat": now,
                 "sub": "auth0|user-x", **prof, "https://api.openai.com/auth": auth, "scp": ["openid", "profile", "email", "offline_access"]})
        idt = jwt({"iss": "https://auth.openai.com", "aud": "app_EMoamEEZ73f0CkXaXp7hrann", "exp": exp, "iat": now,
                   "sub": "auth0|user-x", "email": "user@example.com", "email_verified": True, "https://api.openai.com/auth": auth})
        r = "rt_" + rnd(40) + "." + rnd(43)
        out = {"id_token": idt, "access_token": a, "refresh_token": r, "expires_in": TTL, "token_type": "Bearer",
               "scope": "openid profile email offline_access"}
    F = FAM[fam]; F["access"][a] = exp; ACCESS[a] = fam; ISSUED[a] = time.time()
    with open(f"{OUT}/issued.txt", "a") as f:     # fake values: the test searches the computers for them
        f.write("\n".join(v for k, v in out.items() if k in ("access_token", "refresh_token", "id_token")) + "\n")
    if out.get("id_token"): IDT[out["id_token"]] = fam
    if F["refresh"]: F["retired"].add(F["refresh"])
    F["refresh"] = r; REFRESH[r] = fam
    return out

def access_ok(tok):
    fam = ACCESS.get(tok)
    if not fam: return "unknown token"
    F = FAM[fam]
    if F["revoked"]: return "token revoked"
    if F["access"][tok] < time.time(): return "token expired"
    # `touch OUT/expire` ends every access token issued before it, as time passing would.
    if os.path.exists(f"{OUT}/expire") and ISSUED[tok] < os.path.getmtime(f"{OUT}/expire"): return "token expired"
    return None

def refresh(tok, who):
    fam = REFRESH.get(tok)
    if not fam: return None, "invalid_grant: unknown refresh token"
    F = FAM[fam]
    if F["revoked"]: return None, "invalid_grant: refresh token revoked"
    if tok in F["retired"]:
        # A grace of 10 s: a program that refreshes from several tasks at once gets the same new
        # pair (Codex 0.172.0 sends three refreshes within 0.3 s at start). Later reuse is theft.
        if time.time() - F["rotated_at"] < 10 and F.get("last_out"):
            return F["last_out"], None
        F["revoked"] = True
        return None, "invalid_grant: refresh token reused, every token of this login is revoked"
    out = issue(fam, who); F["rotated_at"] = time.time(); F["last_out"] = out
    return out, None

def sse(ev, d): return f"event: {ev}\ndata: {json.dumps(d)}\n\n".encode()

class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def rec(self, body, verdict):
        auth = self.headers.get("authorization") or self.headers.get("x-api-key") or ""
        with lock:
            REQF.write(json.dumps({"t": round(time.time(), 3), "host": self.headers.get("host"), "method": self.command,
                                   "path": self.path, "auth": auth, "verdict": verdict, "body": body[:2000]}) + "\n"); REQF.flush()

    def send(self, code, obj, ctype="application/json"):
        b = obj if isinstance(obj, bytes) else json.dumps(obj).encode()
        self.send_response(code); self.send_header("content-type", ctype); self.send_header("content-length", str(len(b)))
        self.end_headers(); self.wfile.write(b)

    def body(self):
        n = int(self.headers.get("content-length") or 0); return self.rfile.read(n).decode("utf-8", "replace")

    def bearer(self):
        a = self.headers.get("authorization") or ""
        return a[7:] if a.lower().startswith("bearer ") else None

    def do_GET(self): self.route("")
    def do_POST(self): self.route(self.body())

    def route(self, body):
        host = (self.headers.get("host") or "").split(":")[0]; p = self.path.split("?")[0]
        if p.endswith("/oauth/token"):
            return self.token(host, body)
        tok = self.bearer()
        if tok is None:
            self.rec(body, "no bearer"); return self.send(401, {"type": "error", "error": {"type": "authentication_error", "message": "missing token"}})
        bad = access_ok(tok)
        if bad:
            self.rec(body, bad); return self.send(401, {"type": "error", "error": {"type": "authentication_error", "message": f"OAuth {bad}"}})
        self.rec(body, "ok")
        if host == "api.anthropic.com" and p == "/v1/messages": return self.messages(body)
        if host == "chatgpt.com" and p.endswith("/responses"): return self.codex_responses(body)
        if p.endswith("/models"): return self.send(200, {"models": []})
        if p.endswith("/accounts/check"):
            a = {"id": ACCT, "account_id": ACCT, "name": None, "structure": "personal", "plan_type": "pro", "is_deactivated": False,
                 "workspace_backend_origin": "https://chatgpt.com", "account_routing_override": None}
            return self.send(200, {"accounts": {ACCT: dict(a, account=a)}, "account_ordering": [ACCT]})
        if "profile" in p:
            return self.send(200, {"account": {"uuid": str(uuid.uuid4()), "email": "user@example.com", "email_address": "user@example.com", "display_name": "User"},
                                   "organization": {"uuid": str(uuid.uuid4()), "organization_type": "claude_max", "rate_limit_tier": "default_claude_max_20x", "has_extra_usage_enabled": False}})
        return self.send(200, {})

    def token(self, host, body):
        who = "anthropic" if host in ("platform.claude.com", "console.anthropic.com") else "openai"
        try: j = json.loads(body)
        except Exception:
            from urllib.parse import parse_qs; j = {k: v[0] for k, v in parse_qs(body).items()}
        g = j.get("grant_type")
        if g == "authorization_code":
            if not j.get("code", "").startswith("code-"):
                self.rec(body, "bad code"); return self.send(400, {"error": "invalid_grant"})
            out = issue(new_family(who), who); self.rec(body, "signed in"); return self.send(200, out)
        if g == "refresh_token":
            out, err = refresh(j.get("refresh_token", ""), who)
            self.rec(body, err or "refreshed")
            if err: return self.send(400, {"error": "invalid_grant", "error_description": err})
            return self.send(200, out)
        if g == "urn:ietf:params:oauth:grant-type:token-exchange":
            # Codex trades its ID token for an API key: a new, long-lived credential minted from the login.
            fam = IDT.get(j.get("subject_token", ""))
            if not fam or FAM[fam]["revoked"]:
                self.rec(body, "exchange refused"); return self.send(400, {"error": "invalid_grant"})
            key = "sk-proj-" + rnd(156); FAM[fam]["access"][key] = time.time() + 365 * 86400; ACCESS[key] = fam
            self.rec(body, "API key minted"); return self.send(200, {"access_token": key, "token_type": "Bearer"})
        self.rec(body, "bad grant"); return self.send(400, {"error": "unsupported_grant_type"})

    def messages(self, body):
        j = json.loads(body or "{}")
        tools = [t.get("name") for t in j.get("tools", [])]
        msgs = j.get("messages", [])
        # The main loop: ask for one Bash call, then answer. Side requests (titles, etc.) get text.
        last = msgs[-1]["content"] if msgs else ""
        has_result = isinstance(last, list) and any(isinstance(c, dict) and c.get("type") == "tool_result" for c in last)
        blocks = []
        if "Bash" in tools and not has_result:
            blocks = [{"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": "Bash", "input": {"command": "echo model-ok", "description": "check"}}]
        else:
            blocks = [{"type": "text", "text": "Done."}]
        mid = "msg_" + uuid.uuid4().hex[:24]; model = j.get("model", "claude-x")
        out = [sse("message_start", {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "stop_sequence": None, "usage": {"input_tokens": 50, "output_tokens": 1}}})]
        for i, b in enumerate(blocks):
            if b["type"] == "text":
                out += [sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}),
                        sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": b["text"]}})]
            else:
                out += [sse("content_block_start", {"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}),
                        sse("content_block_delta", {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": json.dumps(b["input"])}})]
            out.append(sse("content_block_stop", {"type": "content_block_stop", "index": i}))
        stop = "tool_use" if blocks[0]["type"] == "tool_use" else "end_turn"
        out += [sse("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": 20}}),
                sse("message_stop", {"type": "message_stop"})]
        self.send(200, b"".join(out), "text/event-stream")

    def codex_responses(self, body):
        j = json.loads(body or "{}")
        inp = j.get("input", [])
        done = any(isinstance(i, dict) and i.get("type") in ("function_call_output", "custom_tool_call_output") for i in inp)
        tools = [t.get("name") or t.get("type") for t in j.get("tools", [])]
        if not done and "exec_command" in tools:
            items = [{"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20],
                      "name": "exec_command", "arguments": json.dumps({"cmd": "echo model-ok"}), "status": "completed"}]
        elif not done and "shell" in tools:
            items = [{"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:20], "call_id": "call_" + uuid.uuid4().hex[:20],
                      "name": "shell", "arguments": json.dumps({"command": ["bash", "-lc", "echo model-ok"]}), "status": "completed"}]
        else:
            items = [{"type": "message", "id": "msg_" + uuid.uuid4().hex[:20], "role": "assistant", "status": "completed",
                      "content": [{"type": "output_text", "text": "Done.", "annotations": []}]}]
        rid = "resp_" + uuid.uuid4().hex[:20]
        evs = [sse("response.created", {"type": "response.created", "response": {"id": rid, "object": "response", "status": "in_progress", "output": []}})]
        for k, it in enumerate(items):
            evs.append(sse("response.output_item.added", {"type": "response.output_item.added", "output_index": k, "item": it}))
            evs.append(sse("response.output_item.done", {"type": "response.output_item.done", "output_index": k, "item": it}))
        evs.append(sse("response.completed", {"type": "response.completed", "response": {"id": rid, "object": "response", "status": "completed", "output": items,
               "usage": {"input_tokens": 50, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 20, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 70}}}))
        self.send(200, b"".join(evs), "text/event-stream")

class S(http.server.ThreadingHTTPServer): daemon_threads = True

if __name__ == "__main__":
    srv = S((os.environ.get("BIND", "127.0.0.1"), int(os.environ.get("PORT", "8443"))), H)
    cx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); cx.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"])
    srv.socket = cx.wrap_socket(srv.socket, server_side=True)
    # A sign-in for each provider, made at start, as if the user had signed in on another machine.
    # The tests use them to stand for "a login inside the computer" (no vault) and to seed the vault.
    seeds = {w: issue(new_family(w), w) for w in ("anthropic", "openai")}
    json.dump(seeds, open(f"{OUT}/seeds.json", "w"))
    srv.serve_forever()
