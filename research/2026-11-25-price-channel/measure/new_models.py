"""How often a new model of Anthropic, OpenAI or Google launched below a fraction of the cheapest
model its provider already sold, from OpenRouter's catalogue. This is the rule an online key would
meet when it adds a model: a new row priced under that line would need the root key.

usage: new_models.py <openrouter models.json> [since YYYY-MM-DD]
OpenRouter's catalogue holds only the models it still sells, so 'already sold' misses retired
models; for the cheapest price that makes the line lower, never higher, than it really was."""
import json, sys, datetime

since = datetime.date.fromisoformat(sys.argv[2] if len(sys.argv) > 2 else "2025-01-01")
ms = []
for m in json.load(open(sys.argv[1]))["data"]:
    i = m["id"]
    if ":" in i: continue
    p, n = i.split("/", 1)
    if p not in ("anthropic", "openai", "google"): continue
    if "text" not in m["architecture"]["output_modalities"]: continue
    try: pin, pout = float(m["pricing"]["prompt"]) * 1e6, float(m["pricing"]["completion"]) * 1e6
    except Exception: continue
    if pin <= 0: continue
    ms.append((datetime.datetime.fromtimestamp(m["created"], datetime.UTC).date(), p, n, pin, pout))
ms.sort()
for frac in (0.5, 0.25):
    tot = below = 0; worst = []
    for k, (d, p, n, pin, pout) in enumerate(ms):
        if d < since: continue
        before = [x for x in ms[:k] if x[1] == p and x[0] < d]
        if not before: continue
        tot += 1
        cin, cout = min(x[3] for x in before), min(x[4] for x in before)
        r = min(pin / cin, pout / cout)
        if r < frac:
            below += 1; worst.append(f"{d} {p}/{n} at {r:.2f}x the cheapest")
    print(f"line at {frac}x the provider's cheapest: {below} of {tot} new models since {since} fall under it")
    for w in worst: print("   ", w)
