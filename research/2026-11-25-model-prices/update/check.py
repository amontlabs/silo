"""Compares price rows from two places, field by field.

usage: check.py <sources.json> pages-vs-openrouter     each provider's page against OpenRouter
       check.py <sources.json> table <prices.json>      a price table against the providers' pages

Prints one line per row that differs, and a count. A row only one side has is listed apart.
"""
import json, sys

FIELDS = ["input", "output", "cache_read", "cache_write_short", "cache_write_long", "long_above", "long_input_mult", "long_output_mult", "web_search_per_1k", "cache_storage_per_hour"]


def key(r):
    return (r["provider"], r["model"], r.get("tier", "standard"), r.get("from", ""))


def index(rows):
    return {key(r): r for r in rows}


def same(a, b):
    if a is None or b is None: return True          # a field one side does not give
    return abs(float(a) - float(b)) <= 1e-9 + 1e-6 * abs(float(b))


def diff(a, b, fields=FIELDS):
    return [(f, a.get(f), b.get(f)) for f in fields if f in a and f in b and not same(a.get(f), b.get(f))]


def compare(left, right, lname, rname, only_left_models=None):
    L, R = index(left), index(right)
    n = differ = 0; only_l, only_r = [], []
    for k, a in sorted(L.items()):
        if only_left_models and k[1] not in only_left_models: continue
        b = R.get(k)
        if b is None: only_l.append(k); continue
        n += 1
        d = diff(a, b)
        if d:
            differ += 1
            print(f"DIFFERS {'/'.join(x for x in k if x)}: " + ", ".join(f"{f} {lname}={x} {rname}={y}" for f, x, y in d))
    for k in R:
        if k not in L and (not only_left_models or k[1] in only_left_models): only_r.append(k)
    print(f"{n} rows on both sides, {differ} differ; {len(only_l)} only in {lname}, {len(only_r)} only in {rname}")
    return only_l, only_r


def main():
    src = json.load(open(sys.argv[1]))["sources"]
    if sys.argv[2] == "pages-vs-openrouter":
        orr = src["openrouter"]["rows"]
        for prov in ("anthropic", "openai", "google"):
            print(f"== {prov}: page against OpenRouter")
            page = [r for r in src[prov]["rows"] if r.get("tier", "standard") in ("standard", "batch") and not r.get("from")]
            mine = {r["model"] for r in page}
            theirs = [r for r in orr if r["provider"] == prov and r["model"] in mine]
            # OpenRouter does not give web search per call the same way, nor cache storage, nor 1h writes for OpenAI.
            only_l, _ = compare(page, theirs, "page", "openrouter")
        print("== Claude on Bedrock (global endpoint) against Anthropic's page")
        bed = [dict(r, provider="anthropic") for r in src["bedrock"]["rows"] if r["tier"] == "standard"]
        compare(bed, [r for r in src["anthropic"]["rows"] if r.get("tier", "standard") == "standard"], "bedrock", "page")
        reg = [r for r in src["bedrock"]["rows"] if r["tier"] == "regional"]
        glob = index([dict(r, tier="regional") for r in src["bedrock"]["rows"] if r["tier"] == "standard"])
        ratios = sorted({round(r["input"] / glob[key(r)]["input"], 4) for r in reg if key(r) in glob})
        print(f"Bedrock regional input over global, per model: {ratios}")
    elif sys.argv[2] == "table":
        table = json.load(open(sys.argv[3]))["rows"]
        page = [r for p in ("anthropic", "openai", "google") if "rows" in src[p] for r in src[p]["rows"]]
        compare(table, page, "table", "page", only_left_models={r["model"] for r in table})


if __name__ == "__main__":
    main()
