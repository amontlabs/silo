"""Reads the providers' own price pages and two independent catalogues, and writes what each says
as rows of Silo's price table (the format of research/2026-11-24-spend-budgets/prices.json).

usage: fetch.py <out dir>          writes <out>/pages/* (what was read) and <out>/sources.json
       fetch.py <out dir> --cached reads <out>/pages/* again without the network

Sources, one key each in sources.json:
  anthropic   platform.claude.com/docs/en/about-claude/pricing.md   (the provider's page)
  openai      developers.openai.com/api/docs/pricing.md             (the provider's page)
  google      ai.google.dev/gemini-api/docs/pricing.md.txt          (the provider's page)
  openrouter  openrouter.ai/api/v1/models                           (a reseller's JSON, at cost)
  bedrock     AWS Price List, AmazonBedrockFoundationModels         (Claude on Bedrock, JSON)
A row says where it was read (`source`) and on which day (`checked`).
"""
import json, os, re, sys, urllib.request, datetime

URLS = {
    "anthropic": "https://platform.claude.com/docs/en/about-claude/pricing.md",
    "openai": "https://developers.openai.com/api/docs/pricing.md",
    "google": "https://ai.google.dev/gemini-api/docs/pricing.md.txt",
    "openrouter": "https://openrouter.ai/api/v1/models",
    "bedrock": "https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonBedrockFoundationModels/current/index.json",
}
TODAY = datetime.date.today().isoformat()


def get(out, name, cached):
    path = f"{out}/pages/{name}"
    if not cached:
        os.makedirs(f"{out}/pages", exist_ok=True)
        req = urllib.request.Request(URLS[name], headers={"user-agent": "silo-price-check"})
        with urllib.request.urlopen(req, timeout=120) as r, open(path, "wb") as f:
            f.write(r.read())
    return open(path, encoding="utf-8").read()


def money(s):
    m = re.search(r"\$\s*([0-9][0-9,]*(?:\.[0-9]+)?)", s)
    return float(m.group(1).replace(",", "")) if m else None


def md_tables(text):
    """Every markdown table under a heading: (heading, header cells, rows of cells)."""
    out, head, lines = [], "", text.splitlines()
    i = 0
    while i < len(lines):
        l = lines[i]
        if l.startswith("#"): head = l.strip("# ").strip()
        if l.startswith("|") and i + 1 < len(lines) and re.match(r"^\|\s*:?-", lines[i + 1]):
            cells = [c.strip() for c in l.strip().strip("|").split("|")]
            rows = []; i += 2
            while i < len(lines) and lines[i].startswith("|"):
                rows.append([c.strip() for c in lines[i].strip().strip("|").split("|")]); i += 1
            out.append((head, cells, rows)); continue
        i += 1
    return out


# ---- Anthropic ----

def anthropic_id(name):
    name = name.split(" (")[0].strip()                          # "(limited availability)" etc.
    return "claude-" + "-".join(name.lower().replace("claude ", "").replace(".", "-").split())


def anthropic(text):
    src = URLS["anthropic"]; rows = []; base = {}
    for head, cells, body in md_tables(text):
        if cells[:2] == ["Model", "Base input tokens"]:
            for c in body:
                long = "over 100,000" in c[0]
                mid = anthropic_id(c[0].split("(for prompts")[0])
                r = {"provider": "anthropic", "model": mid, "input": money(c[1]), "output": money(c[5]), "cache_read": money(c[4]),
                     "cache_write_short": money(c[2]), "cache_write_long": money(c[3]), "web_search_per_1k": 10}
                if long:
                    b = base[mid]
                    b["long_above"] = 100000; b["long_input_mult"] = round(r["input"] / b["input"], 6); b["long_output_mult"] = round(r["output"] / b["output"], 6)
                    continue
                base[mid] = r; rows.append(r)
        elif head == "Fast mode pricing" and cells[0] == "Model":
            for c in body:
                for part in c[0].split(" / "):
                    mid = anthropic_id(part)
                    b = base.get(mid)
                    if not b: continue
                    f = money(c[1]) / b["input"]
                    rows.append({"provider": "anthropic", "model": mid, "tier": "fast", "input": money(c[1]), "output": money(c[2]),
                                 "cache_read": round(b["cache_read"] * f, 6), "cache_write_short": round(b["cache_write_short"] * f, 6), "cache_write_long": round(b["cache_write_long"] * f, 6)})
        elif head == "Batch processing" and cells[0] == "Model":
            for c in body:
                if "over 100,000" in c[0]: continue
                mid = anthropic_id(c[0].split("(for prompts")[0])
                b = base.get(mid)
                if not b: continue
                f = money(c[1]) / b["input"]
                r = {"provider": "anthropic", "model": mid, "tier": "batch", "input": money(c[1]), "output": money(c[2]),
                     "cache_read": round(b["cache_read"] * f, 6), "cache_write_short": round(b["cache_write_short"] * f, 6), "cache_write_long": round(b["cache_write_long"] * f, 6)}
                for k in ("long_above", "long_input_mult", "long_output_mult"):
                    if k in b: r[k] = b[k]
                rows.append(r)
    for r in rows: r.update(source=src, checked=TODAY)
    # US-only inference: 1.1x on every token price, Claude 4.6 and later (the page's "Data residency pricing").
    newer = [r["model"] for r in rows if r.get("tier", "standard") == "standard" and later_than_46(r["model"])]
    mult = [{"provider": "anthropic", "models": sorted(set(newer)), "when": "inference_geo=us", "factor": 1.1}] if "1.1x multiplier" in text else []
    return rows, mult


def later_than_46(mid):
    m = re.match(r"claude-(?:opus|sonnet|haiku|fable|mythos)-(\d+)(?:-(\d+))?$", mid)
    if not m: return False
    v = (int(m.group(1)), int(m.group(2) or 0))
    return v >= (4, 6)


# ---- OpenAI ----

TIERS_OA = {"Standard pricing data": "standard", "Batch pricing data": "batch", "Flex pricing data": "flex", "Fast pricing data": "priority"}


def openai(text):
    src = URLS["openai"]; rows = []
    for head, cells, body in md_tables(text):
        tier = TIERS_OA.get(head)
        if not tier or not cells or cells[0] != "Model": continue
        ix = {c: k for k, c in enumerate(cells)}
        for c in body:
            name = c[0]; above = 272000
            m = re.match(r"(\S+)\s*\(<(\d+)K context length\)", name)
            if m: name, above = m.group(1), int(m.group(2)) * 1000
            def col(k):
                return money(c[ix[k]]) if k in ix and k in ix and ix[k] < len(c) else None
            inp, out = col("Short context input"), col("Short context output")
            if inp is None or out is None: continue
            r = {"provider": "openai", "model": name, "input": inp, "output": out, "cache_read": col("Short context cached input") or 0,
                 "cache_write_short": col("Short context 30m write") or 0, "cache_write_long": col("Short context 12h write") or 0}
            if tier != "standard": r["tier"] = tier
            li, lo = col("Long context input"), col("Long context output")
            if li and lo:
                r["long_above"] = above; r["long_input_mult"] = round(li / inp, 6); r["long_output_mult"] = round(lo / out, 6)
            r.update(source=src, checked=TODAY); rows.append(r)
    mult = []
    if "10% uplift for models released on or after March 5, 2026" in text:
        mult.append({"provider": "openai", "models": [], "when": "regional_endpoint", "factor": 1.1,
                     "note": "models released on or after 2026-03-05 only; the boundary would need each model's release day"})
    return rows, mult


# ---- Google (Gemini API) ----

TABS_G = {"Standard": "standard", "Batch": "batch", "Flex": "flex", "Priority": "priority"}
MONTHS = {m: i for i, m in enumerate(["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"], 1)}


def gdate(s):
    m = re.search(r"(January|February|March|April|May|June|July|August|September|October|November|December) (\d+), (\d{4})", s)
    return datetime.date(int(m.group(3)), MONTHS[m.group(1)], int(m.group(2))) if m else None


def gcell(cell):
    """A paid-tier cell: [(price, from_day, long?)]; 'through X' and 'starting Y' give dated prices."""
    out = []
    for line in re.split(r"\s{2,}\n|\n", cell):
        p = money(line)
        if p is None or "per hour" in line or "storage" in line: continue
        # Text prices only: audio, image output and per-image lines are other units.
        if re.search(r"audio|per image|image output|\(image|video\)$", line, re.I) and "text" not in line: continue
        frm = None
        if "starting" in line: frm = gdate(line).isoformat()
        long = ">" in line and "<=" not in line
        out.append((p, frm, long))
    return out


def google(text):
    src = URLS["google"]; rows = []
    for sec in re.split(r"\n## ", text)[1:]:
        m = re.search(r"\{% id=\"([^\"]+)\" %\}", sec.splitlines()[0])
        if not m: continue
        mid = m.group(1)
        if not mid.startswith("gemini"): continue
        for tm in re.finditer(r'\{% tab title="([^"]+)" %\}(.*?)\{% /tab %\}', sec, re.S):
            tier = TABS_G.get(tm.group(1))
            if not tier: continue
            cells = {}
            for item in tm.group(2).split("---"):
                parts = [p.strip() for p in re.split(r"\n\s*\* ", "\n" + item) if p.strip()]
                if len(parts) >= 3: cells[parts[0].split("{%")[0].strip()] = parts[2]
            inp = next((v for k, v in cells.items() if k.startswith("Input price")), None)
            out = next((v for k, v in cells.items() if k.startswith("Output price")), None)
            if not inp or not out or "Free" in inp.split("\n")[0] and money(inp) is None: continue
            ci, co = gcell(inp), gcell(out)
            cache = gcell(next((v for k, v in cells.items() if k.startswith("Context caching")), "") or "")
            store = re.search(r"\$([0-9.]+) / 1,000,000 tokens per hour", next((v for k, v in cells.items() if k.startswith("Context caching")), "") or "")
            # Dated prices: one row per day of effect.
            days = sorted({d for (_, d, _) in ci + co if d} | {None}, key=lambda d: d or "")
            for d in days:
                def at(lst, long):
                    best = None
                    for p, frm, lg in lst:
                        if lg != long: continue
                        if frm is None or (d is not None and frm <= d): best = p
                    return best
                i0, o0 = at(ci, False), at(co, False)
                if i0 is None or o0 is None: continue
                r = {"provider": "google", "model": mid, "input": i0, "output": o0, "cache_read": at(cache, False) or 0}
                if tier != "standard": r["tier"] = tier
                if d: r["from"] = d
                il, ol = at(ci, True), at(co, True)
                if il and ol:
                    r["long_above"] = 200000; r["long_input_mult"] = round(il / i0, 6); r["long_output_mult"] = round(ol / o0, 6)
                if store: r["cache_storage_per_hour"] = float(store.group(1))
                if "per 1,000 requests" in sec and tier == "standard":
                    g = re.search(r"\$(\d+) per 1,000 requests", sec)
                    if g: r["web_search_per_1k"] = float(g.group(1))
                r.update(source=src, checked=TODAY); rows.append(r)
    return rows, []


# ---- OpenRouter: the first-party models it resells at the provider's price ----

def openrouter(text):
    rows = []
    for m in json.loads(text)["data"]:
        mid = m["id"]; prov, _, name = mid.partition("/")
        if prov not in ("anthropic", "openai", "google"): continue
        tier = "standard"
        if name.endswith(":batch"): tier, name = "batch", name[:-6]
        elif ":" in name: continue
        p = m["pricing"]
        def f(k):
            return float(p[k]) * 1e6 if p.get(k) not in (None, "") else 0
        if f("prompt") <= 0: continue
        if prov == "anthropic": name = name.replace(".", "-")
        r = {"provider": prov, "model": name, "input": round(f("prompt"), 6), "output": round(f("completion"), 6), "cache_read": round(f("input_cache_read"), 6)}
        if prov == "anthropic": r.update(cache_write_short=round(f("input_cache_write"), 6), cache_write_long=round(f("input_cache_write_1h"), 6))
        if tier != "standard": r["tier"] = tier
        ov = (p.get("overrides") or [{}])[0]
        if ov.get("min_prompt_tokens"):
            r["long_above"] = ov["min_prompt_tokens"]; r["long_input_mult"] = round(float(ov["prompt"]) * 1e6 / r["input"], 6)
            r["long_output_mult"] = round(float(ov.get("completion", p["completion"])) * 1e6 / r["output"], 6)
        r.update(source=URLS["openrouter"] + "#" + mid, checked=TODAY); rows.append(r)
    return rows, []


# ---- Claude on Bedrock: AWS's own price list ----

def bedrock(text):
    d = json.loads(text); terms = d["terms"]["OnDemand"]; got = {}
    kinds = {"Input tokens": "input", "Output tokens": "output", "Prompt cache read input tokens": "cache_read", "Prompt cache write input tokens": "cache_write_short"}
    for sku, p in d["products"].items():
        a = p["attributes"]
        if a.get("provider") != "Anthropic" or a.get("regionCode") != "us-east-1": continue
        k = kinds.get(a.get("inferenceType"))
        ut = a.get("usagetype", "")
        if k == "cache_write_short" and "_1h_" in ut: k = "cache_write_long"
        if not k or "_standard-Units" not in ut or "batch" in ut.lower() or "long" in ut.lower(): continue
        where = "global" if "_global_standard" in ut else "regional"
        for t in terms.get(sku, {}).values():
            for pd in t["priceDimensions"].values():
                if pd["unit"] == "1M tokens": got.setdefault((a["model"], where), {})[k] = float(pd["pricePerUnit"]["USD"])
    rows = []
    for (model, where), v in sorted(got.items()):
        if "input" not in v or "output" not in v: continue
        rows.append(dict({"provider": "bedrock", "model": anthropic_id(model), "tier": "standard" if where == "global" else "regional"}, **v,
                         source=URLS["bedrock"] + f" ({d.get('publicationDate')}, us-east-1)", checked=TODAY))
    return rows, []


def main():
    out = sys.argv[1]; cached = "--cached" in sys.argv
    res = {"checked": TODAY, "sources": {}}
    for name, fn in [("anthropic", anthropic), ("openai", openai), ("google", google), ("openrouter", openrouter), ("bedrock", bedrock)]:
        try:
            rows, mult = fn(get(out, name, cached))
            res["sources"][name] = {"url": URLS[name], "rows": rows, "multipliers": mult}
            print(f"{name}: {len(rows)} rows, {len(mult)} multipliers")
        except Exception as e:
            res["sources"][name] = {"url": URLS[name], "error": repr(e)}
            print(f"{name}: FAILED {e!r}")
    json.dump(res, open(f"{out}/sources.json", "w"), indent=1)


if __name__ == "__main__":
    main()
