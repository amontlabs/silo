"""What the stand-in served against what Gemini CLI reported, and what the unread relay saw.
usage: python3 compare.py <out-dir>"""
import collections, json, os, sys

d = sys.argv[1]
rd = lambda f: [json.loads(l) for l in open(os.path.join(d, f))] if os.path.exists(os.path.join(d, f)) else []
served = rd("served.jsonl"); otlp = rd("otlp.jsonl"); relay = rd("relay.jsonl")
model = [s for s in served if s["model_request"]]
other = [s for s in served if not s["model_request"]]
print(f"served: {len(served)} requests, {len(model)} model requests "
      f"({sum(1 for s in model if s.get('refused'))} refused with 429), {len(other)} others")
for k, v in sorted(collections.Counter(s["model"] for s in model).items()):
    print(f"  model requests on the wire, {k}: {v}")
for k, v in sorted(collections.Counter(s["method"] for s in other).items()):
    print(f"  other requests, {k}: {v}")
ev = lambda n: [o["attrs"] for o in otlp if o["kind"] == "log" and o["attrs"].get("event.name") == n]
req, resp, err = ev("gemini_cli.api_request"), ev("gemini_cli.api_response"), ev("gemini_cli.api_error")
print(f"reported: {len(req)} api_request, {len(resp)} api_response, {len(err)} api_error")
for k, v in sorted(collections.Counter((a.get("model"), a.get("role")) for a in req).items()):
    print(f"  api_request as reported, {k[0]} ({k[1]}): {v}")
for a in err:
    print(f"  api_error: model {a.get('model')}, status {a.get('status_code')}, {str(a.get('error'))[:90]}")
print(f"output tokens reported: {sum(int(a.get('output_token_count', 0)) for a in resp)}")
if relay:
    print(f"unread relay: {len(relay)} connections, {sum(r['turns'] for r in relay)} turns, "
          f"{sum(r['up'] for r in relay)} bytes up, {sum(r['down'] for r in relay)} bytes down")
