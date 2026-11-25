"""New models of Anthropic, OpenAI and Google, from OpenRouter's catalogue, against LiteLLM's table.

usage: catalogue.py <openrouter models.json> <events.json from walk_litellm.py>

Prints: new text models per month; how many days after OpenRouter listed a model LiteLLM's table
had a price for it; how a new model's price compared with its family's previous model and with the
dearest model its provider already sold (what a guess or a bound for an unknown model would have
done). OpenRouter's `created` is the day it listed the model, which for these three providers is
the launch day or the day after. Its catalogue holds the models it still sells, so a model retired
since is missing from "the dearest its provider already sold".
"""
import json, sys, re, datetime, statistics, collections

orm = json.load(open(sys.argv[1]))["data"]
ev = json.load(open(sys.argv[2]))["events"]
first = {}
for e in ev:
    if e["kind"] == "new" and e["model"] not in first: first[e["model"]] = e["day"]

ms = []
for m in orm:
    i = m["id"]
    if ":" in i: continue
    p, n = i.split("/", 1)
    if p not in ("anthropic", "openai", "google"): continue
    if "text" not in m["architecture"]["output_modalities"]: continue
    try: pin, pout = float(m["pricing"]["prompt"]) * 1e6, float(m["pricing"]["completion"]) * 1e6
    except Exception: continue
    if pin <= 0: continue
    ms.append((m["created"], p, n, pin, pout))
ms.sort()
day = lambda c: datetime.datetime.fromtimestamp(c, datetime.UTC).date()

print("== new text models per month (OpenRouter's listing day)")
per = collections.Counter((day(c).strftime("%Y-%m"), p) for c, p, *_ in ms if day(c) >= datetime.date(2025, 12, 1))
for mo in sorted({k[0] for k in per}):
    print(f"  {mo}: " + ", ".join(f"{p} {per[(mo, p)]}" for p in ("anthropic", "openai", "google")) + f"  (all {sum(per[(mo, p)] for p in ('anthropic', 'openai', 'google'))})")
six = [x for x in ms if day(x[0]) > datetime.date(2026, 5, 25)]
print(f"  last six months: {len(six)} models, {len(six) / 26:.1f} a week")

print("== days from OpenRouter's listing to a price in LiteLLM's table (since June 2025)")
lags = []; missing = []
for c, p, n, *_ in ms:
    if day(c) < datetime.date(2025, 6, 1): continue
    cands = {"anthropic": [n.replace(".", "-")], "openai": [n], "google": ["gemini/" + n, n]}[p]
    k = next((x for x in cands if x in first), None)
    if k is None: missing.append(f"{p}/{n}"); continue
    lags.append((datetime.date.fromisoformat(first[k]) - day(c)).days)
lags.sort()
print(f"  {len(lags)} models: median {statistics.median(lags)} days, 90th percentile {lags[int(len(lags) * .9)]}, max {lags[-1]}; never priced: {len(missing)} ({', '.join(missing[:6])}...)")

print("== a new model's price against what was known before it (since January 2025)")
since = datetime.datetime(2025, 1, 1, tzinfo=datetime.UTC).timestamp()
fam = lambda p, n: p + ":" + re.sub(r"[-.]?\d+(\.\d+)*", "", n.replace("claude-", "").replace("gemini-", "").replace("gpt-", ""))
last = {}; succ = collections.Counter(); under = []
above = []; tot = 0; worst = 0
for idx, (c, p, n, pin, pout) in enumerate(ms):
    f = fam(p, n)
    if c >= since:
        if f in last:
            _, pi, po = last[f]
            r = max(pin / pi, pout / po)
            succ["dearer" if r > 1.001 else "cheaper or same"] += 1
            if r > 1.001: under.append((n, round(r, 2)))
        prev = [x for x in ms[:idx] if x[1] == p]
        if prev:
            tot += 1
            mi, mo = max(x[3] for x in prev), max(x[4] for x in prev)
            r = max(pin / mi, pout / mo)
            if r > 1: above.append((str(day(c)), n, round(r, 2)))
    last[f] = (n, pin, pout)
print(f"  priced as its family's previous model: under-counted {succ['dearer']} of {sum(succ.values())} successors; by up to {max(r for _, r in under)}x: {under}")
print(f"  priced at the dearest model its provider sold: under-counted {len(above)} of {tot}; {above}")
