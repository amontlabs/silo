"""AWS matcher: service from the SigV4 credential scope and the host, operation from X-Amz-Target,
Action= (query string or form body), the Smithy RPC v2 path, or REST method + path.

Loads ../aws_destructive.json (made from botocore models by another agent) when it exists, and the
hand list in rules.json otherwise. Hand rules for dangerous parameters on operations whose names
look harmless (ModifyDBInstance DeletionProtection=false, ...) are always loaded from rules.json.
"""
import json
import os
import re
from urllib.parse import unquote

import detector as D

VERBS = ("Delete", "Terminate", "Remove", "Purge", "Drop", "Destroy", "Disable", "Revoke",
         "Detach", "Deregister", "Cancel", "Reset", "Restore")

RESOURCE_KEYS = re.compile(r"(Name|Id|Ids|Identifier|Arn|Bucket|Key|member\.\d+)$")


class AwsMatcher:
    def __init__(self, conf, base):
        self.conf = conf
        self.by_service = {}
        self.source = "hand list in rules.json"
        path = os.path.join(base, conf.get("catalogue", ""))
        ops = []
        if conf.get("catalogue") and os.path.exists(path):
            with open(path) as f:
                cat = json.load(f)
            ops = cat["operations"]
            self.source = "%s (%d operations)" % (conf["catalogue"], len(ops))
        else:
            ops = conf.get("hand_ops", [])
        self.services = set()
        for o in ops:
            o = dict(o)
            for proto, m in o["match"].items():
                if proto in ("rest-json", "rest-xml"):
                    o.setdefault("_tpl", {})[proto] = D.compile_template(m["path_template"].split("?")[0])
                    vh = m.get("virtual_hosted")
                    if vh:
                        o.setdefault("_vtpl", {})[proto] = D.compile_template(vh["path_template"].split("?")[0])
            for key in {o["service"], o.get("endpoint_prefix"), o.get("signing_name")} - {None}:
                self.by_service.setdefault(key, []).append(o)
                self.services.add(key)
        self.params = conf.get("params", [])

    # ------------------------------------------------------------
    def services_for(self, ctx):
        out = set()
        for v in D.header(ctx.req, "authorization"):
            m = re.search(r"Credential=[^/,\s]+/\d{8}/(?:([^/,\s]+)/)?([^/,\s]+)/aws4_request", v)
            if m:
                out.add(m.group(2))
        for v in ctx.qget("X-Amz-Credential"):
            parts = v.split("/")
            if len(parts) >= 5:
                out.add(parts[-2])
        labels = ctx.host.split(".")
        for lab in labels:
            lab2 = lab.replace("-fips", "")
            if lab2 in self.services:
                out.add(lab2)
            if lab2.startswith("s3-") and lab2 not in ("s3-control", "s3-accesspoint"):
                out.add("s3")
        if "s3-control" in labels:
            out.add("s3control")
            out.discard("s3")
        return out

    def s3_virtual(self, ctx):
        h = ctx.host
        if ".s3-accesspoint." in h or ".s3-object-lambda." in h:
            return True
        m = re.match(r"^(.+?)\.s3(?:[.-][a-z0-9-]+)*\.amazonaws\.com(?:\.cn)?$", h)
        return bool(m) and m.group(1) not in ("s3",)

    def op_names(self, ctx):
        """(protocol, name) the request names explicitly."""
        out = []
        for v in D.header(ctx.req, "x-amz-target"):
            out.append(("json", v.strip()))
        actions = list(ctx.qget("Action"))
        if ctx.method == "POST":
            try:
                actions += [v for k, v in ctx.form() if k == "Action"]
            except D.Unknown:
                raise
        for a in actions:
            out.append(("query", a.strip()))
        for cand in ctx.paths:
            if len(cand) == 4 and cand[0] == "service" and cand[2] == "operation":
                # original case
                raw = D._clean_path_keep_case(unquote(ctx.raw_path))
                out.append(("cbor", raw[3] if len(raw) == 4 else cand[3]))
        return out

    def check(self, ctx, matches, unknowns, facts, add):
        services = self.services_for(ctx)
        facts["aws_services"] = sorted(services)
        named = self.op_names(ctx)
        if named:
            facts["aws_operation"] = [n for _, n in named]
        found = []
        refined = {(p["service"], p["operation"].lower()) for p in self.params}
        for svc in services:
            for o in self.by_service.get(svc, []):
                if (o["service"], o["operation"].lower()) in refined:
                    continue   # a hand rule on its parameters decides this operation
                if self._op_matches(o, ctx, named):
                    found.append(o)
        # several services share an endpoint prefix or signing name (rds, docdb, neptune):
        # for one operation name keep the service the host names, else the first
        labels = set(ctx.host.split("."))
        best = {}
        for o in found:
            k = o["operation"]
            if k not in best or (o["service"] in labels and best[k]["service"] not in labels):
                best[k] = o
        for o in best.values():
            add(matches, self._match(o, ctx))
        names = {n.split(".")[-1].lower() for _, n in named}
        known_ops = {o["operation"].lower() for o in found}
        for n in names:
            if n.lower() not in known_ops and any(n.lower().startswith(v.lower()) for v in VERBS):
                if not any(n.lower() == o["operation"].lower() for s in services for o in self.by_service.get(s, [])):
                    add(matches, {"api": "aws", "rule": "aws.verb-not-in-catalogue", "outcome": None,
                                  "action": "hold", "facts": {"operation": n},
                                  "say": "calls AWS %s %s, an operation Silo's list does not hold but whose name says it removes something" % ("/".join(sorted(services)) or "(service unknown)", n)})
        for p in self.params:
            if (p["service"] in services) and any(n.split(".")[-1].lower() == p["operation"].lower() for n in names | self._rest_names(ctx, services, p)):
                if self._param_hit(p, ctx):
                    add(matches, {"api": "aws", "rule": "aws.param." + p["operation"], "outcome": p.get("outcome"),
                                  "action": p.get("action", "hold"), "facts": {"operation": p["operation"]},
                                  "say": D.render(p["say"], self._resource(ctx))})
                    if p.get("action") == "unknown":
                        matches.pop()
                        unknowns.append(D.render(p["say"], self._resource(ctx)))

    def _rest_names(self, ctx, services, p):
        if p.get("rest"):
            path = p["rest"]["path"]
            if p["service"] == "s3" and self.s3_virtual(ctx):
                path = "/" + "/".join(path.strip("/").split("/")[1:])   # bucket is in the host
            tpl = D.compile_template(path)
            if p["rest"]["method"] in ctx.methods() and D.match_template(tpl, ctx) is not None:
                q = p["rest"].get("query_key")
                if not q or q in {k.lower() for k in ctx.qkeys()}:
                    return {p["operation"].lower()}
        return set()

    def _param_hit(self, p, ctx):
        if "contains" in p:
            try:
                return p["contains"].encode() in ctx.body()
            except D.Unknown:
                return True
        vals = dict(ctx.query)
        try:
            vals.update(dict(ctx.form()))
            j = ctx.json()
            if isinstance(j, dict):
                vals.update({k: v for k, v in D.flatten_args(j).items()})
        except D.Unknown:
            return True
        v = vals.get(p["wire_name"])
        if v is None:
            return "absent" in p["values"]
        return str(v).lower() in [str(x).lower() for x in p["values"]]

    def _op_matches(self, o, ctx, named):
        for proto, m in o["match"].items():
            if proto == "json":
                want = m["header"]["X-Amz-Target"].lower()
                if any(p == "json" and n.lower() == want for p, n in named):
                    return True
            elif proto in ("query", "ec2"):
                if any(p == "query" and n.lower() == o["operation"].lower() for p, n in named):
                    return True
            elif proto.startswith("smithy-rpc-v2-cbor"):
                if any(p == "cbor" and n.lower() == o["operation"].lower() for p, n in named):
                    want = m["path"].lower().strip("/").split("/")
                    if any(list(c) == want for c in ctx.paths):
                        return True
            elif proto in ("rest-json", "rest-xml"):
                if m["method"] not in ctx.methods():
                    continue
                virtual = o["service"] == "s3" and self.s3_virtual(ctx)
                tpl = (o.get("_vtpl") or {}).get(proto) if virtual else o["_tpl"][proto]
                if tpl is None:
                    continue
                if D.match_template(tpl, ctx) is None:
                    continue
                q = m.get("required_query_key")
                if q and q.lower() not in {k.lower() for k in ctx.qkeys()}:
                    continue
                if not q and o["service"] == "s3":
                    # S3 sub-resources (?tagging, ?acl ...) on the same path are other operations
                    sub = {k.lower() for k in ctx.qkeys()} - {"versionid", "x-id"} - {k.lower() for k in ctx.qkeys() if k.lower().startswith("x-amz-")}
                    if sub:
                        continue
                return True
        return False

    def _resource(self, ctx):
        vals = {}
        try:
            j = ctx.json()
            if isinstance(j, dict):
                vals.update({k: v for k, v in j.items() if isinstance(v, (str, int))})
            vals.update(dict(ctx.form()))
        except D.Unknown:
            pass
        vals.update(dict(ctx.query))
        res = [str(v) for k, v in vals.items() if RESOURCE_KEYS.search(k) and k not in ("Action", "Version")]
        vals["resource"] = ", ".join(res[:3]) if res else D.MISSING
        return vals

    def _match(self, o, ctx):
        vals = self._resource(ctx)
        proto = next(iter(o["match"]))
        if any(p in ("rest-json", "rest-xml") for p in o["match"]):
            path = D._clean_path_keep_case(unquote(ctx.raw_path))
            if o["service"] == "s3" and self.s3_virtual(ctx):
                bucket = ctx.host.split(".s3")[0]
                vals["resource"] = bucket + ("/" + "/".join(path) if path else "")
            elif path:
                vals["resource"] = "/".join(path)
        outcome = o.get("outcome")
        say = "calls AWS %s %s on %s" % (o["service"], o["operation"], vals["resource"])
        if outcome:
            say += " (%s)" % outcome
        return {"api": "aws", "rule": "aws.%s.%s" % (o["service"], o["operation"]), "outcome": outcome,
                "action": "hold", "say": say, "facts": {"service": o["service"], "operation": o["operation"],
                                                        "top": o.get("top", False), "note": o.get("note")}}
