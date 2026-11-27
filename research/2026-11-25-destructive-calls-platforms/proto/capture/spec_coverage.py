"""How many write requests the CLIs sent are outside the provider's documented API.

operation-of-a-request holds a request whose operation the rule set does not list. If a set
lists exactly the documented operations, these are the requests it would hold as unknown.
Reads the same specs as ../surface/extract.py (SPECS, not committed).

Run: python3 capture/spec_coverage.py [SPECS]
"""
import json, os, re, sys, collections

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import labels  # noqa: E402

SPECS = sys.argv[1] if len(sys.argv) > 1 else "/tmp/claude-0/specs"
REST = {"vercel": "vercel.json", "supabase": "supabase.json", "cloudflare": "cloudflare.json"}


def pattern(path):
    return re.compile("^" + re.sub(r"\\\{[^/]+?\\\}", "[^/]+", re.escape(path)) + "$")


def documented():
    out = {}
    for prov, f in REST.items():
        spec = json.load(open(os.path.join(SPECS, f)))
        out[prov] = [(m.upper(), pattern(p)) for p, ops in spec["paths"].items() for m in ops
                     if m.upper() in ("POST", "PUT", "PATCH", "DELETE")]
    fly = json.load(open(os.path.join(HERE, "..", "surface", "ops", "fly.json")))
    out["fly"] = [(o["method"], pattern(o["path"])) for o in fly]
    return out


def gql_fields(body):
    try:
        q = json.loads(body)["query"]
    except (ValueError, TypeError, KeyError):
        return []
    m = re.search(r"mutation[^{]*\{\s*(\w+)", q)
    return [m.group(1)] if m else []


def main():
    docs = documented()
    known = {p: set(json.load(open(os.path.join(HERE, "..", "detector", "schemas", p + "_mutations.json")))["mutations"])
             for p in ("railway", "fly")}
    tally = collections.Counter()
    outside = []
    for c in labels.cases():
        q = c["req"]
        if q["method"] in ("GET", "HEAD") or (("graphql" in q["target"]) and b"mutation" not in q["body"]):
            continue
        if labels.is_aux({"graphql": [{"firstRootField": f} for f in gql_fields(q["body"])], "target": q["target"]}):
            continue  # Railway's telemetry, sent around every command
        prov = c["provider"]
        if "graphql" in q["target"]:
            ok = all(f in known[prov] for f in gql_fields(q["body"]))
        else:
            path = q["target"].split("?")[0]
            if prov == "cloudflare":
                path = path[len("/client/v4"):]
            ok = any(m == q["method"] and p.match(path) for m, p in docs.get(prov, []))
        lab = c["label"].split(":")[0]
        tally[(lab, "documented" if ok else "outside")] += 1
        if not ok:
            outside.append("%-8s %-6s %s %s %s" % (lab, prov, q["method"], q["host"], q["target"].split("?")[0]))
    for k, v in sorted(tally.items()):
        print(k, v)
    print("\n".join(sorted(set(outside))))


if __name__ == "__main__":
    main()
