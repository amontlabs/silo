"""The edge cases of a budget, one per run: routes by which a program can spend without the
boundary counting it, or count it wrong. Each case makes the calls that trigger it, through the
boundary, and prints one JSON line per step. run/edges.sh runs a case under a config and checks
what the boundary recorded against what the stand-in billed.

usage: edges.py <case>     (the case also comes from $CASE)

A case can play two computers: it connects to the boundary at a computer's address itself, with
the provider's name as SNI, instead of going through /etc/hosts.
"""
import http.client, json, os, socket, ssl, sys, time, uuid

CTX = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE"))
OPENAI = "sk-proj-script-fake"; ANTH = "sk-ant-api03-script-fake"; GOOG = "AIzaScriptFake000"
MAIN, FORK = "127.0.0.7", "127.0.0.8"


class Conn(http.client.HTTPSConnection):
    """HTTPS to a computer's address at the boundary, with the provider's name for TLS."""
    def __init__(self, host, addr):
        super().__init__(host, 443, context=CTX, timeout=300)
        self.addr = addr

    def connect(self):
        s = socket.create_connection((self.addr, 443), self.timeout)
        self.sock = CTX.wrap_socket(s, server_hostname=self.host)


def call(host, method, path, body=None, headers=None, addr=MAIN, raw=None, ctype="application/json"):
    c = Conn(host, addr)
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    h = dict(headers or {})
    if data is not None: h["content-type"] = ctype
    c.request(method, path, data, h)
    r = c.getresponse(); got = r.read(); c.close()
    return r.status, got


def jbody(b):
    try: return json.loads(b)
    except Exception: return None


def say(**k):
    print(json.dumps(k), flush=True)


def silo_said(b):
    s = b.decode(errors="replace")
    i = s.find("Silo:")
    return s[i:s.find('"', i)] if i >= 0 else None


H_OA = {"authorization": "Bearer " + OPENAI}
H_AN = {"x-api-key": ANTH, "anthropic-version": "2023-06-01"}
H_GO = {"x-goog-api-key": GOOG}


def text(n_bytes, seed="lorem ipsum dolor sit amet "):
    return (seed * (n_bytes // len(seed) + 1))[:n_bytes]


# ---- Responses: state kept at the provider ----

def prev_id():
    """A chained conversation sends only its new input; the provider bills the whole chain."""
    s, b = call("api.openai.com", "POST", "/v1/responses", {"model": "gpt-6.2-sol", "input": [{"type": "message", "role": "user", "content": text(300000) + " THINK=500"}]}, H_OA)
    rid = (jbody(b) or {}).get("id"); say(step="first, whole input", status=s)
    for k in range(3):
        s, b = call("api.openai.com", "POST", "/v1/responses", {"model": "gpt-6.2-sol", "previous_response_id": rid, "input": [{"type": "message", "role": "user", "content": f"go on {k} THINK=500"}]}, H_OA)
        rid = (jbody(b) or {}).get("id", rid); say(step=f"chained {k}", status=s, said=silo_said(b))
    conv = "conv_" + uuid.uuid4().hex[:12]
    for k in range(3):
        inp = text(200000, "conversation item ") if k == 0 else f"next {k}"
        s, b = call("api.openai.com", "POST", "/v1/responses", {"model": "gpt-6.2-sol", "conversation": conv, "input": [{"type": "message", "role": "user", "content": inp + " THINK=500"}]}, H_OA)
        say(step=f"conversation {k}", status=s, said=silo_said(b))


def background():
    """Background responses: the POST answers at once, the model runs on, and the result (with its
    usage) is fetched later by id. One is cancelled while it runs."""
    ids = []
    for k in range(6):
        s, b = call("api.openai.com", "POST", "/v1/responses", {"model": "gpt-6.2-sol", "background": True, "max_output_tokens": 30000, "input": [{"type": "message", "role": "user", "content": text(8000) + f" job {k} THINK=20000"}]}, H_OA)
        o = jbody(b) or {}
        say(step=f"start {k}", status=s, state=o.get("status"), said=silo_said(b))
        if s == 200: ids.append(o["id"])
    time.sleep(0.5)
    if ids:
        s, b = call("api.openai.com", "POST", f"/v1/responses/{ids[-1]}/cancel", {}, H_OA)
        say(step="cancel last", status=s, state=(jbody(b) or {}).get("status"))
    left = set(ids[:-1]); t0 = time.time()
    while left and time.time() - t0 < 120:
        for rid in list(left):
            s, b = call("api.openai.com", "GET", f"/v1/responses/{rid}", None, H_OA)
            st = (jbody(b) or {}).get("status")
            if st not in ("queued", "in_progress"): left.discard(rid); say(step="fetched", id=rid[-6:], state=st)
        time.sleep(0.5)
    # After the budget's held calls settle, one more fits or not.
    s, b = call("api.openai.com", "POST", "/v1/responses", {"model": "gpt-6.2-sol", "background": True, "max_output_tokens": 30000, "input": [{"type": "message", "role": "user", "content": "one more THINK=2000"}]}, H_OA)
    say(step="one more after settling", status=s, said=silo_said(b))
    if s == 200:
        rid = jbody(b)["id"]
        while True:
            s, b = call("api.openai.com", "GET", f"/v1/responses/{rid}", None, H_OA)
            if (jbody(b) or {}).get("status") not in ("queued", "in_progress"): break
            time.sleep(0.3)


# ---- tools the provider runs and bills per call ----

def websearch():
    for k in range(3):
        s, b = call("api.openai.com", "POST", "/v1/responses", {"model": "gpt-6.2-sol", "stream": k == 1, "tools": [{"type": "web_search"}], "input": [{"type": "message", "role": "user", "content": f"look it up {k} THINK=300"}]}, H_OA)
        say(step=f"responses web search {k}", status=s)
    for k in range(2):
        s, b = call("generativelanguage.googleapis.com", "POST", "/v1beta/models/gemini-3.1-pro-preview:generateContent", {"contents": [{"role": "user", "parts": [{"text": f"search {k} THINK=300"}]}], "tools": [{"googleSearch": {}}]}, H_GO)
        say(step=f"gemini grounding {k}", status=s)


# ---- Chat Completions: usage only on request, and n choices ----

def chat():
    hist = [{"role": "user", "content": text(6000) + " THINK=3000"}]
    for opts, label in [({}, "stream, no usage"), ({"stream_options": {"include_usage": True}}, "stream, include_usage"), ({"n": 4, "stream_options": {"include_usage": True}}, "stream, n=4, include_usage"), ({"n": 4}, "stream, n=4, no usage")]:
        s, b = call("api.openai.com", "POST", "/v1/chat/completions", dict({"model": "gpt-6.2-sol", "stream": True, "messages": hist}, **opts), H_OA)
        say(step=label, status=s, said=silo_said(b))
    s, b = call("api.openai.com", "POST", "/v1/chat/completions", {"model": "gpt-6.2-sol", "n": 3, "messages": hist}, H_OA)
    say(step="not streamed, n=3", status=s, said=silo_said(b))


# ---- prompt caching ----

def cache_expiry():
    """A conversation that writes its cache, reads it, then pauses past the cache's lifetime and
    pays to write it again. The stand-in's lifetimes are scaled (cache_ttl_scale)."""
    big = {"role": "user", "content": [{"type": "text", "text": text(240000), "cache_control": {"type": "ephemeral"}}]}
    msgs = [big]
    for k, pause in enumerate([0, 0, 0, 4, 0, 4]):
        time.sleep(pause)
        msgs = msgs + [{"role": "assistant", "content": "ok"}, {"role": "user", "content": f"turn {k} THINK=200"}] if k else msgs + [{"role": "assistant", "content": "ok"}, {"role": "user", "content": "turn 0 THINK=200"}]
        s, b = call("api.anthropic.com", "POST", "/v1/messages", {"model": "claude-opus-5-5", "max_tokens": 4000, "messages": msgs}, H_AN)
        say(step=f"turn {k} after {pause}s", status=s, said=silo_said(b))


def gcache():
    """Gemini's explicit cache, billed as storage for as long as it lives."""
    contents = [{"role": "user", "parts": [{"text": text(180000)}]}]
    s, b = call("generativelanguage.googleapis.com", "POST", "/v1beta/cachedContents", {"model": "models/gemini-3.1-pro-preview", "contents": contents, "ttl": "86400s"}, H_GO)
    say(step="cache for a day", status=s, said=silo_said(b))
    s, b = call("generativelanguage.googleapis.com", "POST", "/v1beta/cachedContents", {"model": "models/gemini-3.1-pro-preview", "contents": contents, "ttl": "1800s"}, H_GO)
    name = (jbody(b) or {}).get("name"); say(step="cache for 30 minutes", status=s, said=silo_said(b))
    if name:
        s, b = call("generativelanguage.googleapis.com", "POST", "/v1beta/models/gemini-3.1-pro-preview:generateContent", {"cachedContent": name, "contents": [{"role": "user", "parts": [{"text": "use it THINK=300"}]}]}, H_GO)
        say(step="a call that reads it", status=s)
        s, b = call("generativelanguage.googleapis.com", "PATCH", "/v1beta/" + name, {"ttl": "3600s"}, H_GO)
        say(step="extend by an hour", status=s, said=silo_said(b))
        time.sleep(1.0)
        s, b = call("generativelanguage.googleapis.com", "DELETE", "/v1beta/" + name, None, H_GO)
        say(step="delete it", status=s)
    s, b = call("generativelanguage.googleapis.com", "POST", "/v1beta/cachedContents", {"model": "models/gemini-3.1-pro-preview", "contents": contents[:1], "ttl": "600s"}, H_GO)
    say(step="left to expire", status=s, said=silo_said(b))
    time.sleep(1.5)


# ---- batches ----

def openai_batch(lines, max_tokens, read=True):
    jl = "\n".join(json.dumps({"custom_id": f"r{i}", "method": "POST", "url": "/v1/chat/completions", "body": {"model": "gpt-6.2-sol", "max_completion_tokens": max_tokens, "messages": [{"role": "user", "content": text(6000) + f" item {i} THINK=1500"}]}}) for i in range(lines)) + "\n"
    bnd = "----silo" + uuid.uuid4().hex
    raw = (f"--{bnd}\r\ncontent-disposition: form-data; name=\"purpose\"\r\n\r\nbatch\r\n--{bnd}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"b.jsonl\"\r\ncontent-type: application/jsonl\r\n\r\n").encode() + jl.encode() + f"\r\n--{bnd}--\r\n".encode()
    s, b = call("api.openai.com", "POST", "/v1/files", None, H_OA, raw=raw, ctype="multipart/form-data; boundary=" + bnd)
    fid = (jbody(b) or {}).get("id"); say(step=f"upload {lines} lines", status=s)
    s, b = call("api.openai.com", "POST", "/v1/batches", {"input_file_id": fid, "endpoint": "/v1/chat/completions", "completion_window": "24h"}, H_OA)
    say(step=f"openai batch of {lines}, max {max_tokens}", status=s, said=silo_said(b))
    if s != 200 or not read: return
    bid = jbody(b)["id"]
    while True:
        s, b = call("api.openai.com", "GET", f"/v1/batches/{bid}", None, H_OA)
        o = jbody(b)
        if o["status"] in ("completed", "failed", "expired", "cancelled"): break
        time.sleep(0.5)
    s, b = call("api.openai.com", "GET", f"/v1/files/{o['output_file_id']}/content", None, H_OA)
    say(step="results read", status=s, lines=b.count(b"\n"))


def anthropic_batch(n, max_tokens, read=True):
    reqs = [{"custom_id": f"r{i}", "params": {"model": "claude-opus-5-5", "max_tokens": max_tokens, "messages": [{"role": "user", "content": text(6000) + f" item {i} THINK=1500"}]}} for i in range(n)]
    s, b = call("api.anthropic.com", "POST", "/v1/messages/batches", {"requests": reqs}, H_AN)
    say(step=f"anthropic batch of {n}, max {max_tokens}", status=s, said=silo_said(b))
    if s != 200 or not read: return
    bid = jbody(b)["id"]
    while True:
        s, b = call("api.anthropic.com", "GET", f"/v1/messages/batches/{bid}", None, H_AN)
        if jbody(b)["processing_status"] == "ended": break
        time.sleep(0.5)
    s, b = call("api.anthropic.com", "GET", f"/v1/messages/batches/{bid}/results", None, H_AN)
    say(step="results read", status=s, lines=b.count(b"\n"))


def batch():
    openai_batch(20, 2000)
    openai_batch(400, 4000)
    anthropic_batch(10, 2000)
    anthropic_batch(10, 2000, read=False)
    time.sleep(3)


# ---- a budget reached in the middle of a long answer ----

def midstream():
    """Small calls bring the budget near its limit, then one call reasons at length."""
    msgs = [{"role": "user", "content": "warm up THINK=4000"}]
    for k in range(4):
        s, b = call("api.anthropic.com", "POST", "/v1/messages", {"model": "claude-opus-5-5", "max_tokens": 32000, "stream": True, "messages": msgs}, H_AN)
        say(step=f"small {k}", status=s, said=silo_said(b))
    t0 = time.time()
    s, b = call("api.anthropic.com", "POST", "/v1/messages", {"model": "claude-opus-5-5", "max_tokens": 128000, "stream": True, "messages": [{"role": "user", "content": "the long one THINK=60000"}]}, H_AN)
    ended = "message_stop" in b.decode(errors="replace")
    say(step="long call", status=s, whole=ended, secs=round(time.time() - t0, 1), said=silo_said(b), tail=b[-300:].decode(errors="replace"))
    s, b = call("api.anthropic.com", "POST", "/v1/messages", {"model": "claude-opus-5-5", "max_tokens": 1000, "stream": True, "messages": msgs}, H_AN)
    say(step="after", status=s, said=silo_said(b))


# ---- two computers sharing a conversation ----

def fork():
    """Main works through a conversation, then a fork of main takes the same conversation up, and
    main goes on with it too. Each turn costs about the same."""
    hist = [{"role": "user", "content": text(30000, "the shared task ") + " THINK=3000"}]
    def turn(addr, who, k):
        nonlocal_hist = hist_of[who]
        s, b = call("api.anthropic.com", "POST", "/v1/messages", {"model": "claude-opus-5-5", "max_tokens": 8000, "messages": nonlocal_hist}, H_AN if addr == MAIN else {"x-api-key": "sk-ant-api03-fork-fake", "anthropic-version": "2023-06-01"}, addr=addr)
        say(step=f"{who} turn {k}", status=s, said=silo_said(b))
        if s == 200:
            hist_of[who] = nonlocal_hist + [{"role": "assistant", "content": f"{who} did {k}"}, {"role": "user", "content": f"go on {k} THINK=3000"}]
        return s
    hist_of = {"main": hist}
    for k in range(3): turn(MAIN, "main", k)
    hist_of["fork"] = list(hist_of["main"])  # the fork is copied from main here
    for k in range(6):
        if turn(FORK, "fork", k) != 200: break
    for k in range(3, 6):
        if turn(MAIN, "main", k) != 200: break


CASES = {"prev_id": prev_id, "background": background, "websearch": websearch, "chat": chat, "cache_expiry": cache_expiry,
         "gcache": gcache, "batch": batch, "midstream": midstream, "fork": fork}

if __name__ == "__main__":
    CASES[sys.argv[1] if len(sys.argv) > 1 else os.environ["CASE"]]()
