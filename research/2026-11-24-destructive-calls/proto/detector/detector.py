"""Prototype of the detector Silo's boundary runs on a request it has already decrypted.

    d = Detector.load("rules.json")
    r = d.check({"host": ..., "method": ..., "target": "/path?query", "headers": [[k, v], ...],
                 "body": b"...", "facts": {...}})
    r["decision"]  -> "hold" | "allow"
    r["verdict"]   -> "destructive" | "unknown" | "harmless" | "not covered"
    r["says"]      -> sentences in Silo's words, one per thing a yes would let pass
    r["facts"]     -> what the detector read from the request to decide

"allow" means only that the detector does not hold the request. Silo's own rules still decide
whether the request may leave at all.

Facts the boundary passes in `facts` (not read from the request):
  body_complete: false when the boundary did not read the whole body (too large, still streaming)
  secret: {"name": ..., "prefix": "sk_live_"} when the boundary swapped a secret into this request
"""
import base64
import fnmatch
import gzip
import json
import re
import time
import zlib
from urllib.parse import parse_qsl, unquote, unquote_to_bytes

import brotli
import zstandard
from graphql import parse as gql_parse
from graphql.language import ast as gast
from graphql.utilities import value_from_ast_untyped

import awsmatch
import sqlclass

MAX_BODY = 8 * 1024 * 1024          # decoded bytes Silo reads; more is unknown
OVERRIDE_HEADERS = ("x-http-method-override", "x-http-method", "x-method-override")
MISSING = "(not in the request)"
# headers a sentence may quote; credentials never are
SAFE_HEADERS = {"stripe-account", "x-github-api-version", "content-type", "x-amz-target"}


class Unknown(Exception):
    pass


# ---------------------------------------------------------------- request normalisation

def header(req, name):
    name = name.lower()
    vals = [v for k, v in req["headers"] if k.lower() == name]
    return vals


def _one_host(h):
    h = (h or "").strip().lower()
    if h.startswith("["):
        h = h.split("]")[0] + "]"
    elif h.count(":") == 1:
        h = h.split(":")[0]
    return h.rstrip(".")


def all_hosts(req):
    """Every name the request goes by: the Host header, the host of an absolute-form target,
    the TLS SNI and the name the boundary resolved. A rule for any of them applies."""
    out = []
    m = re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*://([^/?#]*)", req.get("target", ""))
    cands = header(req, "host") + header(req, ":authority") + [m.group(1).split("@")[-1] if m else None,
             (req.get("facts") or {}).get("sni"), req.get("host")]
    for h in cands:
        h = _one_host(h)
        if h and h not in out:
            out.append(h)
    return out


def norm_host(req):
    hs = all_hosts(req)
    return hs[0] if hs else ""


def split_target(target):
    t = target
    m = re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*://[^/]*(.*)$", t)
    if m:
        t = m.group(1) or "/"
    t = t.split("#", 1)[0]
    if "?" in t:
        p, q = t.split("?", 1)
    else:
        p, q = t, ""
    return p or "/", q


def _clean_path(p):
    p = p.replace("\\", "/")
    segs = []
    for s in p.split("/"):
        s = s.split(";", 1)[0]           # matrix params: /v9/projects/x;y
        if s in ("", "."):
            continue
        if s == "..":
            if segs:
                segs.pop()
            continue
        segs.append(s.lower())
    return tuple(segs)


def path_candidates(raw):
    """Every reading of the path a server might take. Matching any of them counts."""
    out = []
    variants = [raw]
    once = unquote(raw, errors="replace")
    variants.append(once)
    full = once
    for _ in range(3):
        nxt = unquote(full, errors="replace")
        if nxt == full:
            break
        full = nxt
    variants.append(full)
    for v in variants:
        c = _clean_path(v)
        if c not in out:
            out.append(c)
    return out


def dechunk(data):
    out = bytearray()
    i = 0
    while True:
        j = data.find(b"\r\n", i)
        if j < 0:
            raise Unknown("the chunked body is cut short")
        size = int(data[i:j].split(b";")[0].strip() or b"0", 16)
        i = j + 2
        if size == 0:
            return bytes(out)
        out += data[i:i + size]
        if len(data) < i + size:
            raise Unknown("the chunked body is cut short")
        i += size + 2


def _bounded(chunks_iter):
    out = bytearray()
    for c in chunks_iter:
        out += c
        if len(out) > MAX_BODY:
            raise Unknown("the decompressed body is larger than Silo reads (%d bytes)" % MAX_BODY)
    return bytes(out)


def decode_one(enc, data):
    if enc in ("identity", ""):
        return data
    if enc in ("gzip", "x-gzip"):
        d = zlib.decompressobj(16 + zlib.MAX_WBITS)
        out = d.decompress(data, MAX_BODY + 1)
        if len(out) > MAX_BODY:
            raise Unknown("the decompressed body is larger than Silo reads")
        return out
    if enc == "deflate":
        for wbits in (zlib.MAX_WBITS, -zlib.MAX_WBITS):
            try:
                d = zlib.decompressobj(wbits)
                out = d.decompress(data, MAX_BODY + 1)
                if len(out) > MAX_BODY:
                    raise Unknown("the decompressed body is larger than Silo reads")
                return out
            except zlib.error:
                continue
        raise Unknown("the deflate body does not decode")
    if enc == "br":
        dec = brotli.Decompressor()
        out = dec.process(data)
        if len(out) > MAX_BODY:
            raise Unknown("the decompressed body is larger than Silo reads")
        return out
    if enc == "zstd":
        r = zstandard.ZstdDecompressor().decompressobj()
        out = r.decompress(data)
        if len(out) > MAX_BODY:
            raise Unknown("the decompressed body is larger than Silo reads")
        return out
    raise Unknown("the body uses a Content-Encoding Silo does not read (%s)" % enc)


class Ctx:
    """One request, read lazily: each part is decoded once, on first use."""

    def __init__(self, req):
        self.req = req
        self.hosts = all_hosts(req)
        self.host = self.hosts[0] if self.hosts else ""
        self.method = req["method"].upper()
        raw_path, self.raw_query = split_target(req["target"])
        self.raw_path = raw_path
        self.paths = path_candidates(raw_path)
        self.query = parse_qsl(self.raw_query, keep_blank_values=True)
        self.facts = dict(req.get("facts") or {})
        self._body = None
        self._json = None
        self._form = None
        self._methods = None

    def qget(self, key):
        return [v for k, v in self.query if k == key]

    def qkeys(self):
        return {k for k, _ in self.query}

    def ctype(self):
        return ((header(self.req, "content-type") or [""])[0]).lower()

    def body(self):
        if self._body is None:
            if self.facts.get("body_complete") is False:
                raise Unknown("Silo did not read the whole body (it is larger than Silo reads)")
            data = self.req.get("body") or b""
            te = ",".join(header(self.req, "transfer-encoding")).lower()
            if "chunked" in te and self.req.get("body_is_raw_chunked"):
                data = dechunk(data)
            encs = [e.strip().lower() for h in header(self.req, "content-encoding")
                    for e in h.split(",") if e.strip()]
            for enc in reversed(encs):
                try:
                    data = decode_one(enc, data)
                except Unknown:
                    raise
                except Exception as e:
                    raise Unknown("the %s body does not decode (%s)" % (enc, type(e).__name__))
            if len(data) > MAX_BODY:
                raise Unknown("the body is larger than Silo reads")
            self._body = data
        return self._body

    def json(self):
        """Parsed JSON body, or None if the body is not JSON. Duplicate keys are unknown."""
        if self._json is None:
            b = self.body()
            if b.startswith(b"\xef\xbb\xbf"):
                b = b[3:]
            s = b.strip()
            if not s or s[:1] not in (b"{", b"["):
                self._json = ("none",)
                return None
            dup = []

            def hook(pairs):
                seen = set()
                for k, _ in pairs:
                    if k in seen:
                        dup.append(k)
                    seen.add(k)
                return dict(pairs)
            try:
                v = json.loads(s.decode("utf-8"), object_pairs_hook=hook)
            except Exception:
                self._json = ("bad",)
                return None
            if dup:
                raise Unknown("the JSON body names the key %r twice; servers differ on which one counts" % dup[0])
            self._json = ("ok", v)
        return self._json[1] if self._json[0] == "ok" else None

    def json_is_bad(self):
        self.json()
        return self._json[0] == "bad"

    def form(self):
        if self._form is None:
            ct = self.ctype()
            if "x-www-form-urlencoded" in ct:
                try:
                    self._form = parse_qsl(self.body().decode("utf-8", "replace"), keep_blank_values=True)
                except Unknown:
                    raise
            else:
                self._form = []
        return self._form

    def methods(self):
        """The method and every method an override could turn it into (assumed honoured)."""
        if self._methods is None:
            ms = {self.method}
            for h in OVERRIDE_HEADERS:
                for v in header(self.req, h):
                    ms.add(v.strip().upper())
            for v in self.qget("_method"):
                ms.add(v.strip().upper())
            if self.method == "POST":
                try:
                    for k, v in self.form():
                        if k == "_method":
                            ms.add(v.strip().upper())
                    j = self.json() if "json" in self.ctype() else None
                    if isinstance(j, dict) and isinstance(j.get("_method"), str):
                        ms.add(j["_method"].upper())
                except Unknown:
                    pass
            self._methods = ms
        return self._methods

    def overridden(self):
        return self.methods() - {self.method}


# ---------------------------------------------------------------- templates

def compile_template(t):
    path, _, q = t.partition("?")
    segs = []
    for s in path.strip("/").split("/"):
        if not s:
            continue
        m = re.fullmatch(r"\{([A-Za-z0-9_]+)(\+?)\}", s)
        if m:
            segs.append(("var+", m.group(1)) if m.group(2) else ("var", m.group(1)))
        elif "*" in s:
            segs.append(("glob", s.lower()))
        else:
            segs.append(("lit", s.lower()))
    return {"segs": segs, "query_key": q or None}


def match_segs(segs, path):
    vars_ = {}
    i = 0
    for n, (kind, val) in enumerate(segs):
        if kind == "var+":
            if i >= len(path):
                return None
            vars_[val] = "/".join(path[i:])
            return vars_ if n == len(segs) - 1 else None
        if i >= len(path):
            return None
        if kind == "lit" and path[i] != val:
            return None
        if kind == "glob" and not fnmatch.fnmatchcase(path[i], val):
            return None
        if kind == "var":
            vars_[val] = path[i]
        i += 1
    return vars_ if i == len(path) else None


def match_template(tpl, ctx, original_case=True):
    for cand in ctx.paths:
        v = match_segs(tpl["segs"], cand)
        if v is not None:
            if tpl["query_key"] and tpl["query_key"] not in {k.lower() for k in ctx.qkeys()}:
                continue
            if original_case:   # give variables back in the case they were sent
                orig = _clean_path_keep_case(unquote(ctx.raw_path, errors="replace"))
                if len(orig) == len(cand):
                    lower_to_orig = dict(zip(cand, orig))
                    v = {k: lower_to_orig.get(x, x) if "/" not in x else x for k, x in v.items()}
            return v
    return None


def _clean_path_keep_case(p):
    out = []
    for s in p.replace("\\", "/").split("/"):
        s = s.split(";", 1)[0]
        if s in ("", "."):
            continue
        if s == "..":
            if out:
                out.pop()
            continue
        out.append(s)
    return tuple(out)


def host_matches(patterns, host):
    return any(fnmatch.fnmatchcase(host, p) for p in patterns)


def get_path(obj, dotted):
    cur = obj
    for part in dotted.split("."):
        if isinstance(cur, dict) and part in cur:
            cur = cur[part]
        elif isinstance(cur, list) and part.isdigit() and int(part) < len(cur):
            cur = cur[int(part)]
        else:
            return None
    return cur


def render(template, values):
    def rep(m):
        v = None
        for key in m.group(1).split("|"):
            v = values.get(key)
            if v not in (None, ""):
                break
        if v is None or v == "":
            return MISSING
        s = v if isinstance(v, str) else json.dumps(v)
        return s if len(s) <= 96 else s[:93] + "..."
    return re.sub(r"\{([A-Za-z0-9_.|-]+)\}", rep, template)


def template_keys(template):
    return {k for g in re.findall(r"\{([A-Za-z0-9_.|-]+)\}", template) for k in g.split("|")}


# ---------------------------------------------------------------- GraphQL

def flatten_args(d, prefix=""):
    out = {}
    if isinstance(d, dict):
        for k, v in d.items():
            key = prefix + k
            out[key] = v
            if isinstance(v, (dict, list)):
                out.update(flatten_args(v, key + "."))
    elif isinstance(d, list):
        for i, v in enumerate(d):
            key = prefix + str(i)
            out[key] = v
            if isinstance(v, (dict, list)):
                out.update(flatten_args(v, key + "."))
    return out


def graphql_payloads(ctx):
    """Every (query, operationName, variables, extensions) the server might execute."""
    payloads = []
    # 1. the URL (GET, and some servers read it on POST too)
    q = ctx.qget("query")
    if q or ctx.qget("extensions"):
        payloads.append({"query": q[-1] if q else None,
                         "operationName": (ctx.qget("operationName") or [None])[-1],
                         "variables": (ctx.qget("variables") or [None])[-1],
                         "extensions": (ctx.qget("extensions") or [None])[-1]})
    if ctx.method in ("GET", "HEAD") and not ctx.req.get("body"):
        return payloads
    body = ctx.body()
    if not body.strip():
        return payloads
    j = ctx.json()
    if j is not None:
        items = j if isinstance(j, list) else [j]
        for it in items:
            if not isinstance(it, dict):
                raise Unknown("a GraphQL batch entry is not an object")
            payloads.append(it)
        return payloads
    form = ctx.form()
    if form:
        f = dict(form)
        if "query" in f or "extensions" in f:
            payloads.append({k: f.get(k) for k in ("query", "operationName", "variables", "extensions")})
            return payloads
    ct = ctx.ctype()
    if "multipart/form-data" in ct:
        m = re.search(rb'name="operations"\r\n(?:[^\r\n]+\r\n)*?\r\n(.*?)\r\n--', body, re.S)
        if not m:
            raise Unknown("a multipart GraphQL body without an operations part")
        try:
            ops = json.loads(m.group(1))
        except Exception:
            raise Unknown("the multipart operations part is not JSON")
        for it in (ops if isinstance(ops, list) else [ops]):
            payloads.append(it)
        return payloads
    # 2. a raw document (application/graphql, or a mislabelled body)
    try:
        text = body.decode("utf-8")
    except UnicodeDecodeError:
        raise Unknown("the GraphQL body is not text")
    if ctx.json_is_bad():
        raise Unknown("the body looks like JSON but does not parse")
    payloads.append({"query": text, "operationName": ctx.qget("operationName")[-1] if ctx.qget("operationName") else None})
    return payloads


def _vars(v):
    if v is None:
        return {}
    if isinstance(v, str):
        try:
            v = json.loads(v)
        except Exception:
            return {}
    return v if isinstance(v, dict) else {}


def graphql_root_fields(payload, persisted):
    """Root fields of the operation(s) the server would execute: list of dicts."""
    query = payload.get("query")
    ext = payload.get("extensions")
    if isinstance(ext, str):
        try:
            ext = json.loads(ext)
        except Exception:
            ext = None
    if not isinstance(query, str) or not query.strip():
        h = None
        if isinstance(ext, dict) and isinstance(ext.get("persistedQuery"), dict):
            h = ext["persistedQuery"].get("sha256Hash")
        for k in ("id", "documentId", "doc_id", "queryId"):
            if payload.get(k):
                h = payload.get(k)
        if h is not None:
            known = persisted.get(h)
            if known is None:
                raise Unknown("a persisted query: only the hash %s was sent, so Silo cannot see which fields it runs" % str(h)[:16])
            query = known
        else:
            raise Unknown("a GraphQL request without a query")
    try:
        doc = gql_parse(query, no_location=True, max_tokens=50000)
    except Exception as e:
        raise Unknown("the GraphQL text does not parse (%s)" % str(e).split("\n")[0][:80])
    ops = [d for d in doc.definitions if isinstance(d, gast.OperationDefinitionNode)]
    frags = {d.name.value: d for d in doc.definitions if isinstance(d, gast.FragmentDefinitionNode)}
    name = payload.get("operationName")
    if name:
        chosen = [o for o in ops if o.name and o.name.value == name]
        if not chosen:
            raise Unknown("operationName %r names no operation in the document" % name)
    elif len(ops) == 1:
        chosen = ops
    else:
        chosen = ops   # the server should refuse; Silo looks at all of them
    variables = _vars(payload.get("variables"))
    fields = []
    for op in chosen:
        optype = op.operation.value
        seen = set()

        def walk(sel_set, depth=0):
            if depth > 50:
                raise Unknown("fragments nest too deep")
            for sel in sel_set.selections:
                if isinstance(sel, gast.FieldNode):
                    args = {}
                    for a in sel.arguments or []:
                        try:
                            v = value_from_ast_untyped(a.value, variables)
                        except Exception:
                            v = None
                        if isinstance(v, str) and v.lstrip()[:1] in ("{", "["):
                            try:   # a JSON scalar sent as a string (Railway's patch)
                                v = json.loads(v)
                            except Exception:
                                pass
                        args[a.name.value] = v
                    fields.append({"type": optype, "field": sel.name.value,
                                   "alias": sel.alias.value if sel.alias else None,
                                   "args": args, "operationName": op.name.value if op.name else None})
                elif isinstance(sel, gast.InlineFragmentNode):
                    walk(sel.selection_set, depth + 1)
                elif isinstance(sel, gast.FragmentSpreadNode):
                    fn = sel.name.value
                    if fn in seen:
                        continue
                    seen.add(fn)
                    if fn not in frags:
                        raise Unknown("the document spreads a fragment it does not define (%s)" % fn)
                    walk(frags[fn].selection_set, depth + 1)
        walk(op.selection_set)
    return fields


# ---------------------------------------------------------------- SQL over HTTP

def sql_texts(shape, ctx):
    """[(sql, where)] for one SQL-over-HTTP body shape."""
    j = ctx.json()
    if j is None:
        raise Unknown("the SQL request body is not JSON")
    out = []
    if shape == "supabase":                     # {"query": "...", "read_only": bool}
        out.append((j.get("query") if isinstance(j, dict) else None, "query"))
    elif shape == "neon":                       # {"query","params"} or {"queries":[...]}
        if isinstance(j, dict) and isinstance(j.get("queries"), list):
            for i, q in enumerate(j["queries"]):
                out.append((q.get("query") if isinstance(q, dict) else None, "queries.%d" % i))
        else:
            out.append((j.get("query") if isinstance(j, dict) else None, "query"))
    elif shape == "d1":                         # {"sql","params"} or {"batch":[{"sql"}]}
        if isinstance(j, dict) and isinstance(j.get("batch"), list):
            for i, q in enumerate(j["batch"]):
                out.append((q.get("sql") if isinstance(q, dict) else None, "batch.%d" % i))
        else:
            out.append((j.get("sql") if isinstance(j, dict) else None, "sql"))
    elif shape == "hrana":                      # Hrana over HTTP pipeline
        reqs = j.get("requests") if isinstance(j, dict) else None
        if not isinstance(reqs, list):
            raise Unknown("a Turso pipeline without requests")
        for i, r in enumerate(reqs):
            t = r.get("type") if isinstance(r, dict) else None
            stmts = []
            if t == "execute":
                stmts = [r.get("stmt")]
            elif t == "batch":
                stmts = [s.get("stmt") for s in (r.get("batch") or {}).get("steps", []) if isinstance(s, dict)]
            elif t == "sequence":
                stmts = [{"sql": r.get("sql"), "sql_id": r.get("sql_id")}]
            elif t == "store_sql":
                stmts = [{"sql": r.get("sql")}]
            elif t in ("close", "close_sql", "get_autocommit", "describe"):
                continue
            else:
                raise Unknown("a Turso pipeline request of a type Silo does not know (%s)" % t)
            for s in stmts:
                if not isinstance(s, dict):
                    raise Unknown("a Turso statement that is not an object")
                if s.get("sql") is None and s.get("sql_id") is not None:
                    raise Unknown("a Turso statement runs SQL stored by an earlier request (sql_id %s)" % s.get("sql_id"))
                out.append((s.get("sql"), "requests.%d" % i))
    else:
        raise Unknown("unknown SQL body shape %s" % shape)
    return out


# ---------------------------------------------------------------- git smart HTTP

def receive_pack_commands(body):
    cmds = []
    i = 0
    while i + 4 <= len(body):
        try:
            n = int(body[i:i + 4], 16)
        except ValueError:
            raise Unknown("the git push body does not parse")
        if n == 0:
            break
        line = body[i + 4:i + n]
        i += n
        line = line.split(b"\0", 1)[0].strip()
        parts = line.split(b" ")
        if len(parts) >= 3 and len(parts[0]) in (40, 64):
            cmds.append((parts[0].decode(), parts[1].decode(), parts[2].decode("utf-8", "replace")))
        elif line.startswith(b"shallow") or line.startswith(b"push-cert"):
            continue
    return cmds


# ---------------------------------------------------------------- the detector

class Detector:
    def __init__(self, rules, base="."):
        self.rules = rules
        self.apis = []
        for api in rules["apis"]:
            a = dict(api)
            a["compiled"] = []
            for r in api.get("rules", []):
                r = dict(r)
                m = r["match"]
                if "rest" in m:
                    paths = m["rest"]["path"] if isinstance(m["rest"]["path"], list) else [m["rest"]["path"]]
                    r["_tpls"] = [compile_template(x) for x in paths]
                g = m.get("graphql")
                if g and g.get("field_from"):
                    with open(base + "/" + g["field_from"]["file"]) as f:
                        src = json.load(f)
                    kinds = set(g["field_from"].get("kinds", []))
                    g = dict(g)
                    g["field_in"] = sorted({x["mutation"] for x in src["mutations"] if not kinds or x["kind"] in kinds})
                    r["match"] = dict(m, graphql=g)
                if "sql" in m:
                    paths = m["sql"]["path"] if isinstance(m["sql"]["path"], list) else [m["sql"]["path"]]
                    r["_tpls"] = [compile_template(x) for x in paths]
                if "git" in m:
                    r["_tpl"] = compile_template(m["git"]["path"])
                a["compiled"].append(r)
            gq = api.get("graphql")
            if gq:
                gq = dict(gq)
                gq["_tpls"] = [compile_template(p) for p in gq["paths"]]
                if gq.get("known_mutations"):
                    with open(base + "/" + gq["known_mutations"]) as f:
                        gq["_known"] = set(json.load(f)["mutations"])
                a["graphql"] = gq
            if api.get("aws"):
                a["_aws"] = awsmatch.AwsMatcher(api["aws"], base)
            self.apis.append(a)

    @classmethod
    def load(cls, path):
        import os
        with open(path) as f:
            rules = json.load(f)
        return cls(rules, os.path.dirname(os.path.abspath(path)))

    # ------------------------------------------------------------
    def check(self, req):
        ctx = Ctx(req)
        apis = []
        for h in ctx.hosts:
            for a in self.apis:
                if a not in apis and host_matches(a["hosts"], h):
                    apis.append(a)
                    if not a.get("mcp_sniff"):
                        ctx.host = h
        result = {"decision": "allow", "verdict": "not covered", "says": [], "matches": [],
                  "unknown": [], "facts": {"host": ctx.host, "method": ctx.method}}
        if not apis:
            return result
        facts = result["facts"]
        facts["apis"] = [a["id"] for a in apis]
        if ctx.overridden():
            facts["method_override"] = sorted(ctx.overridden())
        self._key_mode(ctx, facts)
        unknown_policy = "allow"
        for api in apis:
            matches, unknowns = [], []
            try:
                self._check_api(api, ctx, matches, unknowns, facts)
            except Unknown as e:
                unknowns.append(str(e))
            if unknowns:
                pol = api.get("unknown", "hold")
                if pol == "hold":
                    unknown_policy = "hold"
                result["unknown"] += ["%s: %s" % (api["name"], u) for u in unknowns]
            for mm in [x for x in matches if x["action"] == "unknown"]:
                result["unknown"].append("%s: %s" % (api["name"], mm["say"]))
                if api.get("unknown", "hold") == "hold":
                    unknown_policy = "hold"
            result["matches"] += matches
        if any(not a.get("mcp_sniff") for a in apis if any(m["api"] == a["id"] for m in result["matches"])):
            result["matches"] = [m for m in result["matches"] if m["api"] != "mcp-any"]
        holds = [m for m in result["matches"] if m["action"] == "hold"]
        if holds:
            result["verdict"] = "destructive"
            result["decision"] = "hold"
            result["says"] = [m["say"] for m in holds]
        elif result["unknown"]:
            result["verdict"] = "unknown"
            result["decision"] = unknown_policy
            if unknown_policy == "hold":
                result["says"] = ["sends a request to %s that Silo cannot read: %s" % (apis[0]["name"], u.split(": ", 1)[1]) for u in result["unknown"]]
        else:
            result["verdict"] = "harmless"
            if all(a.get("mcp_sniff") for a in apis):
                result["verdict"] = "not covered"   # only the any-host MCP sniffer looked
        return result

    def _key_mode(self, ctx, facts):
        prefix = None
        sec = ctx.facts.get("secret")
        if isinstance(sec, dict) and sec.get("prefix"):
            prefix = sec["prefix"]
            facts["secret"] = sec.get("name")
        else:
            # a key Silo does not hold, sent by the computer itself: read only its prefix
            for v in header(ctx.req, "authorization"):
                tok = v.split(" ", 1)[-1].strip()
                if v.lower().startswith("basic "):
                    try:
                        tok = base64.b64decode(tok).decode("latin-1").split(":")[0]
                    except Exception:
                        tok = ""
                m = re.match(r"^((?:sk|rk|pk)_(?:live|test)_)", tok)
                if m:
                    prefix = m.group(1)
                    facts["key_not_held_by_silo"] = True
        if prefix:
            facts["key_prefix"] = prefix
            facts["key_mode"] = "live" if "_live_" in prefix else "test"

    def _when_ok(self, rule, facts):
        w = rule.get("when")
        if not w:
            return True
        for k, allowed in w.items():
            v = facts.get(k)
            if v is None:
                v = "absent"
            if v not in allowed:
                return False
        return True

    def _add(self, api, rule, values, matches, facts):
        vals = dict(values)
        vals.setdefault("host", facts["host"])
        say = render(rule["say"], vals)
        action = rule.get("action", "hold")
        # facts keep only what the sentence names, plus path variables: never headers or bodies whole
        keep = template_keys(rule["say"]) | set(values.get("_path_vars", ()))
        matches.append({"api": api["id"], "rule": rule["id"], "outcome": rule.get("outcome"),
                        "action": action, "say": say,
                        "facts": {k: values[k] for k in sorted(keep) if k in values and not k.startswith("header.")}})

    def _check_api(self, api, ctx, matches, unknowns, facts):
        specific = []
        fallbacks = []
        for rule in api["compiled"]:
            (fallbacks if rule.get("fallback") else specific).append(rule)
        for group in (specific, fallbacks):
            if group is fallbacks and any(m["api"] == api["id"] for m in matches):
                break
            for rule in group:
                if not self._when_ok(rule, facts):
                    continue
                m = rule["match"]
                try:
                    if "rest" in m:
                        self._rest(api, rule, ctx, matches, facts)
                    elif "sql" in m:
                        self._sql(api, rule, ctx, matches, unknowns, facts)
                    elif "mcp" in m:
                        self._mcp(api, rule, ctx, matches, unknowns, facts)
                    elif "git" in m:
                        self._git(api, rule, ctx, matches, facts)
                except Unknown as e:
                    unknowns.append(str(e))
        if api.get("graphql"):
            try:
                self._graphql(api, ctx, matches, unknowns, facts)
            except Unknown as e:
                unknowns.append(str(e))
        if api.get("_aws"):
            try:
                api["_aws"].check(ctx, matches, unknowns, facts, self._add_aws)
            except Unknown as e:
                unknowns.append(str(e))

    def _add_aws(self, matches, m):
        matches.append(m)

    # REST ---------------------------------------------------------
    def _rest(self, api, rule, ctx, matches, facts):
        m = rule["match"]["rest"]
        methods = m["method"] if isinstance(m["method"], list) else [m["method"]]
        if not (set(methods) & ctx.methods()) and "*" not in methods:
            return
        vars_ = None
        for tpl in rule["_tpls"]:
            vars_ = match_template(tpl, ctx)
            if vars_ is not None:
                break
        if vars_ is None:
            return
        for k, want in (m.get("query") or {}).items():
            if not any(v.lower() in [str(x).lower() for x in want] for v in ctx.qget(k)):
                return
        if m.get("no_filter"):
            ignore = set(m["no_filter"])
            if any(k not in ignore for k in ctx.qkeys()):
                return
        if m.get("body"):
            body_ok = self._body_cond(m["body"], ctx, vars_)
            if not body_ok:
                return
        vars_["_path_vars"] = list(vars_)
        for k, v in ctx.query:
            vars_.setdefault("query." + k, v)
        for k, v in ctx.req["headers"]:
            if k.lower() in SAFE_HEADERS:
                vars_.setdefault("header." + k.lower(), v)
        try:
            j = ctx.json() if ctx.req.get("body") else None
            if isinstance(j, dict):
                for k, v in flatten_args(j).items():
                    if not isinstance(v, (dict, list)):
                        vars_.setdefault("body." + k, v)
                    elif isinstance(v, list):
                        vars_.setdefault("count." + k, str(len(v)))
            for k, v in ctx.form():
                vars_.setdefault("body." + k, v)
        except Unknown:
            pass
        vars_["method"] = ctx.method if ctx.method in methods else "/".join(sorted(set(methods) & ctx.methods()))
        vars_["path"] = ctx.raw_path
        if ctx.overridden():
            vars_["method_override"] = ",".join(sorted(ctx.overridden()))
        self._add(api, rule, vars_, matches, facts)

    def _body_cond(self, cond, ctx, vars_):
        """cond: {"field": dotted, "in": [values]} or {"contains": text}. All must hold."""
        if "contains" in cond:
            try:
                return cond["contains"].encode() in ctx.body()
            except Unknown:
                return True     # cannot read: count the condition as met
        fields = {}
        j = ctx.json()
        if isinstance(j, dict):
            fields = flatten_args(j)
        else:
            fields = dict(ctx.form())
        keys = [k for k in fields if fnmatch.fnmatchcase(k, cond["field"])] if "*" in cond["field"] else [cond["field"]]
        want = [str(x).lower() for x in cond["in"]]
        hit = False
        for key in keys:
            v = fields.get(key)
            if v is None:
                continue
            if str(v).lower() in want or (isinstance(v, bool) and str(v).lower() in want):
                vars_[cond["field"]] = v
                hit = True
        if not keys or all(fields.get(k) is None for k in keys):
            return "absent" in cond["in"]
        return hit

    # GraphQL ------------------------------------------------------
    def _graphql(self, api, ctx, matches, unknowns, facts):
        gq = api["graphql"]
        if not any(match_template(t, ctx, False) is not None for t in gq["_tpls"]):
            return
        payloads = graphql_payloads(ctx)
        if not payloads:
            return
        facts.setdefault("graphql", [])
        for p in payloads:
            try:
                fields = graphql_root_fields(p, gq.get("persisted", {}))
            except Unknown as e:
                unknowns.append(str(e))
                continue
            for f in fields:
                facts["graphql"].append("%s %s%s" % (f["type"], (f["alias"] + ": ") if f["alias"] else "", f["field"]))
                hit = False
                for rule in api["compiled"]:
                    g = rule["match"].get("graphql")
                    if not g or not self._when_ok(rule, facts):
                        continue
                    if g.get("type", "mutation") not in (f["type"], "any"):
                        continue
                    if "field" in g and f["field"] != g["field"]:
                        continue
                    if "field_in" in g and f["field"] not in g["field_in"]:
                        continue
                    if "field_glob" in g and not any(fnmatch.fnmatchcase(f["field"], x) for x in g["field_glob"]):
                        continue
                    if "field_glob_exclude" in g and any(fnmatch.fnmatchcase(f["field"], x) for x in g["field_glob_exclude"]):
                        continue
                    flat = flatten_args(f["args"])
                    matched_keys = []
                    if "arg" in g:
                        ok = True
                        for k, v in g["arg"].items():
                            keys = [x for x in flat if fnmatch.fnmatchcase(x, k)] if "*" in k else [k]
                            hits = [x for x in keys if str(flat.get(x)).lower() in [str(y).lower() for y in v]]
                            if not hits:
                                ok = False
                            matched_keys += hits
                        if not ok:
                            continue
                    if rule.get("fallback") and hit:
                        continue
                    vals = {k: v for k, v in flat.items() if not isinstance(v, (dict, list))}
                    vals["field"] = f["field"]
                    vals["alias"] = f["alias"]
                    vals["_path_vars"] = ["field", "alias", "matched"]
                    if matched_keys:
                        vals["matched"] = ", ".join(sorted({".".join(x.split(".")[1:-1]) for x in matched_keys}))
                    self._add(api, rule, vals, matches, facts)
                    hit = True
                if (not hit and f["type"] == "mutation" and gq.get("_known") is not None
                        and f["field"] not in gq["_known"] and f["field"] != "__typename"):
                    unknowns.append("the mutation field %s is not in Silo's copy of %s's schema" % (f["field"], api["name"]))

    # SQL ----------------------------------------------------------
    def _sql(self, api, rule, ctx, matches, unknowns, facts):
        m = rule["match"]["sql"]
        if ctx.method not in m.get("method", ["POST"]) and not (set(m.get("method", ["POST"])) & ctx.methods()):
            return
        vars_ = None
        for tpl in rule["_tpls"]:
            vars_ = match_template(tpl, ctx)
            if vars_ is not None:
                break
        if vars_ is None:
            return
        vars_["_path_vars"] = list(vars_)
        for v in header(ctx.req, "neon-connection-string"):
            mm = re.match(r"^postgres(?:ql)?://(?:[^@/]*@)?([^/?:]+)(?::\d+)?/([^?]*)", v)
            if mm:   # host and database name only: the password stays out of facts
                vars_["database"] = "%s/%s" % (mm.group(1), mm.group(2))
        for sql, where in sql_texts(m["shape"], ctx):
            res = sqlclass.classify(sql, m.get("dialect", "postgres"))
            facts.setdefault("sql", []).append({"where": where, "verdict": res["verdict"],
                                                "kinds": [s["kind"] for s in res["statements"]]})
            if res["verdict"] == "destructive":
                bad = [s for s in res["statements"] if s["verdict"] == "destructive"]
                v = dict(vars_)
                v["sql_kind"] = bad[0]["kind"].upper()
                v["objects"] = ", ".join(bad[0]["objects"]) or MISSING
                v["statement"] = sql if len(sql) < 120 else sql[:117] + "..."
                self._add(api, rule, v, matches, facts)
            elif res["verdict"] == "unknown":
                reason = res.get("reason") or next((s["reason"] for s in res["statements"] if s["verdict"] == "unknown"), "")
                unknowns.append("SQL Silo cannot classify: %s" % reason)

    # MCP ----------------------------------------------------------
    def _mcp(self, api, rule, ctx, matches, unknowns, facts):
        m = rule["match"]["mcp"]
        if ctx.method != "POST":
            return
        if m.get("paths") and not any(match_template(compile_template(p), ctx, False) is not None for p in m["paths"]):
            return
        ct = ctx.ctype()
        if api.get("mcp_sniff") and "json" not in ct:
            return
        j = ctx.json()
        if j is None:
            if ctx.json_is_bad() and not api.get("mcp_sniff"):
                raise Unknown("the MCP body is not JSON")
            return
        msgs = j if isinstance(j, list) else [j]
        h_method = header(ctx.req, "mcp-method")
        h_name = header(ctx.req, "mcp-name")
        if (h_method or h_name) and rule is api["compiled"][0]:
            b_methods = {m_.get("method") for m_ in msgs if isinstance(m_, dict)}
            b_names = {(m_.get("params") or {}).get("name") for m_ in msgs if isinstance(m_, dict) and m_.get("method") == "tools/call"}
            if (h_method and h_method[0] not in b_methods) or (h_name and h_name[0] not in b_names):
                self._add(api, {"id": "mcp.header-body-mismatch", "outcome": None,
                                "say": "calls an MCP tool on {host} whose Mcp-Method/Mcp-Name headers ({hm} {hn}) do not match the body ({bm} {bn}); Silo decides on the body and holds the mismatch"},
                          {"hm": ",".join(h_method), "hn": ",".join(h_name), "bm": ",".join(sorted(x for x in b_methods if x)),
                           "bn": ",".join(sorted(x for x in b_names if x))}, matches, facts)
        for msg in msgs:
            if not isinstance(msg, dict) or msg.get("method") != "tools/call":
                continue
            params = msg.get("params") or {}
            name = params.get("name")
            if not isinstance(name, str):
                unknowns.append("an MCP tools/call without a tool name")
                continue
            facts.setdefault("mcp_tools", []).append(name)
            if not any(fnmatch.fnmatchcase(name.lower(), g) for g in m["tool"]):
                continue
            args = params.get("arguments") or {}
            vals = {k: v for k, v in flatten_args(args).items() if not isinstance(v, (dict, list))}
            vals["tool"] = name
            if m.get("sql_arg"):
                if m["sql_arg"] not in args:
                    continue
                res = sqlclass.classify(args.get(m["sql_arg"]), m.get("dialect", "postgres"))
                facts.setdefault("sql", []).append({"where": "arguments." + m["sql_arg"], "verdict": res["verdict"]})
                if res["verdict"] == "harmless":
                    continue
                if res["verdict"] == "unknown":
                    unknowns.append("SQL Silo cannot classify in tool %s" % name)
                    continue
                bad = [s for s in res["statements"] if s["verdict"] == "destructive"][0]
                vals["sql_kind"] = bad["kind"].upper()
                vals["objects"] = ", ".join(bad["objects"]) or MISSING
            self._add(api, rule, vals, matches, facts)

    # git ----------------------------------------------------------
    def _git(self, api, rule, ctx, matches, facts):
        if ctx.method != "POST":
            return
        vars_ = match_template(rule["_tpl"], ctx)
        if vars_ is None:
            return
        body = ctx.body()
        zero = re.compile(r"^0+$")
        for old, new, ref in receive_pack_commands(body):
            kind = "delete" if zero.match(new) else ("create" if zero.match(old) else "update")
            facts.setdefault("git_refs", []).append("%s %s" % (kind, ref))
            want = rule["match"]["git"]["ref_change"]
            if kind != want:
                continue
            if rule["match"]["git"].get("refs") and not any(fnmatch.fnmatchcase(ref, g) for g in rule["match"]["git"]["refs"]):
                continue
            v = dict(vars_)
            v["ref"] = ref
            v["branch"] = ref.replace("refs/heads/", "")
            v["repo"] = v.get("repo", "").removesuffix(".git")
            self._add(api, rule, v, matches, facts)


if __name__ == "__main__":
    import sys
    d = Detector.load(sys.argv[1] if len(sys.argv) > 1 else "rules.json")
    req = json.load(sys.stdin)
    req["body"] = base64.b64decode(req.get("body_b64", "")) if "body_b64" in req else (req.get("body") or "").encode()
    t = time.perf_counter()
    r = d.check(req)
    r["ms"] = round((time.perf_counter() - t) * 1000, 3)
    print(json.dumps(r, indent=2))
