"""A stand-in for Google's Code Assist API (cloudcode-pa.googleapis.com, v1internal), as Gemini CLI
uses it when signed in with a Google account. It keeps a daily quota per model counted in model
requests, as Google's quota page describes, answers retrieveUserQuota from it, and logs every
request it serves to <OUT>/served.jsonl: the method, the model, and whether it is a model request.

The main conversation is scripted: the model asks for STEPS shell commands, then answers.
Requests that ask for JSON (the router, the next-speaker check, loop detection) get a JSON answer.

Run: OUT=out PORT=8500 STEPS=6 [TLS_CERT=c.pem TLS_KEY=k.pem] [DELAY=0.3] python3 fake_code_assist.py
"""
import http.server, json, os, ssl, threading, time

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
STEPS = int(os.environ.get("STEPS", "6")); DELAY = float(os.environ.get("DELAY", "0.3"))
DAILY = int(os.environ.get("DAILY", "1500"))
LOG = open(f"{OUT}/served.jsonl", "a"); LOCK = threading.Lock()
used = {}  # model -> requests counted today
MODEL_METHODS = {"generateContent", "streamGenerateContent"}


def log(rec):
    with LOCK:
        LOG.write(json.dumps(rec) + "\n"); LOG.flush()


def usage(p, o):
    return {"promptTokenCount": p, "candidatesTokenCount": o, "totalTokenCount": p + o,
            "cachedContentTokenCount": 0}


def answer(req):
    r = req.get("request", {})
    gc = r.get("generationConfig", {}) or {}
    if gc.get("responseJsonSchema") or gc.get("responseSchema") or gc.get("responseMimeType") == "application/json":
        txt = json.dumps({"reasoning": "stand-in", "next_speaker": "user", "model_choice": "flash",
                          "complexity_reasoning": "stand-in", "complexity_score": 10,
                          "unproductive_state_confidence": 0.0, "unproductive_state_analysis": "none"})
        return [{"text": txt}], 40
    done = sum(1 for c in r.get("contents", []) for p in c.get("parts", []) if "functionResponse" in p)
    if done < STEPS:
        return [{"functionCall": {"name": "run_shell_command",
                                  "args": {"command": f"echo step {done + 1}",
                                           "description": "a step"}}}], 150
    return [{"text": "Done."}], 30


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def send_json(self, obj, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code); self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(b))); self.end_headers(); self.wfile.write(b)

    def do_GET(self):
        m = self.path.split(":")[-1].split("?")[0]
        log({"t": time.time(), "method": m, "model": None, "model_request": False})
        self.send_json({"freeTierDataCollectionOptin": False})

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length", 0) or 0))
        try:
            req = json.loads(body or b"{}")
        except Exception:
            req = {}
        m = self.path.split(":")[-1].split("?")[0]
        model = req.get("model")
        is_model = m in MODEL_METHODS
        if is_model:
            with LOCK:
                if used.get(model, 0) >= DAILY:
                    over = True
                else:
                    over = False; used[model] = used.get(model, 0) + 1
            log({"t": time.time(), "method": m, "model": model, "model_request": True, "refused": over})
            if over:
                return self.send_json({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED",
                    "message": "Quota exceeded", "details": [
                        {"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "QUOTA_EXHAUSTED",
                         "domain": "cloudcode-pa.googleapis.com"},
                        {"@type": "type.googleapis.com/google.rpc.QuotaFailure",
                         "violations": [{"quotaId": "GenerateRequestsPerDayPerProjectPerModel"}]},
                        {"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "3600s"}]}}, 429)
        else:
            log({"t": time.time(), "method": m, "model": model, "model_request": False})
        if m == "loadCodeAssist":
            return self.send_json({"currentTier": {"id": "standard-tier", "name": "Gemini Code Assist",
                                                   "userDefinedCloudaicompanionProject": False},
                                   "allowedTiers": [{"id": "standard-tier", "isDefault": True}],
                                   "cloudaicompanionProject": "stand-in-project"})
        if m == "onboardUser":
            return self.send_json({"done": True, "response": {"cloudaicompanionProject": {"id": "stand-in-project"}}})
        if m == "retrieveUserQuota":
            with LOCK:
                b = [{"modelId": k, "tokenType": "REQUESTS", "remainingAmount": str(DAILY - v),
                      "remainingFraction": (DAILY - v) / DAILY, "resetTime": "2026-11-26T08:00:00Z"}
                     for k, v in used.items()]
            return self.send_json({"buckets": b})
        if m == "countTokens":
            return self.send_json({"totalTokens": 100})
        if m in MODEL_METHODS:
            time.sleep(DELAY)
            parts, out = answer(req)
            resp = {"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP"}],
                    "usageMetadata": usage(1200, out), "modelVersion": model}
            if m == "generateContent":
                return self.send_json({"response": resp, "traceId": "t"})
            self.send_response(200); self.send_header("content-type", "text/event-stream")
            self.send_header("transfer-encoding", "chunked"); self.end_headers()
            for chunk in (json.dumps({"response": resp, "traceId": "t"}),):
                data = f"data: {chunk}\n\n".encode()
                self.wfile.write(b"%x\r\n%s\r\n" % (len(data), data))
            self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()
            return
        return self.send_json({})


srv = http.server.ThreadingHTTPServer(("127.0.0.1", int(os.environ.get("PORT", "8500"))), H)
if os.environ.get("TLS_CERT"):
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(os.environ["TLS_CERT"], os.environ["TLS_KEY"])
    srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
srv.serve_forever()
