"""Compare what Claude Code reported about itself over OpenTelemetry (otlp.jsonl) with what the
fake provider served (trace.jsonl), call by call and in total.
usage: compare.py <out-dir>"""
import json, sys

d = sys.argv[1]
served = [json.loads(l) for l in open(f"{d}/trace.jsonl")]
told = [json.loads(l) for l in open(f"{d}/otlp.jsonl")]
reqs = [r["attrs"] for r in told if r["kind"] == "log" and r["attrs"].get("event.name") == "api_request"]
metrics = {}
for r in told:
    if r["kind"] == "metric":
        key = (r["name"], r["attrs"].get("type"))
        metrics[key] = metrics.get(key, 0) + r["value"]   # deltas: a sum over exports


def tok(u):
    return (u["input_tokens"], u["output_tokens"], u["cache_read_input_tokens"],
            u["cache_creation_input_tokens"])


def tok_otel(a):
    return (int(a["input_tokens"]), int(a["output_tokens"]), int(a["cache_read_tokens"]),
            int(a["cache_creation_tokens"]))


s_tok = [tok(r["response"]["usage"]) for r in served]
o_tok = [tok_otel(a) for a in reqs]
matched = sum(1 for t in o_tok if t in s_tok)
tot = lambda L: tuple(sum(x[i] for x in L) for i in range(4))
print(f"calls served {len(served)}, api_request events {len(reqs)}, events matching a served call exactly {matched}")
print(f"tokens served   (in, out, cache read, cache write) {tot(s_tok)}")
print(f"tokens reported (in, out, cache read, cache write) {tot(o_tok)}")
print(f"cost reported in events ${sum(float(a['cost_usd']) for a in reqs):.6f}, "
      f"in the cost metric ${metrics.get(('claude_code.cost.usage', None), 0):.6f}")
print("models asked", sorted({r['request']['model'] for r in served}),
      "answered", sorted({r['response']['model'] for r in served}),
      "in events", sorted({a['model'] for a in reqs}))
print("window figures in any event or metric:",
      any("utiliz" in k or "ratelimit" in k for r in told for k in r["attrs"]))
