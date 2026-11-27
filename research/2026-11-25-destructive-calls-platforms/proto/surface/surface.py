"""Turns every labelled operation (and each variant of an operation whose body decides) into one
request, so a rule set can be scored on the whole documented surface of an API.

Labels: labels/<provider>.txt (hand-written, see ../../details/surface.md). Variants: variants.json.
The request is the plainest well-formed call: path parameters filled with sample values, a JSON
body only where a variant gives one, a GraphQL mutation with the variant's arguments or none.
"""
import json, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
PROVIDERS = ["railway", "fly-gql", "fly", "vercel", "supabase", "cloudflare"]
SAMPLE = {"account_id": "acc123", "zone_id": "zone123", "ref": "abcdefghijklmnopqrst", "app": "my-app"}


def gql_lit(v):
    if isinstance(v, dict):
        return "{" + ", ".join("%s: %s" % (k, gql_lit(x)) for k, x in v.items()) + "}"
    if isinstance(v, list):
        return "[" + ", ".join(gql_lit(x) for x in v) + "]"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if v is None:
        return "null"
    if isinstance(v, (int, float)):
        return str(v)
    return json.dumps(v)


def read_labels(provider):
    out = {}
    for line in open(os.path.join(HERE, "labels", provider + ".txt")):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(" ")
        if parts[0] in ("GET", "POST", "PUT", "PATCH", "DELETE"):
            key, label, why = parts[0] + " " + parts[1], parts[2], " ".join(parts[3:])
        else:
            key, label, why = parts[0], parts[1], " ".join(parts[2:])
        out[key] = (label, why)
    return out


def fill(path, extra=None):
    extra = extra or {}
    def rep(m):
        n = m.group(1)
        return extra.get(n) or SAMPLE.get(n) or (n.replace("_", "")[:12] + "1")
    return re.sub(r"\{([^}]+)\}", rep, path)


def request(op, body=None, args=None, path_extra=None):
    headers = [["content-type", "application/json"], ["authorization", "Bearer <redacted>"]]
    if op["kind"] == "graphql":
        q = "mutation { %s%s }" % (op["name"], "(" + gql_lit(args)[1:-1] + ")" if args else "")
        return {"host": op["host"], "method": "POST", "target": op["path"], "headers": headers,
                "body": json.dumps({"query": q}).encode(), "facts": {}}
    return {"host": op["host"], "method": op["method"], "target": fill(op["path"], path_extra), "headers": headers,
            "body": json.dumps(body).encode() if body is not None else b"", "facts": {}}


def cases():
    variants = json.load(open(os.path.join(HERE, "variants.json")))
    out = []
    for prov in PROVIDERS:
        ops = json.load(open(os.path.join(HERE, "ops", prov + ".json")))
        labels = read_labels(prov)
        for op in ops:
            key = op["name"] if op["kind"] == "graphql" else op["method"] + " " + op["path"]
            label, why = labels[op["name"] if prov == "fly" else key]
            vkey = prov + " " + ("mutation " + op["name"] if op["kind"] == "graphql" else key)
            if label == "cond":
                for i, v in enumerate(variants[vkey]):
                    out.append({"id": "%s %s #%d" % (prov, key, i + 1), "provider": prov, "op": key,
                                "label": v["label"], "why": v["note"],
                                "req": request(op, v.get("body"), v.get("args"), v.get("path"))})
                continue
            out.append({"id": "%s %s" % (prov, key), "provider": prov, "op": key, "label": label, "why": why,
                        "req": request(op)})
    return out


if __name__ == "__main__":
    import collections
    cs = cases()
    c = collections.Counter((x["provider"], x["label"].split(":")[0]) for x in cs)
    for p in PROVIDERS:
        print(p, {k[1]: v for k, v in c.items() if k[0] == p})
    print(len(cs))
