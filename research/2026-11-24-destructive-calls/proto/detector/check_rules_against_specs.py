"""Checks every REST rule template in rules.json against the provider's published API description.

Usage: python3 check_rules_against_specs.py <dir with the spec files>
Spec files (fetched 2026-11-24, not committed):
  vercel.json   https://openapi.vercel.sh/
  neon.json     https://neon.com/api_spec/release/v2.json
  cf.json       https://raw.githubusercontent.com/cloudflare/api-schemas/main/openapi.json
  gh.json       https://raw.githubusercontent.com/github/rest-api-description/main/descriptions/api.github.com/api.github.com.json
  stripe.json   https://raw.githubusercontent.com/stripe/openapi/master/openapi/spec3.json
  supa.json     https://api.supabase.com/api/v1-json
  heroku.json   https://api.heroku.com/schema (Accept: application/vnd.heroku+json; version=3)
Fallback rules ({rest+}) are skipped: they match any path on purpose.
"""
import fnmatch
import json
import os
import re
import sys

SPECS = {"vercel": ("vercel.json", ""), "neon": ("neon.json", "/api/v2"), "cloudflare": ("cf.json", "/client/v4"),
         "github": ("gh.json", ""), "stripe": ("stripe.json", ""), "supabase": ("supa.json", ""), "heroku": ("heroku.json", "")}


def norm(p):
    return re.sub(r"\{[^}]*\}", "{}", p.rstrip("/"))


def spec_ops(path):
    d = json.load(open(path))
    ops = set()
    if "paths" in d:
        for p, v in d["paths"].items():
            for m in v:
                if m in ("get", "post", "put", "patch", "delete"):
                    ops.add((m.upper(), norm(p)))
    else:  # Heroku JSON hyper-schema
        for s in d["definitions"].values():
            for l in s.get("links", []):
                ops.add((l["method"].upper(), norm(re.sub(r"\{\([^)]*\)\}", "{}", l["href"]))))
    return ops


def main(specdir):
    rules = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "rules.json")))
    found, missing = 0, []
    for api in rules["apis"]:
        if api["id"] not in SPECS:
            continue
        fname, prefix = SPECS[api["id"]]
        ops = spec_ops(os.path.join(specdir, fname))
        for r in api["rules"]:
            m = r["match"].get("rest") or r["match"].get("sql")
            if not m or r.get("fallback"):
                continue
            paths = m["path"] if isinstance(m["path"], list) else [m["path"]]
            methods = m.get("method", ["POST"])
            methods = methods if isinstance(methods, list) else [methods]
            for p in paths:
                if p.startswith("/api/") and api["id"] == "vercel":
                    continue   # the vercel.com/api alias, not in the spec
                q = norm(p[len(prefix):] if p.startswith(prefix) else p)
                for meth in methods:
                    ok = any(meth == om and fnmatch.fnmatchcase(op, q.replace("{}", "[{]}")) for om, op in ops) \
                        if "*" in q else (meth, q) in ops
                    if ok:
                        found += 1
                    else:
                        missing.append("%s %s %s (%s)" % (api["id"], meth, p, r["id"]))
    print("found in the spec:", found)
    for x in missing:
        print("NOT IN SPEC:", x)


if __name__ == "__main__":
    main(sys.argv[1])
