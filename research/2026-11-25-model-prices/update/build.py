"""Builds the next release of Silo's price table from the providers' own pages, and lists every
price an independent source gives otherwise, for a person to settle before it is signed.

usage: build.py <sources.json> <previous release or -> <out file> [--strict]

- Rows: each provider's page (Anthropic, OpenAI, Google), text models only, every tier and every
  dated price the page announces. A dated price is a row with `from`; a later release that no
  longer has it withdraws it (as Anthropic withdrew Sonnet 5's increase of 1 September 2026).
- Multipliers: what the pages say applies on top (Anthropic's 1.1x for US-only inference).
- version: the previous release's plus one. check_by: 14 days after today.
- Disputes: a row whose price OpenRouter, which resells at the provider's price, gives otherwise.
  With --strict, any dispute on a text model stops the build.
"""
import json, sys, datetime, re

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from check import index, diff, key  # noqa: E402

NOT_TEXT = re.compile(r"image|tts|live|transcribe|omni|banana|genie|lyria|robotics|embedding|veo|imagen", re.I)


def main():
    src = json.load(open(sys.argv[1]))
    prev = None if sys.argv[2] == "-" else json.load(open(sys.argv[2]))
    out = sys.argv[3]; strict = "--strict" in sys.argv
    today = datetime.date.fromisoformat(src["checked"])
    rows, mults = [], []
    for p in ("anthropic", "openai", "google"):
        s = src["sources"][p]
        if "error" in s:
            sys.exit(f"{p}: the page could not be read ({s['error']}); no release without it")
        rows += [r for r in s["rows"] if not NOT_TEXT.search(r["model"])]
        mults += [m for m in s["multipliers"] if m.get("when") == "inference_geo=us"]
    orr = index(src["sources"].get("openrouter", {}).get("rows", []))
    disputes = []
    for r in rows:
        o = orr.get(key(r))
        if o:
            d = diff(r, o, ["input", "output", "cache_read"])
            if d: disputes.append({"row": "/".join(x for x in key(r) if x), "fields": [{"field": f, "page": a, "openrouter": b} for f, a, b in d]})
    version = (prev["version"] + 1) if prev else int(today.strftime("%Y%m%d")) * 100 + 1
    rel = {"format": 2, "version": version, "published": today.isoformat(),
           "check_by": (today + datetime.timedelta(days=14)).isoformat(), "rows": rows, "multipliers": mults,
           "disputes": disputes}
    if prev:
        P, N = index(prev["rows"]), index(rows)
        added = sorted("/".join(x for x in k if x) for k in N if k not in P)
        gone = sorted("/".join(x for x in k if x) for k in P if k not in N)
        changed = sorted("/".join(x for x in k if x) for k in N if k in P and diff(N[k], P[k]))
        rel["changes"] = {"added": added, "withdrawn": gone, "changed": changed}
        print(f"against version {prev['version']}: {len(added)} added, {len(gone)} withdrawn, {len(changed)} changed")
    json.dump(rel, open(out, "w"), indent=1)
    print(f"version {version}: {len(rows)} rows, {len(mults)} multipliers, {len(disputes)} disputes")
    for d in disputes: print("  DISPUTE", d["row"], d["fields"])
    if strict and disputes: sys.exit(1)


if __name__ == "__main__":
    main()
