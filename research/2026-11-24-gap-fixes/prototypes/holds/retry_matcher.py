"""What is "the same request" when an agent tries again after a hold ended?

It sends real requests twice, as an agent's retry would, to hold_server.py (HOLD=0, DUMP_DIR set),
and compares two fingerprints of each pair:

  coarse  method, host, path, and which secret the request would use (the placeholder in its
          credential header). This is what PR #350 proposes.
  exact   coarse, plus the query, the body in a canonical form (JSON keys sorted, form fields
          sorted, multipart parts without their random boundary, a push as its ref updates), and
          every header except a short list known to change on each try (date, user agent,
          content length, idempotency key, tracing ids).

Then it shows the bad path of the coarse fingerprint: a different charge, or a push of other
commits to the same branch, matches it.

Run: python3 retry_matcher.py <hold_server port> <dump dir> <git work dir> <stripe sdk dir>
"""
import base64, email.parser, email.policy, glob, hashlib, json, os, subprocess, sys, urllib.parse

PORT, DUMP, GITWD, SDK = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
BASE = f"http://127.0.0.1:{PORT}"
PH = "silo-ph-stripe-live-1"  # a placeholder, as `secret-placeholder` puts in the computer
VOLATILE = {"date", "user-agent", "content-length", "idempotency-key", "x-request-id", "traceparent",
            "tracestate", "x-stripe-client-user-agent", "x-stripe-client-telemetry", "accept-encoding",
            "connection", "host", "authorization", "content-type", "expect"}


def secret_of(h):
    a = h.get("Authorization") or h.get("authorization") or ""
    return "stripe-live" if PH in a else ("none" if not a else "other")


def canon_body(ctype, body):
    ctype = (ctype or "").lower()
    if "json" in ctype:
        try:
            return "json:" + json.dumps(json.loads(body), sort_keys=True)
        except ValueError:
            pass
    if "x-www-form-urlencoded" in ctype:
        return "form:" + json.dumps(sorted(urllib.parse.parse_qsl(body.decode(), keep_blank_values=True)))
    if "multipart/form-data" in ctype:
        msg = email.parser.BytesParser(policy=email.policy.HTTP).parsebytes(
            b"Content-Type: " + ctype.encode() + b"\r\n\r\n" + body)
        parts = sorted((p.get_param("name", header="content-disposition"), p.get_filename(),
                        p.get_content_type(), hashlib.sha256(p.get_payload(decode=True) or b"").hexdigest()[:16])
                       for p in msg.iter_parts())
        return "multipart:" + json.dumps(parts)
    if "git-receive-pack" in ctype:
        cmds, i = [], 0
        while i + 4 <= len(body):
            n = int(body[i:i + 4], 16)
            if n == 0:
                break
            cmds.append(body[i + 4:i + n].split(b"\0")[0].decode().strip()); i += n
        return "push:" + json.dumps(sorted(cmds))
    return "raw:" + hashlib.sha256(body).hexdigest()


def fingerprints(d):
    h = {k: v for k, v in d["headers"].items()}
    lower = {k.lower(): v for k, v in h.items()}
    url = urllib.parse.urlsplit(d["path"])
    coarse = (d["method"], d["host"], url.path, secret_of(h))
    body = base64.b64decode(d["body"])
    exact = coarse + (url.query, canon_body(lower.get("content-type"), body),
                      json.dumps(sorted((k, v) for k, v in lower.items() if k not in VOLATILE)))
    raw = hashlib.sha256(body).hexdigest()[:12]
    return coarse, exact, raw, lower.get("idempotency-key")


def run(cmd, cwd=None, env=None):
    before = set(glob.glob(f"{DUMP}/*.json"))
    subprocess.run(cmd, shell=True, cwd=cwd, env=dict(os.environ, **(env or {})), capture_output=True)
    new = sorted(set(glob.glob(f"{DUMP}/*.json")) - before)
    return [json.load(open(f)) for f in new]


AUTH = f"-H 'Authorization: Bearer {PH}'"
STRIPE_JS = f"""
const Stripe = require('stripe');
const s = new Stripe('{PH}', {{host: '127.0.0.1', port: {PORT}, protocol: 'http', maxNetworkRetries: 1}});
s.charges.create({{amount: 4000, currency: 'eur', source: 'tok_visa', description: 'Order 1182'}}).then(() => {{}}, () => {{}});
"""
open(f"{SDK}/charge.js", "w").write(STRIPE_JS)
PYREQ = f"import requests; requests.post('{BASE}/v1/refunds', json={{'charge': 'ch_1', 'amount': 4000, 'reason': 'requested_by_customer'}}, headers={{'Authorization': 'Bearer {PH}'}})"

cases = [
    ("curl form POST", f"curl -s -o /dev/null {AUTH} -X POST {BASE}/v1/charges -d amount=4000 -d currency=eur"),
    ("curl multipart upload", f"echo hello > /tmp/_up.txt; curl -s -o /dev/null {AUTH} -F purpose=dispute_evidence -F file=@/tmp/_up.txt {BASE}/v1/files"),
    ("curl JSON", f"curl -s -o /dev/null {AUTH} --json '{{\"amount\":4000,\"currency\":\"eur\"}}' {BASE}/v1/payment_intents"),
    ("python requests JSON", f"python3 -c \"{PYREQ}\""),
    ("stripe-node SDK charge", f"node {SDK}/charge.js"),
    ("git push, same commits", f"git push -q {BASE}/repo.git main"),
]
print(f"{'case':<26} {'coarse':>7} {'exact':>6} {'raw body':>9}  idempotency keys")
for name, cmd in cases:
    a = run(cmd, cwd=GITWD)
    b = run(cmd, cwd=GITWD)
    if not a or not b:
        print(f"{name:<26} no request reached the server"); continue
    fa, fb = fingerprints(a[-1]), fingerprints(b[-1])
    print(f"{name:<26} {'same' if fa[0]==fb[0] else 'DIFF':>7} {'same' if fa[1]==fb[1] else 'DIFF':>6} "
          f"{'same' if fa[2]==fb[2] else 'DIFF':>9}  {fa[3]} / {fb[3]}")

print("\nBad paths: a different request after the yes, compared with the one you saw")
first = run(f"curl -s -o /dev/null {AUTH} -X POST {BASE}/v1/charges -d amount=4000 -d currency=eur")[-1]
other = run(f"curl -s -o /dev/null {AUTH} -X POST {BASE}/v1/charges -d amount=990000 -d currency=eur")[-1]
f1, f2 = fingerprints(first), fingerprints(other)
print(f"  charge of 40.00 then 9900.00: coarse {'MATCHES' if f1[0]==f2[0] else 'differs'}, exact {'MATCHES' if f1[1]==f2[1] else 'differs'}")
p1 = run(f"git push -q {BASE}/repo.git main", cwd=GITWD)[-1]
subprocess.run("echo more >> f && git commit -qam 'more' ", shell=True, cwd=GITWD)
p2 = run(f"git push -q {BASE}/repo.git main", cwd=GITWD)[-1]
g1, g2 = fingerprints(p1), fingerprints(p2)
print(f"  push of main, then a push of main with one more commit: coarse {'MATCHES' if g1[0]==g2[0] else 'differs'}, exact {'MATCHES' if g1[1]==g2[1] else 'differs'}")
print(f"    first:  {g1[1][5]}\n    second: {g2[1][5]}")
subprocess.run("git reset -q --hard HEAD~1", shell=True, cwd=GITWD)
