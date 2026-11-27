"""The contract between Silo's boundary and one release of an agent program, checked on one run.

usage: check.py [--text] <run dir> [<baseline fingerprint.json>]

Reads what run.sh left: the stand-in's requests.jsonl and sent.jsonl (what the program sent and
what the model answered), the boundary's record.jsonl (what Silo read), lookups.txt (every name
the program looked up), and the program's exit. Each check names the plan entry that rests on it.
Prints a JSON report, or a table with --text. With a baseline, the run's fingerprint of the model
traffic is compared with it, and every difference is listed: a new release passes the contract
only when nothing the boundary reads changed, and a person looks at everything else that did.
"""
import hashlib, json, os, re, sys

TEXT = "--text" in sys.argv
args = [a for a in sys.argv[1:] if a != "--text"]
D = args[0]
BASE = json.load(open(args[1])) if len(args) > 1 else None


def jl(name):
    p = os.path.join(D, name)
    return [json.loads(l) for l in open(p) if l.strip()] if os.path.exists(p) else []


def rd(name):
    p = os.path.join(D, name)
    return open(p, errors="replace").read() if os.path.exists(p) else ""


prog, ver, scen, trust = rd("what.txt").split()
reqs, sent, rec = jl("requests.jsonl"), jl("sent.jsonl"), jl("record.jsonl")
m = re.search(r"exit=(\d+) secs=([\d.]+)", rd("result.txt"))
exit_code, secs = (int(m.group(1)), float(m.group(2))) if m else (None, None)
lookups = sorted({l.split()[1] for l in rd("lookups.txt").splitlines() if len(l.split()) > 2})

MODEL_PATH = re.compile(r"(/v1/messages(\?|$)|/responses(\?|$)|/chat/completions|:(stream)?[gG]enerateContent)")
model_reqs = [r for r in reqs if (r["method"] in ("POST", "WS")) and MODEL_PATH.search(r["path"]) and isinstance(r["body"], dict)
              and (r["method"] == "POST" or r["body"].get("type") == "response.create")]
# Hosts the boundary reads (#374's providerOf). A model call anywhere else is not read.
READ_HOSTS = {"api.anthropic.com", "api.openai.com", "chatgpt.com", "generativelanguage.googleapis.com"}
EXPECT_CRED = {"claude-code": "x-api-key:sk-ant-api", "claude-code-plan": "authorization:sk-ant-oat",
               "codex": "authorization:sk-proj-", "codex-openai-key": "authorization:sk-proj-", "gemini-cli": "x-goog-api-key:AIza"}[prog]

checks = []


def check(cid, entry, ok, detail):
    checks.append({"id": cid, "entry": entry, "result": {True: "pass", False: "fail", None: "n/a"}[ok], "detail": detail})


# ---- what the program does ----

if scen == "task":
    check("reaches-the-model", "reading-each-agent-program", exit_code == 0 and len(sent) > 0,
          f"exit {exit_code} in {secs} s, {len(sent)} model calls answered")
    proof = os.path.exists(os.path.join(D, "work", "contract-proof.txt"))
    check("runs-the-tool-call", "reading-each-agent-program", proof,
          "the shell tool call the stand-in sent ran" if proof else "contract-proof.txt was not written: the program did not take the stand-in's answer")

# ---- hosts and transports ----

refused = sorted({r.get("sni", "") for r in rec if r.get("kind") == "refused" and r.get("sni")})
hs_failed = sum(1 for r in rec if r.get("kind") == "handshake-failed")
model_hosts = sorted({r["host"] for r in model_reqs})
check("model-hosts-read", "program-that-cannot-be-read", all(h in READ_HOSTS for h in model_hosts) if model_hosts else None,
      f"model calls went to {', '.join(model_hosts) or 'no host'}")
transports = sorted({("websocket" if r["transport"] == "websocket" else
                      "sse" if (r["body"].get("stream") or "alt=sse" in r["path"] or ":streamGenerateContent" in r["path"]) else "json")
                     for r in model_reqs})
check("transports", "reading-each-agent-program", None, ", ".join(transports) or "none")
check("other-hosts", "reading-each-agent-program", None,
      f"looked up {', '.join(lookups)}; the boundary refused {', '.join(refused) or 'none'}; {hs_failed} handshakes refused Silo's certificate")

# ---- what the boundary read, against what was sent ----

calls = [r for r in rec if r.get("kind") == "call"]
wsc = [r for r in rec if r.get("kind") == "websocket-closed"]
unread = [r for r in rec if r.get("kind") in ("unreadable", "unreadable-request", "probe-failed")]
if scen == "task":
    http_sent = [s for s in sent if s["path"] and not any(r["transport"] == "websocket" and r["path"] == s["path"] for r in reqs if r["method"] == "WS")]
    ws_sent = [s for s in sent if s not in http_sent]

    def billed(s):
        u = s["usage"]
        if s["format"] == "anthropic": return u["input_tokens"] + u["cache_read_input_tokens"] + u["cache_creation_input_tokens"], u["output_tokens"]
        if s["format"] in ("responses",): return u["input_tokens"], u["output_tokens"]
        if s["format"] == "chat": return u["prompt_tokens"], u["completion_tokens"]
        return u["promptTokenCount"], u["candidatesTokenCount"] + u.get("thoughtsTokenCount", 0)

    def counted(c):
        return c["input"] + c["cache_read"] + c["cache_write_short"] + c["cache_write_long"], c["output"]

    b_http = sorted(billed(s) for s in http_sent)
    c_http = sorted(counted(c) for c in calls if c.get("final"))
    ok = b_http == c_http and len(calls) == len(http_sent)
    detail = f"{len(http_sent)} HTTP calls billed {sum(i for i, _ in b_http)} in / {sum(o for _, o in b_http)} out; the boundary counted {len(calls)} calls, {sum(i for i, _ in c_http)} in / {sum(o for _, o in c_http)} out"
    if ws_sent:
        bi, bo = sum(billed(s)[0] for s in ws_sent), sum(billed(s)[1] for s in ws_sent)
        ci, co = sum(max(0, w["input_tokens"]) for w in wsc), sum(max(0, w["output_tokens"]) for w in wsc)
        detail += f"; {len(ws_sent)} WebSocket calls billed {bi} in / {bo} out, counted {ci} in / {co} out on {len(wsc)} sockets, as no call of the ledger"
        ok = ok and False
    check("usage-counted", "what-a-budget-promises", ok if sent else None, detail)
    models_sent = sorted({s["model"] for s in http_sent})
    models_rec = sorted({c["model"] for c in calls})
    check("model-named", "what-traffic-gives", (models_sent == models_rec) if http_sent else None,
          f"sent {models_sent}, recorded {models_rec}")
    fmts = sorted({c["format"] for c in calls})
    check("format-known", "adapters-follow-providers", (not unread and (bool(calls) or bool(wsc))) if sent else None,
          f"formats read: {', '.join(fmts + (['responses over WebSocket'] if wsc else [])) or 'none'}; {len(unread)} unreadable")

    # Tool calls: each one the model sent is in the record with its id, name and input.
    tc_rec = {r["id"]: r for r in rec if r.get("kind") == "tool-call"}
    sent_calls = [c for s in sent for c in s["tool_calls"]]
    missing, wrong = [], []
    for c in sent_calls:
        r = tc_rec.get(c["id"]) or next((x for x in tc_rec.values() if x["input_sha256"] == c["input_sha256"]), None)
        if not r: missing.append(c["name"]); continue
        if r["name"] != c["name"] or r["input_sha256"] != c["input_sha256"]: wrong.append(f"{c['name']}: recorded {r['name']} {r['input_sha256'][:8]} for {c['input_sha256'][:8]}")
    check("tool-calls-read", "tool-calls-from-traffic", (not missing and not wrong) if sent_calls else None,
          f"{len(sent_calls)} sent, {len(sent_calls)-len(missing)} in the record" + (f"; missing: {missing}" if missing else "") + (f"; wrong: {wrong}" if wrong else ""))

    # Tool results come back in a later request, joined to their call.
    def result_ids(body):
        out = set()
        for msg in body.get("messages") or []:
            for b in (msg.get("content") if isinstance(msg.get("content"), list) else []):
                if isinstance(b, dict) and b.get("type") == "tool_result": out.add(b.get("tool_use_id"))
        inp = body.get("input")
        for it in (inp if isinstance(inp, list) else []):
            if isinstance(it, dict) and it.get("type", "").endswith("_output"): out.add(it.get("call_id"))
        for cont in (body.get("request", body).get("contents") or []):
            for p in cont.get("parts") or []:
                if isinstance(p, dict) and "functionResponse" in p:
                    out.add(p["functionResponse"].get("id") or p["functionResponse"].get("name"))
        return out
    back = set().union(*[result_ids(r["body"]) for r in model_reqs]) if model_reqs else set()
    joined = [c for c in sent_calls if c["id"] in back or c["name"] in back]
    check("tool-results-joined", "author-from-traffic", (len(joined) == len(sent_calls)) if sent_calls else None,
          f"{len(joined)} of {len(sent_calls)} results came back naming their call")

    # The command text is readable from the call's input (rules on a tool call's text, attribution).
    readable = [c for c in sent_calls if "contract-proof.txt" in (c["input"] or "")]
    check("command-text-readable", "rule-on-a-tool-calls-text", (len(readable) == len(sent_calls)) if sent_calls else None,
          f"{len(readable)} of {len(sent_calls)} calls carry the command as text: " + ", ".join(sorted({c['name'] for c in sent_calls})))

# ---- the conversation and the credential ----

def conv_key(r):
    b, h = r["body"], r["headers"]
    uid = (b.get("metadata") or {}).get("user_id")
    if uid:
        try: return "metadata.user_id.session_id", json.loads(uid).get("session_id")
        except Exception:
            mm = re.search(r"session_([0-9a-f-]{36})", uid)
            return "metadata.user_id (string)", mm.group(1) if mm else None
    if b.get("prompt_cache_key"): return "prompt_cache_key", b["prompt_cache_key"]
    for k in ("session-id", "session_id", "x-claude-code-session-id"):
        if h.get(k): return "header " + k, h[k]
    if r["method"] == "WS":
        up = next((x for x in reqs if x["method"] == "GET" and x["transport"] == "websocket"), None)
        if up:
            for k in ("session-id", "session_id"):
                if up["headers"].get(k): return "upgrade header " + k, up["headers"][k]
    return None, None

main_reqs = [r for r in model_reqs if r["body"].get("tools") or r["body"].get("input") and r["method"] == "WS" or (r["body"].get("request") or {}).get("tools")]
keys = [conv_key(r) for r in main_reqs]
where = sorted({k for k, _ in keys if k})
vals = {v for _, v in keys}
check("conversation-named", "recognising-a-conversation", (bool(where) and None not in vals and len(vals) == 1) if main_reqs else None,
      f"{len(main_reqs)} requests of the task's conversation; key in {', '.join(where) or 'nothing'}; {len(vals - {None})} distinct")
creds = sorted({r["credential"] for r in model_reqs})
if any(r["method"] == "WS" for r in model_reqs):
    creds = sorted({r["credential"] for r in model_reqs if r["method"] != "WS"} | {r["credential"] for r in reqs if r["transport"] == "websocket" and r["method"] == "GET"})
check("credential-kind", "claude-key-or-plan", creds == [EXPECT_CRED] if model_reqs else None,
      f"model requests carry {', '.join(creds) or 'nothing'}; expected {EXPECT_CRED}")

# ---- a refusal in the provider's shape stops the program and says why ----

if scen == "refuse":
    refusals = [r for r in rec if r.get("kind") == "refused" and r.get("why")]
    said = "Silo" in rd("out.txt") + rd("err.txt")
    check("stops-on-refusal", "budget-denies-or-holds", bool(refusals) and exit_code is not None and secs < 30 and len(refusals) <= 3 and said,
          f"{len(refusals)} refused ({', '.join(sorted({r['why'] for r in refusals}))}); exit {exit_code} after {secs} s; Silo's sentence {'shown' if said else 'not shown'}")

# ---- the fingerprint of the model traffic ----

def shape(o, path="", out=None, depth=0):
    """Key paths and types of a JSON value; lists merged; text values left out."""
    out = set() if out is None else out
    if depth > 7: return out
    if isinstance(o, dict):
        for k, v in o.items():
            if re.fullmatch(r"[0-9a-f-]{16,}|[A-Za-z0-9_/+.-]{40,}", k): k = "<id>"
            shape(v, f"{path}.{k}", out, depth + 1)
    elif isinstance(o, list):
        for v in o[:200]: shape(v, path + "[]", out, depth + 1)
    else:
        out.add(f"{path}:{type(o).__name__}")
    return out

fp = {"paths": set(), "headers": set(), "body": set(), "item_types": set(), "tool_kinds": set(), "tools": set(), "betas": set(), "transports": set(transports), "hosts": set(model_hosts)}
for r in model_reqs:
    fp["paths"].add(re.sub(r"/models/[^:/]+", "/models/<model>", r["path"]))
    fp["headers"] |= {k for k in r["headers"] if not k.startswith("sec-")}
    if r["method"] == "WS":
        fp["headers"] |= {k for x in reqs if x["transport"] == "websocket" and x["method"] == "GET" for k in x["headers"] if not k.startswith("sec-")}
    b = r["body"]
    fp["betas"] |= set(filter(None, (r["headers"].get("anthropic-beta") or r["headers"].get("openai-beta") or "").split(",")))
    for k, v in b.items():
        if k in ("messages", "input", "contents", "tools", "system", "systemInstruction", "instructions"):
            fp["body"].add(k)
        else:
            shape(v, k, fp["body"])
    for msg in b.get("messages") or []:
        for blk in (msg.get("content") if isinstance(msg.get("content"), list) else []):
            fp["item_types"].add("block:" + str(blk.get("type")))
    for it in (b.get("input") if isinstance(b.get("input"), list) else []):
        if isinstance(it, dict):
            fp["item_types"].add("item:" + str(it.get("type") or it.get("role")))
            if it.get("type") == "additional_tools":
                for ns in it.get("tools") or []:
                    for t in ns.get("tools") or [ns]:
                        fp["tools"].add(f"additional:{t.get('type')}:{t.get('name')}")
    for cont in (b.get("request", b).get("contents") or []):
        for p in cont.get("parts") or []:
            fp["item_types"] |= {"part:" + k for k in p}
    for t in b.get("tools") or b.get("request", {}).get("tools") or []:
        if not isinstance(t, dict): continue
        if "functionDeclarations" in t:
            for d in t["functionDeclarations"]: fp["tools"].add("function:" + d.get("name", "?")); fp["tool_kinds"].add("gemini:functionDeclarations")
        else:
            fp["tool_kinds"].add(str(t.get("type") or ("custom" if "input_schema" in t else "?")))
            fp["tools"].add(f"{t.get('type') or 'tool'}:{t.get('name')}")
fp = {k: sorted(v) for k, v in fp.items()}
json.dump(fp, open(os.path.join(D, "fingerprint.json"), "w"), indent=1, sort_keys=True)
# What the boundary reads: a change here fails the contract. Everything else is shown to a person.
READS = ("paths", "hosts", "transports", "item_types", "tool_kinds")
if BASE:
    diff = {k: {"new": sorted(set(fp.get(k, [])) - set(BASE.get(k, []))), "gone": sorted(set(BASE.get(k, [])) - set(fp.get(k, [])))} for k in set(fp) | set(BASE)}
    diff = {k: v for k, v in diff.items() if v["new"] or v["gone"]}
    check("shape-unchanged", "reading-each-agent-program", not any(k in READS for k in diff),
          "; ".join(f"{k}: +{len(v['new'])} -{len(v['gone'])}" for k, v in sorted(diff.items())) or "same as the baseline")
    json.dump(diff, open(os.path.join(D, "shape-diff.json"), "w"), indent=1, sort_keys=True)

report = {"program": prog, "version": ver, "scenario": scen, "trust": trust, "exit": exit_code, "secs": secs,
          "passed": all(c["result"] != "fail" for c in checks), "checks": checks}
if TEXT:
    print(f"{prog} {ver} {scen} trust={trust}: {'PASS' if report['passed'] else 'FAIL'}")
    for c in checks:
        print(f"  {c['result']:4} {c['id']:22} {c['detail']}  [{c['entry']}]")
else:
    print(json.dumps(report, indent=1))
