# Walks LiteLLM's price file (model_prices_and_context_window.json) through its first-parent git
# history and writes every new model id and every price change for the first-party providers Silo
# meters (Anthropic, OpenAI, Google's Gemini API).
#
# usage: walk_litellm.py <litellm clone> <blobs file: "<blob> <commit> <day>" per line, newest first> <out.json>
# measure.sh makes the blobs file.
import json, subprocess, sys, collections
repo, blobs_file, out = sys.argv[1], sys.argv[2], sys.argv[3]
rows = [l.split() for l in open(blobs_file)]
rows.reverse()  # oldest first
PROV = {"anthropic": "anthropic", "openai": "openai", "gemini": "google", "text-completion-openai": None}
FIELDS = ["input_cost_per_token", "output_cost_per_token", "cache_read_input_token_cost", "cache_creation_input_token_cost",
          "input_cost_per_token_above_200k_tokens", "output_cost_per_token_above_200k_tokens"]
p = subprocess.Popen(["git", "-C", repo, "cat-file", "--batch"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
def get(oid):
    p.stdin.write((oid + "\n").encode()); p.stdin.flush()
    hdr = p.stdout.readline().split()
    n = int(hdr[2]); data = p.stdout.read(n); p.stdout.read(1)
    return data
prev = {}
events = []
bad = 0
for blob, commit, day in rows:
    try:
        d = json.loads(get(blob))
    except Exception:
        bad += 1; continue
    cur = {}
    for k, v in d.items():
        if not isinstance(v, dict): continue
        prov = PROV.get(v.get("litellm_provider"))
        if not prov: continue
        if v.get("mode") not in (None, "chat", "responses", "completion"): continue
        cur[k] = (prov, {f: v.get(f) for f in FIELDS})
    for k, (prov, pr) in cur.items():
        if k not in prev:
            events.append({"day": day, "commit": commit, "kind": "new", "provider": prov, "model": k, "prices": pr})
        else:
            old = prev[k][1]
            for f in FIELDS:
                if old.get(f) is not None and pr.get(f) is not None and abs(old[f] - pr[f]) > 1e-15:
                    events.append({"day": day, "commit": commit, "kind": "change", "provider": prov, "model": k, "field": f, "old": old[f], "new": pr[f]})
    for k in prev:
        if k not in cur:
            events.append({"day": day, "commit": commit, "kind": "gone", "provider": prev[k][0], "model": k})
    prev = cur
json.dump({"unparsable_versions": bad, "events": events}, open(out, "w"), indent=0)
print("versions", len(rows), "unparsable", bad, "events", len(events))
