"""#383's engine (#353's detector with two body conditions), plus what the big clouds need:

- AWS: a labelled catalogue for the services this folder labelled (aws_labelled.json). For those
  services every documented operation is known: the ones to hold carry their outcome, the others
  an "allow" match, so #353's verb heuristic ("an unknown operation whose name starts with Delete")
  no longer fires on them and the fallback is not asked. Other services keep #353's catalogue.
  Parameter rules gain a glob in wire_name, a header condition, and a regex on the body.
- SQL inside a JSON field (Cloud SQL executeSql, BigQuery jobs): sql shape "field:<dotted path>".
- Path templates may hold {x+} before the end (Azure's /{scope}/providers/...): it takes one or more segments.
- A body condition may be a list (all must hold) or {"regex": ...} on the body as sent, URL-decoded and
  JSON-unescaped.
- A REST rule with "no_verb" does not match a path whose last segment holds a Google custom
  method (":verb").
- Query-protocol AWS operations are told apart by Version when two services share an endpoint.
- A REST rule is skipped early when no reading of the path has a length its templates can match
  (speed only: about 5,000 rules here).

Nothing in #353's or #383's files changes.
"""
import fnmatch
import importlib.util
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
P383 = os.path.normpath(os.path.join(HERE, "..", "..", "..", "2026-11-25-destructive-calls-platforms", "proto", "detector"))
_spec = importlib.util.spec_from_file_location("engine383", os.path.join(P383, "engine.py"))
engine383 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(engine383)
base = engine383.base          # #353's detector module
import awsmatch  # noqa: E402  (on the path through #383's engine)

_sql_texts = base.sql_texts


def sql_texts(shape, ctx):
    if shape.startswith("field:"):
        j = ctx.json()
        if j is None:
            raise base.Unknown("the SQL request body is not JSON")
        v = base.get_path(j, shape[6:]) if isinstance(j, dict) else None
        return [(v, shape[6:])] if isinstance(v, str) else []
    return _sql_texts(shape, ctx)


base.sql_texts = sql_texts


def match_segs(segs, path):
    """#353's match_segs, with {x+} allowed anywhere: it takes one or more segments."""
    def go(n, i, vars_):
        if n == len(segs):
            return vars_ if i == len(path) else None
        kind, val = segs[n]
        if kind == "var+":
            for j in range(len(path), i, -1):
                r = go(n + 1, j, dict(vars_, **{val: "/".join(path[i:j])}))
                if r is not None:
                    return r
            return None
        if i >= len(path):
            return None
        if kind == "lit" and path[i] != val:
            return None
        if kind == "glob" and not fnmatch.fnmatchcase(path[i], val):
            return None
        if kind == "var":
            vars_ = dict(vars_, **{val: path[i]})
        return go(n + 1, i + 1, vars_)
    return go(0, 0, {})


base.match_segs = match_segs


def body_texts(ctx):
    from urllib.parse import unquote_plus
    raw = ctx.body()
    return [raw, unquote_plus(raw.decode("utf-8", "replace")).encode(), raw.replace(b'\\"', b'"')]


class AwsMatcher2(awsmatch.AwsMatcher):
    def __init__(self, conf, basedir):
        super().__init__(conf, basedir)
        lab = conf.get("labelled")
        self.known_services = set()
        if lab:
            with open(os.path.join(basedir, lab)) as f:
                cat = json.load(f)
            self.known_services = set(cat["services"])
            # drop #353's entries for the labelled services, add the labelled ones
            for key in list(self.by_service):
                self.by_service[key] = [o for o in self.by_service[key] if o["service"] not in self.known_services]
            for o in cat["operations"]:
                o = dict(o)
                for proto, m in o["match"].items():
                    if proto in ("rest-json", "rest-xml"):
                        o.setdefault("_tpl", {})[proto] = base.compile_template(m["path_template"].split("?")[0])
                        vh = m.get("virtual_hosted")
                        if vh:
                            o.setdefault("_vtpl", {})[proto] = base.compile_template(vh["path_template"].split("?")[0])
                for key in {o["service"], o.get("endpoint_prefix"), o.get("signing_name")} - {None}:
                    self.by_service.setdefault(key, []).append(o)
                    self.services.add(key)

    def check(self, ctx, matches, unknowns, facts, add):
        mine = []
        super().check(ctx, mine, unknowns, facts, lambda ms, m: ms.append(m))
        svc = set(facts.get("aws_services") or [])
        for m in mine:
            if m["rule"] == "aws.verb-not-in-catalogue" and svc & self.known_services:
                continue
            add(matches, m)
        # an operation with parameter rules is skipped by #353's catalogue pass; here it still
        # counts as known, with its own outcome when no parameter rule held it
        refined = {(p["service"], p["operation"].lower()) for p in self.params}
        named = self.op_names(ctx)
        seen = set()
        for s in svc:
            for o in self.by_service.get(s, []):
                k = (o["service"], o["operation"].lower())
                if k in refined and k not in seen and o["service"] in self.known_services and self._op_matches(o, ctx, named):
                    seen.add(k)
                    add(matches, self._match(o, ctx))

    def _op_matches(self, o, ctx, named):
        # Query APIs that share an endpoint (classic ELB 2012-06-01 and ELBv2 2015-12-01 both answer
        # Action=RemoveTags) are told apart by Version, which #353 did not read.
        for proto in ("query", "ec2"):
            want = (o["match"].get(proto) or {}).get("form", {}).get("Version")
            if want and len(o["match"]) == 1:
                got = list(ctx.qget("Version"))
                try:
                    got += [v for k, v in ctx.form() if k == "Version"]
                except base.Unknown:
                    pass
                if got and want not in got:
                    return False
        return super()._op_matches(o, ctx, named)

    def _match(self, o, ctx):
        m = super()._match(o, ctx)
        if o.get("action") == "allow":
            m["action"] = "allow"
            m["outcome"] = None
        return m

    def _param_hit(self, p, ctx):
        if "header" in p:
            vals = [v.strip().lower() for v in base.header(ctx.req, p["header"])]
            return any(v in [x.lower() for x in p["values"]] for v in vals)
        if "regex" in p:
            try:
                return any(re.search(p["regex"].encode(), t, re.I) for t in body_texts(ctx))
            except base.Unknown:
                return True
        if "*" in p.get("wire_name", ""):
            vals = dict(ctx.query)
            try:
                vals.update(dict(ctx.form()))
                j = ctx.json()
                if isinstance(j, dict):
                    vals.update(base.flatten_args(j))
            except base.Unknown:
                return True
            want = [str(x).lower() for x in p["values"]]
            return any(str(v).lower() in want for k, v in vals.items() if fnmatch.fnmatchcase(k, p["wire_name"]))
        return super()._param_hit(p, ctx)


class Detector(engine383.Detector):
    def __init__(self, rules, basedir="."):
        super().__init__(rules, basedir)
        for a in self.apis:
            if a.get("aws") and a["aws"].get("labelled"):
                a["_aws"] = AwsMatcher2(a["aws"], basedir)

    def _rest(self, api, rule, ctx, matches, facts):
        lens = rule.get("_lens")
        if lens is None:
            lens = set()
            for t in rule["_tpls"]:
                if any(k == "var+" for k, _ in t["segs"]):
                    lens = "any"
                    break
                lens.add(len(t["segs"]))
            rule["_lens"] = lens
        if lens != "any" and not any(len(c) in lens for c in ctx.paths):
            return
        # Google's custom methods: /things/{id}:verb is not the plain /things/{id}
        if rule["match"]["rest"].get("no_verb") and all(c and ":" in c[-1] for c in ctx.paths):
            return
        return super()._rest(api, rule, ctx, matches, facts)

    def _body_cond(self, cond, ctx, vars_):
        if isinstance(cond, list):
            return all(self._body_cond(c, ctx, vars_) for c in cond)
        if "regex" in cond:
            try:
                return any(re.search(cond["regex"].encode(), t, re.I) for t in body_texts(ctx))
            except base.Unknown:
                return True
        return super()._body_cond(cond, ctx, vars_)


def load(path):
    return Detector.load(path)
