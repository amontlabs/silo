#!/usr/bin/env python3
"""#353's capture server (../../../2026-11-24-destructive-calls/proto/capture/server.py), unchanged,
with one addition: GraphQL requests to Railway and Fly are answered by running the query against
the provider's real schema (introspected 2026-11-25) with made-up values, so a CLI reaches its
destructive step without a canned answer per query. Canned answers still win when one matches.

Made-up values: id "<type>-1", other strings from --mock (JSON {"Type.field": value}) or "x",
numbers 1, booleans false (true for a mutation's own result), enums their first value, lists one
item. Never logs a credential (the base server redacts them).
"""
import argparse, json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
BASE = os.path.normpath(os.path.join(HERE, "..", "..", "..", "2026-11-24-destructive-calls", "proto", "capture"))
sys.path.insert(0, BASE)
import server as base  # noqa: E402
from graphql import (build_client_schema, parse, execute, get_nullable_type, is_list_type, is_scalar_type,  # noqa: E402
                     is_enum_type, is_non_null_type)

SCHEMAS = {"backboard.railway.com": "railway_full.json", "backboard.railway.app": "railway_full.json",
           "api.fly.io": "fly_full.json"}


class Mock:
    def __init__(self, path, fixtures):
        self.schema = build_client_schema(json.load(open(path))["data"], assume_valid=True)
        self.fx = fixtures

    def value(self, t, parent, field, is_mutation_root):
        t = get_nullable_type(t)
        if is_list_type(t):
            return [self.value(t.of_type, parent, field, False)]
        if is_scalar_type(t):
            k = "%s.%s" % (parent, field)
            if k in self.fx:
                return self.fx[k]
            if field in self.fx:
                return self.fx[field]
            n = t.name
            if n in ("Boolean",):
                return bool(is_mutation_root)
            if n == "BigInt":
                return "1"
            if n in ("Int", "Float"):
                return 1
            if n in ("DateTime", "ISO8601DateTime", "Date"):
                return "2026-11-24T00:00:00Z"
            if n in ("JSON", "JSONObject", "EnvironmentConfig", "EnvironmentVariables", "DeploymentMeta", "CanvasConfig", "ServiceInstanceLimit", "SubscriptionPlanLimit"):
                return {}
            if field == "id" or n == "ID":
                return "%s-1" % parent.lower()
            return "x"
        if is_enum_type(t):
            v = list(t.values.values())[0]
            return v.value if v.value is not None else list(t.values)[0]
        return {"__parent": t.name}

    def resolver(self, root, info, **args):
        if isinstance(root, dict) and info.field_name in root and info.field_name != "__parent":
            return root[info.field_name]
        parent = info.parent_type.name
        k = "%s.%s" % (parent, info.field_name)
        if k in self.fx:
            return self.fx[k]
        t = get_nullable_type(info.return_type)
        if (not is_non_null_type(info.return_type) and is_scalar_type(t) and t.name in ("DateTime", "ISO8601DateTime")
                and info.field_name not in ("createdAt", "updatedAt")):
            return None   # deletedAt, archivedAt, expiresAt...: absent
        return self.value(info.return_type, parent, info.field_name, parent == (self.schema.mutation_type and self.schema.mutation_type.name))

    def type_resolver(self, value, info, abstract):
        if isinstance(value, dict) and value.get("__parent") and value["__parent"] != abstract.name:
            return value["__parent"]
        return self.schema.get_possible_types(abstract)[0].name

    def answer(self, text):
        try:
            d = json.loads(text)
        except Exception:
            return {"errors": [{"message": "bad json"}]}
        items = d if isinstance(d, list) else [d]
        outs = []
        for it in items:
            try:
                doc = parse(it.get("query") or "")
            except Exception as e:
                outs.append({"data": None, "errors": [{"message": str(e)}]})
                continue
            r = execute(self.schema, doc, root_value={}, variable_values=it.get("variables"),
                        operation_name=it.get("operationName"), field_resolver=self.resolver,
                        type_resolver=self.type_resolver)
            o = {"data": r.data}
            if r.errors:
                o["errors"] = [{"message": e.message} for e in r.errors]
            outs.append(o)
        return outs if isinstance(d, list) else outs[0]


class Server(base.Server):
    def __init__(self, args):
        super().__init__(args)
        fx = json.load(open(args.mock)) if args.mock else {}
        self.mocks = {}
        for h, f in SCHEMAS.items():
            p = os.path.join(args.specs, f)
            if os.path.exists(p):
                self.mocks[h] = Mock(p, fx)

    def reload(self):
        a = self.args
        if a.mock and os.path.exists(a.mock):
            m = os.path.getmtime(a.mock)
            if m != getattr(self, "_mtime", None):
                self._mtime = m
                fx = json.load(open(a.mock))
                for mk in self.mocks.values():
                    mk.fx = fx
        if a.canned and os.path.exists(a.canned):
            m = os.path.getmtime(a.canned)
            if m != getattr(self, "_cmtime", None):
                self._cmtime = m
                self.rules = base.Rules(a.canned)

    def canned(self, host, method, path, ops, text):
        self.reload()
        r = self.rules.find(host, method, path, ops)
        if r is None and ops and host in self.mocks:
            return 200, {}, json.dumps(self.mocks[host].answer(text)).encode()
        if r is None and self.args.default_body is not None:
            return 200, {}, self.args.default_body.encode()
        return super().canned(host, method, path, ops, text)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=443)
    ap.add_argument("--bind", default="127.0.0.1")
    ap.add_argument("--cert", default="/opt/clis/pki/chain.crt")
    ap.add_argument("--key", default="/opt/clis/pki/leaf.key")
    ap.add_argument("--log", required=True)
    ap.add_argument("--canned")
    ap.add_argument("--mock")
    ap.add_argument("--default-body")
    ap.add_argument("--specs", default="/tmp/claude-0/specs")
    ap.add_argument("--hold")
    ap.add_argument("--hold-secs", type=float, default=5)
    ap.add_argument("--hold-style", default="json403")
    ap.add_argument("--hold-status", type=int, default=403)
    ap.add_argument("--hold-ctype", default="application/json")
    ap.add_argument("--hold-body", default="")
    ap.add_argument("--alpn", default="http/1.1")
    ap.add_argument("--git-root", default="/opt/clis/git")
    Server(ap.parse_args()).serve()
