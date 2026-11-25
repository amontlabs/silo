"""Which requests may carry the real value of a secret limited to named places on its host.

classify(request, hosts) reads one request the way the boundary sees it after ending TLS: method,
host, target, headers, body. It returns a Verdict: the effect of the request at the host, the
places it touches, and the node ids the boundary would have to look up before it can say where a
GraphQL mutation lands. Nothing here is a guess about intent: every field comes from the bytes of
the request and from data/*.json, which is built from the host's own API description.

decide(verdict, scope) turns that into allow, hold or deny for a secret limited to a Scope.

Effects: read, change, create, expose, runs, destroy, unknown (see build_github_rest.py).
"""

import base64
import gzip
import json
import os
import re
import struct
import zlib
from dataclasses import dataclass, field
from urllib.parse import parse_qs, unquote

from graphql import parse as gql_parse
from graphql.error import GraphQLError
from graphql.language import ast as gast

HERE = os.path.dirname(os.path.abspath(__file__))
REST = json.load(open(os.path.join(HERE, "data", "github-rest.json")))["operations"]
GQL = json.load(open(os.path.join(HERE, "data", "github-graphql.json")))
MUTATIONS = GQL["mutations"]

WITHIN, CREATES_OR_EXPOSES, DESTROYS, UNKNOWN = "within", "creates or exposes", "destroys", "unknown"
BUCKET = {"read": WITHIN, "change": WITHIN, "create": CREATES_OR_EXPOSES, "expose": CREATES_OR_EXPOSES,
          "runs": CREATES_OR_EXPOSES, "destroy": DESTROYS, "unknown": UNKNOWN}
RANK = {"read": 0, "change": 1, "create": 2, "expose": 2, "runs": 2, "destroy": 3, "unknown": 4}
OVERRIDE_HEADERS = {"x-http-method-override", "x-http-method", "x-method-override"}
WORKFLOW_DIRS = ("workflows", "actions")
ZERO = "0" * 40


@dataclass
class Request:
    method: str
    host: str
    target: str
    headers: list = field(default_factory=list)
    body: bytes = b""

    def header(self, name):
        for k, v in self.headers:
            if k.lower() == name:
                return v
        return None


@dataclass
class Verdict:
    effect: str
    places: list = field(default_factory=list)   # "repo:owner/name", "repo-id:123", "node:ID", "account", ...
    tags: list = field(default_factory=list)
    lookups: list = field(default_factory=list)  # node ids or trees the boundary must resolve first
    why: str = ""

    @property
    def bucket(self):
        return BUCKET[self.effect]


def unknown(why, places=()):
    return Verdict("unknown", list(places), [], [], why)


def worst(verdicts):
    """The verdict of a request that does several things is its worst part, with every place."""
    v = max(verdicts, key=lambda x: RANK[x.effect])
    out = Verdict(v.effect, [], [], [], "; ".join(x.why for x in verdicts))
    for x in verdicts:
        out.places += [p for p in x.places if p not in out.places]
        out.tags += [t for t in x.tags if t not in out.tags]
        out.lookups += [n for n in x.lookups if n not in out.lookups]
    return out


# ---------------------------------------------------------------- hosts

@dataclass
class GitHubHosts:
    """Where one GitHub lives. github.com by default; GHES puts everything on one host."""
    api: str = "api.github.com"
    web: str = "github.com"
    uploads: str = "uploads.github.com"
    api_prefix: str = ""
    graphql_path: str = "/graphql"
    uploads_prefix: str = ""

    @classmethod
    def enterprise_server(cls, host):
        return cls(api=host, web=host, uploads=host, api_prefix="/api/v3", graphql_path="/api/graphql",
                   uploads_prefix="/api/uploads")


GITHUB_READ_ONLY_HOSTS = {"codeload.github.com", "raw.githubusercontent.com", "objects.githubusercontent.com",
                          "avatars.githubusercontent.com", "media.githubusercontent.com"}


def strict_path(target):
    """Split a target into path and query, or say why the path is not one plain spelling.

    A path that two programs could read two ways is not classified: an encoded slash or dot, a
    backslash, an empty or dot segment. GitHub's own paths never need them."""
    path, _, query = target.partition("?")
    if not path.startswith("/"):
        return None, None, "the target is not a path"
    if re.search(r"%2[fFeE]|%5[cC]|%00|\\|;", path):
        return None, None, "the path holds an encoded slash, dot, backslash or a semicolon"
    segs = path.split("/")[1:]
    if any(s in ("", ".", "..") for s in segs[:-1]) or segs[-1] in (".", ".."):
        return None, None, "the path holds an empty or dot segment"
    if any(ord(c) > 126 or ord(c) < 33 for c in path):
        return None, None, "the path holds a character outside printable ASCII"
    return path, query, None


def decoded_body(req):
    enc = (req.header("content-encoding") or "").lower()
    if enc in ("", "identity"):
        return req.body
    if enc == "gzip":
        return gzip.decompress(req.body)
    if enc == "deflate":
        return zlib.decompress(req.body)
    raise ValueError(f"content encoding {enc} is not read")


NAME = re.compile(r"^[A-Za-z0-9_.-]+$")


def repo_place(owner, repo):
    repo = repo[:-4] if repo.lower().endswith(".git") else repo
    if not (NAME.match(owner) and NAME.match(repo)) or repo in (".", ".."):
        return None
    return f"repo:{owner.lower()}/{repo.lower()}"


# ---------------------------------------------------------------- GitHub REST

MULTI_SEGMENT = {"ref", "path", "branch"}


def _compile():
    table = []
    for op in REST:
        parts = op["path"].strip("/").split("/") if op["path"] != "/" else []
        literal = sum(1 for p in parts if not p.startswith("{"))
        def seg(p):
            if not p.startswith("{"):
                return re.escape(p)
            name = p[1:-1].replace("-", "_")
            # These parameters hold names that may contain a slash: a Git ref, a file path, a branch.
            return f"(?P<{name}>.+)" if name in MULTI_SEGMENT else f"(?P<{name}>[^/]+)"
        rx = "^/" + "/".join(seg(p) for p in parts) + "$"
        table.append((re.compile(rx), literal, len(parts), op))
    # More literal segments first, so /repos/{o}/{r}/pulls beats /repos/{o}/{r}/{anything}.
    table.sort(key=lambda t: (-t[1], -t[2]))
    return table


REST_TABLE = _compile()

SAFE_REPO_FIELDS = {"description", "homepage", "has_issues", "has_projects", "has_wiki", "has_discussions",
                    "allow_squash_merge", "allow_merge_commit", "allow_rebase_merge", "allow_auto_merge",
                    "delete_branch_on_merge", "allow_update_branch", "use_squash_pr_title_as_default",
                    "squash_merge_commit_title", "squash_merge_commit_message", "merge_commit_title",
                    "merge_commit_message", "web_commit_signoff_required", "is_template"}


def classify_rest(req, path, query, hosts):
    method = "GET" if req.method.upper() == "HEAD" else req.method.upper()
    matches = [(rx.match(path), op) for rx, _, _, op in REST_TABLE if op["method"] == method]
    matches = [(m, op) for m, op in matches if m]
    if len({op["effect"] for _, op in matches}) > 1:
        # A branch named "x/protection" makes /branches/x/protection two operations. GitHub picks
        # one; the boundary does not guess which.
        return unknown(f"{req.method} {path} matches operations with different effects: "
                       + ", ".join(op["op"] for _, op in matches))
    for m, op in matches[:1]:
        g = m.groupdict()
        places = []
        owner, repo = g.get("owner") or g.get("template_owner"), g.get("repo") or g.get("template_repo")
        if owner and repo:
            p = repo_place(owner, repo)
            if not p:
                return unknown("the repository name is not one plain spelling")
            places.append(p)
        elif "repository_id" in g:
            places.append(f"repo-id:{g['repository_id']}")
        elif op["scope"] == "account":
            places.append("account")
        elif op["scope"] == "gist":
            places.append(f"gist:{g.get('gist_id', 'new')}")
        elif op["scope"] == "org":
            places.append(f"org:{(g.get('org') or '').lower()}")
        else:
            places.append(op["scope"])
        v = Verdict(op["effect"], places, list(op["tags"]), [], f"REST {op['op']}")
        return refine_rest(req, op, g, v)
    return unknown(f"no operation of GitHub's REST description is {req.method} {path}")


def no_duplicates(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        # {"visibility": "private", "visibility": "public"}: which one the host keeps is not ours to guess.
        raise ValueError("a JSON object names a key twice")
    return dict(pairs)


def strict_json(b):
    return json.loads(b, object_pairs_hook=no_duplicates)


def json_body(req):
    try:
        b = decoded_body(req)
        return strict_json(b) if b.strip() else {}
    except (ValueError, OSError):
        return None


def refine_rest(req, op, g, v):
    """Facts in the body that change the effect of a few operations."""
    if "body" not in op["tags"]:
        return v
    body = json_body(req)
    if body is None or not isinstance(body, dict):
        return unknown(f"REST {op['op']}: the body is not a JSON object", v.places)
    name = op["op"]
    if name == "repos/update":
        if set(body) <= SAFE_REPO_FIELDS:
            v.effect, v.why = "change", v.why + " (settings only)"
        else:
            v.why += " (names " + ", ".join(sorted(set(body) - SAFE_REPO_FIELDS)) + ")"
    elif name == "repos/create-or-update-file-contents":
        p = g.get("path", "")
        if p.lower().startswith((".github/workflows/", ".github/actions/")):
            v.effect, v.why = "runs", v.why + " (writes a workflow)"
    elif name == "git/update-ref":
        if body.get("force") is True:
            v.effect, v.why = "destroy", v.why + " (force)"
    elif name in ("repos/create-release", "repos/update-release"):
        if body.get("draft") is True:
            v.effect, v.why = "change", v.why + " (draft)"
    elif name == "repos/create-fork":
        if body.get("organization"):
            v.places.append(f"org:{str(body['organization']).lower()}")
    return v


# ---------------------------------------------------------------- GitHub GraphQL

def gql_value(node, variables):
    if node is None:
        return None
    if isinstance(node, gast.VariableNode):
        return variables.get(node.name.value)
    if isinstance(node, gast.ObjectValueNode):
        return {f.name.value: gql_value(f.value, variables) for f in node.fields}
    if isinstance(node, gast.ListValueNode):
        return [gql_value(x, variables) for x in node.values]
    if isinstance(node, gast.NullValueNode):
        return None
    return getattr(node, "value", None)


def top_fields(selection_set, fragments, seen=()):
    for sel in selection_set.selections:
        if isinstance(sel, gast.FieldNode):
            yield sel
        elif isinstance(sel, gast.InlineFragmentNode):
            yield from top_fields(sel.selection_set, fragments, seen)
        elif isinstance(sel, gast.FragmentSpreadNode):
            name = sel.name.value
            if name in seen:
                raise GraphQLError(f"fragment {name} spreads itself")
            if name not in fragments:
                raise GraphQLError(f"fragment {name} is not defined")
            yield from top_fields(fragments[name].selection_set, fragments, seen + (name,))


def dig(value, path):
    """Values at a dotted path, through lists."""
    vals = [value]
    for key in path.split("."):
        nxt = []
        for v in vals:
            if isinstance(v, list):
                nxt += [x.get(key) for x in v if isinstance(x, dict)]
            elif isinstance(v, dict):
                nxt.append(v.get(key))
        vals = nxt
    out = []
    for v in vals:
        out += v if isinstance(v, list) else [v]
    return [v for v in out if v is not None]


def classify_mutation(f, variables):
    name = f.name.value
    entry = MUTATIONS.get(name)
    if entry is None:
        return unknown(f"GraphQL mutation {name} is not in GitHub's schema")
    args = {a.name.value: gql_value(a.value, variables) for a in (f.arguments or [])}
    v = Verdict(entry["effect"], [], list(entry["tags"]), [], f"GraphQL {name}")
    for place in entry["places"]:
        for val in dig(args, place):
            if place.endswith("NameWithOwner"):
                owner, _, repo = str(val).partition("/")
                p = repo_place(owner, repo)
                if not p:
                    return unknown(f"GraphQL {name}: {place} is not owner/name")
                v.places.append(p)
            elif place.endswith("Login"):
                v.places.append(f"login:{str(val).lower()}")
            else:
                v.places.append(f"node:{val}")
                v.lookups.append(str(val))
    if name == "createRepository" or name == "cloneTemplateRepository":
        v.places.append("new-repository")
    inp = args.get("input") or {}
    if name == "updateRepository" and isinstance(inp, dict) and "name" in inp:
        v.effect, v.why = "expose", v.why + " (renames the repository)"
    if name == "createIssue" and isinstance(inp, dict) and inp.get("agentAssignment"):
        v.effect, v.why = "runs", v.why + " (assigns an agent)"
    if not v.places:
        v.places.append("account")
    return v


ACCOUNT_QUERY_FIELDS = {"viewer", "rateLimit", "meta", "relay", "codeOfConduct", "codesOfConduct", "license",
                        "licenses", "__typename", "__schema", "__type"}


def classify_query(f, variables):
    name = f.name.value
    args = {a.name.value: gql_value(a.value, variables) for a in (f.arguments or [])}
    if name == "repository":
        p = repo_place(str(args.get("owner", "")), str(args.get("name", "")))
        return Verdict("read", [p], [], [], "GraphQL query repository") if p else \
            unknown("GraphQL repository() without a plain owner and name")
    if name in ("node", "nodes"):
        ids = args.get("id") if name == "node" else args.get("ids")
        ids = ids if isinstance(ids, list) else [ids]
        return Verdict("read", [f"node:{i}" for i in ids], [], [str(i) for i in ids], f"GraphQL query {name}")
    if name == "viewer" and f.selection_set and all(
            isinstance(s, gast.FieldNode) and s.selection_set is None for s in f.selection_set.selections):
        return Verdict("read", ["account"], [], [], "GraphQL query viewer (own fields only)")
    if name in ACCOUNT_QUERY_FIELDS and name != "viewer":
        return Verdict("read", ["account"], [], [], f"GraphQL query {name}")
    if name not in GQL["query_fields"]:
        return unknown(f"GraphQL query field {name} is not in GitHub's schema")
    return Verdict("read", ["wide"], [], [], f"GraphQL query {name} (reaches places it does not name)")


def classify_graphql_document(text, variables, operation_name):
    try:
        doc = gql_parse(text, no_location=True)
    except GraphQLError as e:
        return unknown(f"GraphQL that does not parse: {e.message}")
    fragments = {d.name.value: d for d in doc.definitions if isinstance(d, gast.FragmentDefinitionNode)}
    ops = [d for d in doc.definitions if isinstance(d, gast.OperationDefinitionNode)]
    if not ops:
        return unknown("GraphQL document without an operation")
    # Every operation in the document counts, whatever operationName says: the host picks one,
    # and the boundary does not rely on picking the same one.
    verdicts = []
    for op in ops:
        vars_ = dict(variables or {})
        for vd in op.variable_definitions or []:
            if vd.variable.name.value not in vars_ and vd.default_value is not None:
                vars_[vd.variable.name.value] = gql_value(vd.default_value, {})
        kind = op.operation.value
        if kind == "subscription":
            verdicts.append(unknown("GraphQL subscription"))
            continue
        try:
            fields = list(top_fields(op.selection_set, fragments))
        except GraphQLError as e:
            verdicts.append(unknown(f"GraphQL: {e.message}"))
            continue
        for f in fields:
            if f.name.value == "__typename":
                continue
            verdicts.append(classify_mutation(f, vars_) if kind == "mutation" else classify_query(f, vars_))
    return worst(verdicts) if verdicts else unknown("GraphQL operation that selects nothing")


def classify_graphql(req, query):
    if req.method.upper() == "GET":
        q = parse_qs(query)
        if "query" not in q:
            return unknown("GraphQL GET without a query")
        variables = json.loads(q.get("variables", ["{}"])[0])
        return classify_graphql_document(q["query"][0], variables, q.get("operationName", [None])[0])
    if req.method.upper() != "POST":
        return unknown(f"GraphQL sent as {req.method}")
    try:
        raw = decoded_body(req)
    except (ValueError, OSError) as e:
        return unknown(f"GraphQL body: {e}")
    if (req.header("content-type") or "").startswith("application/graphql"):
        return classify_graphql_document(raw.decode("utf-8", "replace"), {}, None)
    try:
        body = strict_json(raw)
    except ValueError as e:
        return unknown(f"GraphQL body is not plain JSON: {e}")
    batch = body if isinstance(body, list) else [body]
    verdicts = []
    for item in batch:
        if not isinstance(item, dict) or not isinstance(item.get("query"), str):
            verdicts.append(unknown("GraphQL request without a query text (a persisted query or an id)"))
            continue
        variables = item.get("variables") or {}
        if not isinstance(variables, dict):
            verdicts.append(unknown("GraphQL variables are not an object"))
            continue
        verdicts.append(classify_graphql_document(item["query"], variables, item.get("operationName")))
    if not verdicts:
        return unknown("empty GraphQL batch")
    return worst(verdicts)


# ---------------------------------------------------------------- Git over HTTP

def pkt_lines(data, pos=0):
    """Yields (line or None for a flush, next position) until the first flush is consumed."""
    while pos + 4 <= len(data):
        n = int(data[pos:pos + 4], 16)
        if n == 0:
            yield None, pos + 4
            return
        if n < 4:
            raise ValueError("bad pkt-line length")
        yield data[pos + 4:pos + n], pos + n
        pos += n
    raise ValueError("pkt-lines end without a flush")


TYPES = {1: "commit", 2: "tree", 3: "blob", 4: "tag", 6: "ofs", 7: "ref"}


def apply_delta(base, delta):
    def varint(i):
        v = s = 0
        while True:
            b = delta[i]; i += 1
            v |= (b & 0x7f) << s; s += 7
            if not b & 0x80:
                return v, i
    _, i = varint(0)
    _, i = varint(i)
    out = bytearray()
    while i < len(delta):
        op = delta[i]; i += 1
        if op & 0x80:
            off = size = 0
            for k in range(4):
                if op & (1 << k):
                    off |= delta[i] << (8 * k); i += 1
            for k in range(3):
                if op & (0x10 << k):
                    size |= delta[i] << (8 * k); i += 1
            out += base[off:off + (size or 0x10000)]
        elif op:
            out += delta[i:i + op]; i += op
        else:
            raise ValueError("delta opcode 0")
    return bytes(out)


def read_pack(data):
    """Objects of a pack, by sha1. Deltas whose base is outside the pack (a thin pack) are listed apart."""
    import hashlib
    if data[:4] != b"PACK":
        raise ValueError("no pack")
    count = struct.unpack(">I", data[8:12])[0]
    pos, raw, by_offset, pending = 12, {}, {}, []
    for _ in range(count):
        start = pos
        c = data[pos]; pos += 1
        typ, size, shift = (c >> 4) & 7, c & 15, 4
        while c & 0x80:
            c = data[pos]; pos += 1
            size |= (c & 0x7f) << shift; shift += 7
        base = None
        if typ == 6:
            c = data[pos]; pos += 1
            off = c & 0x7f
            while c & 0x80:
                c = data[pos]; pos += 1
                off = ((off + 1) << 7) | (c & 0x7f)
            base = ("ofs", start - off)
        elif typ == 7:
            base = ("ref", data[pos:pos + 20].hex()); pos += 20
        d = zlib.decompressobj()
        body = d.decompress(data[pos:])
        pos = len(data) - len(d.unused_data)
        pending.append((start, TYPES[typ], body, base))
    objects, thin = {}, []
    resolved = {}
    progress = True
    while pending and progress:
        progress, rest = False, []
        for start, typ, body, base in pending:
            if base is None:
                full_t, full = typ, body
            else:
                b = resolved.get(base[1]) if base[0] == "ofs" else objects.get(base[1])
                if b is None:
                    rest.append((start, typ, body, base))
                    continue
                full_t, full = b[0], apply_delta(b[1], body)
            sha = hashlib.sha1(f"{full_t} {len(full)}".encode() + b"\0" + full).hexdigest()
            objects[sha] = (full_t, full)
            resolved[start] = (full_t, full)
            progress = True
        pending = rest
    thin = [base[1] for _, _, _, base in pending if base[0] == "ref"]
    if pending and not thin:
        raise ValueError("pack deltas that do not resolve")
    return objects, thin


def tree_entries(body):
    out, i = {}, 0
    while i < len(body):
        sp = body.index(b" ", i); nul = body.index(b"\0", sp)
        out[body[sp + 1:nul].decode("utf-8", "replace")] = (body[i:sp].decode(), body[nul + 1:nul + 21].hex())
        i = nul + 21
    return out


def commit_fields(body):
    head = body.split(b"\n\n", 1)[0].decode("utf-8", "replace")
    tree, parents = None, []
    for line in head.split("\n"):
        if line.startswith("tree "):
            tree = line[5:]
        elif line.startswith("parent "):
            parents.append(line[7:])
    return tree, parents


def workflow_trees(objects, tree_sha):
    """(id of .github, {workflows: id, actions: id}) in a tree.

    The dict is None when the .github tree is not in the pack; the root tree missing gives None."""
    t = objects.get(tree_sha)
    if t is None:
        return None
    gh = tree_entries(t[1]).get(".github")
    if gh is None:
        return ("", {})
    sub = objects.get(gh[1])
    if sub is None:
        return (gh[1], None)
    ents = tree_entries(sub[1])
    return (gh[1], {d: ents[d][1] for d in WORKFLOW_DIRS if d in ents})


def classify_receive_pack(req, place, tree_of):
    """A push. tree_of(place, commit) answers what the host already has (a read the boundary makes)."""
    try:
        data = decoded_body(req)
    except (ValueError, OSError) as e:
        return unknown(f"push body: {e}", [place])
    commands, pos, caps = [], 0, ""
    try:
        for line, pos in pkt_lines(data):
            if line is None:
                break
            text, _, more = line.partition(b"\0")
            if not commands:
                caps = more.decode("ascii", "replace")
            old, new, ref = text.decode("utf-8").strip().split(" ", 2)
            commands.append((old, new, ref))
        if "push-options" in caps.split():
            for line, pos2 in pkt_lines(data, pos):
                pos = pos2
                if line is None:
                    break
        objects, thin = read_pack(data[pos:]) if data[pos:pos + 4] == b"PACK" else ({}, [])
    except (ValueError, IndexError, zlib.error) as e:
        return unknown(f"push that does not parse: {e}", [place])
    verdicts = []
    for old, new, ref in commands:
        tags = (["tag"] if ref.startswith("refs/tags/") else []) + [f"ref:{ref}"]
        if new == ZERO:
            verdicts.append(Verdict("destroy", [place], tags, [], f"push deletes {ref}"))
            continue
        effect, why = "change", f"push {ref}"
        if old != ZERO and not fast_forward(objects, old, new):
            effect, why = "destroy", f"push {ref} that may rewrite it: {old[:7]} is not an ancestor in the pack"
        wf = changed_workflows(objects, place, old, new, ref, tree_of)
        if wf is None:
            verdicts.append(Verdict("unknown", [place], tags, [f"tree:{new}"],
                                    why + "; the files it changes under .github are not at hand"))
            continue
        if wf:
            effect, why = ("runs" if effect == "change" else effect), why + f"; changes .github/{', .github/'.join(wf)}"
        verdicts.append(Verdict(effect, [place], tags, [], why))
    if not verdicts:
        return unknown("push without commands", [place])
    return worst(verdicts)


def fast_forward(objects, old, new):
    seen, todo = set(), [new]
    while todo:
        c = todo.pop()
        if c == old:
            return True
        if c in seen or c not in objects or objects[c][0] != "commit":
            continue
        seen.add(c)
        todo += commit_fields(objects[c][1])[1]
    return False


def changed_workflows(objects, place, old, new, ref, tree_of):
    """Which of .github/workflows and .github/actions differ between what the ref held and what it gets.

    Returns a list (maybe empty), or None when a tree needed is neither in the pack nor known.
    tree_of(place, commit) is the boundary's own read of the host, used only when the pack lacks it."""
    def wf_of(commit, need_dirs):
        got = None
        if commit in objects and objects[commit][0] == "commit":
            got = workflow_trees(objects, commit_fields(objects[commit][1])[0])
        if got is None or (need_dirs and got[1] is None):
            got = tree_of(place, commit) if tree_of else None
        return got
    after = wf_of(new, False)
    before_commit = old if old != ZERO else "default-branch"
    before = wf_of(before_commit, False)
    if after is None or before is None:
        return None
    if after[0] == before[0]:
        return []
    if after[1] is None:
        after = wf_of(new, True)
    if before[1] is None:
        before = wf_of(before_commit, True)
    if after is None or before is None or after[1] is None or before[1] is None:
        return None
    return [d for d in WORKFLOW_DIRS if after[1].get(d) != before[1].get(d)]


def classify_git(req, path, query, tree_of):
    m = re.match(r"^/([^/]+)/([^/]+?)(\.git)?/(info/refs|git-upload-pack|git-receive-pack|info/lfs/(.+))$", path)
    if not m:
        return None
    place = repo_place(m.group(1), m.group(2))
    if not place:
        return unknown("the repository name in a Git path is not one plain spelling")
    what, method = m.group(4), req.method.upper()
    if what == "info/refs" and method == "GET":
        svc = parse_qs(query).get("service", [""])[0]
        if svc not in ("", "git-upload-pack", "git-receive-pack"):
            return unknown(f"Git service {svc}", [place])
        return Verdict("read", [place], ["push-intent"] if svc == "git-receive-pack" else [], [], f"Git refs ({svc or 'dumb'})")
    if what == "git-upload-pack" and method == "POST":
        return Verdict("read", [place], [], [], "Git fetch")
    if what == "git-receive-pack" and method == "POST":
        return classify_receive_pack(req, place, tree_of)
    if what.startswith("info/lfs/"):
        sub = m.group(5)
        if sub == "objects/batch" and method == "POST":
            body = json_body(req)
            op = body.get("operation") if isinstance(body, dict) else None
            if op == "download":
                return Verdict("read", [place], ["lfs"], [], "LFS download")
            if op == "upload":
                return Verdict("change", [place], ["lfs"], [], "LFS upload")
            return unknown("LFS batch without a known operation", [place])
        if sub.startswith("locks"):
            return Verdict("read" if method == "GET" else "change", [place], ["lfs"], [], "LFS locks")
    return unknown(f"Git {method} {what}", [place])


# ---------------------------------------------------------------- npm, PyPI, S3

def classify_npm(req, path, query):
    method = req.method.upper()
    if path.startswith("/-/"):
        if re.match(r"^/-/npm/v1/tokens", path):
            return Verdict("read" if method == "GET" else ("destroy" if method == "DELETE" else "expose"),
                           ["account"], [], [], "npm tokens")
        m = re.match(r"^/-/package/(@[^/]+(?:%2[fF]|/)[^/]+|[^@/][^/]*)/(dist-tags|access|collaborators)", path)
        if m:
            name = unquote(m.group(1)).lower()
            eff = "read" if method == "GET" else "expose"
            return Verdict(eff, [f"package:npm/{name}"], ["publish"] if m.group(2) == "dist-tags" else [], [],
                           f"npm {m.group(2)}")
        if re.match(r"^/-/(v1/search|npm/v1/security/advisories/bulk|npm/v1/security/audits)", path):
            return Verdict("read", ["global"], [], [], "npm search or audit")
        if path.startswith("/-/user/") and method == "PUT":
            return Verdict("expose", ["account"], [], [], "npm login, which makes a token")
        if path.startswith("/-/whoami") or path == "/-/ping":
            return Verdict("read", ["account"], [], [], "npm whoami")
        return unknown(f"npm {method} {path}")
    m = re.match(r"^/(@[^/]+(?:%2[fF]|/)[^/]+|[^@/][^/]*)(/-rev/[^/]+|/-/.+|/[^/]+)?$", path)
    if not m:
        return unknown(f"npm path {path}")
    name = unquote(m.group(1)).lower()
    place = f"package:npm/{name}"
    rest = m.group(2) or ""
    if method in ("GET", "HEAD"):
        return Verdict("read", [place], [], [], "npm metadata or tarball")
    if method == "PUT" and rest == "":
        body = json_body(req)
        if isinstance(body, dict) and body.get("_attachments"):
            return Verdict("expose", [place], ["publish"], [], "npm publish")
        return unknown("npm PUT of a package document without a tarball", [place])
    if method == "PUT" and rest.startswith("/-rev/"):
        return Verdict("destroy", [place], [], [], "npm change of the package document (unpublish, deprecate, owners)")
    if method == "DELETE":
        return Verdict("destroy", [place], [], [], "npm unpublish")
    return unknown(f"npm {method} {path}", [place])


def classify_pypi_upload(req, path):
    if req.method.upper() != "POST" or path not in ("/legacy/", "/legacy"):
        return unknown(f"PyPI upload host {req.method} {path}")
    ctype = req.header("content-type") or ""
    m = re.search(r'boundary="?([^";]+)"?', ctype)
    if not m:
        return unknown("PyPI upload that is not multipart")
    fields = {}
    for part in req.body.split(b"--" + m.group(1).encode()):
        hm = re.search(rb'name="([^"]+)"', part.split(b"\r\n\r\n", 1)[0])
        if hm and b"filename=" not in part.split(b"\r\n\r\n", 1)[0]:
            fields[hm.group(1).decode()] = part.split(b"\r\n\r\n", 1)[1].rsplit(b"\r\n", 1)[0].decode("utf-8", "replace")
    if fields.get(":action") != "file_upload" or "name" not in fields:
        return unknown("PyPI upload without :action=file_upload and a name")
    name = re.sub(r"[-_.]+", "-", fields["name"]).lower()
    return Verdict("expose", [f"package:pypi/{name}"], ["publish"], [], "PyPI upload")


S3_EXPOSE_SUBRESOURCES = {"policy", "acl", "publicAccessBlock", "cors", "website", "ownershipControls",
                          "replication", "notification", "accelerate", "logging"}


def classify_s3(req, path, query):
    host = req.host.lower()
    m = re.match(r"^(?:(?P<bucket>[a-z0-9][a-z0-9.-]*)\.)?s3(?:[.-][a-z0-9-]+)?\.amazonaws\.com$", host)
    if not m:
        return None
    bucket, key = m.group("bucket"), path.lstrip("/")
    if bucket is None:
        bucket, _, key = key.partition("/")
    if not bucket:
        return Verdict("read", ["account"], [], [], "S3 ListBuckets") if req.method == "GET" else unknown("S3 request without a bucket")
    place, sub = f"bucket:{bucket}", set(parse_qs(query, keep_blank_values=True))
    method = req.method.upper()
    tags = ["owner-checked"] if req.header("x-amz-expected-bucket-owner") else []
    if method in ("GET", "HEAD"):
        return Verdict("read", [place], tags, [], "S3 read")
    if sub & S3_EXPOSE_SUBRESOURCES or (req.header("x-amz-acl") or "").startswith("public"):
        return Verdict("expose", [place], tags, [], "S3 change of who can read a bucket or object")
    if method == "PUT" and not key and not sub:
        return Verdict("create", [place], tags, [], "S3 CreateBucket")
    if method == "DELETE" or (method == "POST" and "delete" in sub):
        return Verdict("destroy", [place], tags, [], "S3 delete")
    if method in ("PUT", "POST"):
        if req.header("x-amz-copy-source"):
            src = unquote(req.header("x-amz-copy-source")).lstrip("/").split("/")[0]
            return Verdict("change", [place, f"bucket:{src}"], tags, [], "S3 CopyObject")
        return Verdict("change", [place], tags, [], "S3 write")
    return unknown(f"S3 {method}", [place])


# ---------------------------------------------------------------- entry points

def classify(req, hosts=GitHubHosts(), tree_of=None):
    target = req.target
    if req.host.lower() == "registry.npmjs.org":
        # npm spells a scoped name with one encoded slash: /@scope%2fname. It is the only one allowed.
        target = re.sub(r"^/(-/package/)?@([A-Za-z0-9_.-]+)%2[fF]", r"/\1@\2/", target, count=1)
    path, query, bad = strict_path(target)
    if bad:
        return unknown(bad)
    if req.method.upper() not in ("GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"):
        return unknown(f"method {req.method}")
    if any(k.lower() in OVERRIDE_HEADERS for k, _ in req.headers) or "_method" in parse_qs(query):
        return unknown("the request asks the host to read it as another method")
    host = req.host.lower().rstrip(".")
    if host == hosts.api and path == hosts.graphql_path:
        return classify_graphql(req, query)
    if host == hosts.web:
        g = classify_git(req, path, query, tree_of)
        if g is not None:
            return g
    if host == hosts.uploads and path.startswith(hosts.uploads_prefix + "/repos/"):
        sub = path[len(hosts.uploads_prefix):]
        return classify_rest(req, sub, query, hosts)
    if host == hosts.api and path.startswith(hosts.api_prefix + "/"):
        return classify_rest(req, path[len(hosts.api_prefix):] if hosts.api_prefix else path, query, hosts)
    if host == "gist.github.com":
        m = re.match(r"^/(?:[^/]+/)?([0-9a-f]+)(\.git)?/(info/refs|git-upload-pack|git-receive-pack)$", path)
        if m:
            eff = "change" if m.group(3) == "git-receive-pack" else "read"
            return Verdict(eff, [f"gist:{m.group(1)}"], [], [], "Git to a gist")
    if host in GITHUB_READ_ONLY_HOSTS:
        if req.method.upper() in ("GET", "HEAD"):
            parts = path.split("/")
            p = repo_place(parts[1], parts[2]) if len(parts) > 2 and host != "objects.githubusercontent.com" else None
            return Verdict("read", [p or "global"], [], [], f"read from {host}")
        return unknown(f"{req.method} to {host}, which only serves reads")
    if host == "registry.npmjs.org":
        return classify_npm(req, path, query)
    if host == "upload.pypi.org":
        return classify_pypi_upload(req, path)
    s3 = classify_s3(req, path, query) if host.endswith("amazonaws.com") else None
    if s3 is not None:
        return s3
    return unknown(f"no description of {host} {path}")


@dataclass
class Scope:
    """What a secret is limited to. Names are lowercase. Ids come from the host when the limit is set."""
    repos: set = field(default_factory=set)          # "owner/name"
    repo_ids: dict = field(default_factory=dict)     # database id -> "owner/name"
    repo_nodes: dict = field(default_factory=dict)   # node id -> "owner/name" (named repositories)
    node_repo: dict = field(default_factory=dict)    # node id -> "owner/name" or "" (looked up, any node)
    packages: set = field(default_factory=set)       # "npm/name", "pypi/name"
    buckets: set = field(default_factory=set)
    reads: str = "named"                             # "named" or "anywhere"
    default_branches: dict = field(default_factory=dict)  # "owner/name" -> "main", read when the limit is set
    public: set = field(default_factory=set)         # named repositories GitHub said are public
    hold_public: bool = False                        # hold writes that land in a public repository
    allow: set = field(default_factory=set)          # effects or tags the user allowed without a hold


def place_named(place, scope):
    kind, _, rest = place.partition(":")
    if kind == "repo":
        return rest in scope.repos
    if kind == "repo-id":
        return rest in scope.repo_ids
    if kind == "node":
        if rest in scope.repo_nodes:
            return True
        return bool(scope.node_repo.get(rest)) and scope.node_repo[rest] in scope.repos
    if kind == "package":
        return rest in scope.packages
    if kind == "bucket":
        return rest in scope.buckets
    return False


def decide(v, scope):
    """allow, hold or deny, for a request carrying a secret limited to scope. Deny means the secret
    is not swapped in and the request is refused, as secret-limits says."""
    if v.effect == "unknown":
        return "hold", f"not classified: {v.why}"
    missing = [n for n in v.lookups if n.startswith("tree:") or
               (n not in scope.repo_nodes and n not in scope.node_repo)]
    if missing:
        return "hold", f"the boundary must look up {', '.join(missing)} first"
    places = [p for p in v.places if p not in ("account",)] if v.effect == "read" else v.places
    meta_only = all(p in ("account", "global") for p in v.places)
    inside = all(place_named(p, scope) for p in places) if places else True
    if v.effect == "read":
        if scope.reads == "anywhere" or inside or meta_only:
            return "allow", v.why
        return "deny", f"read outside the named places: {v.why}"
    if v.effect == "create":
        return ("allow" if "create" in scope.allow else "hold"), v.why
    if not inside:
        return "deny", f"{v.effect} outside the named places: {v.why}"
    if v.effect == "change":
        tags = list(v.tags)
        for p in v.places:
            repo = p.partition(":")[2]
            if p.startswith("repo:") and f"ref:refs/heads/{scope.default_branches.get(repo)}" in tags:
                tags.append("merge")  # a push to the default branch is a merge, as merges-and-deploys-held means it
            if p.startswith("repo:") and repo in scope.public and scope.hold_public:
                tags.append("public")
        held = [t for t in tags if t in ("merge", "deploy", "public") and t not in scope.allow]
        return ("hold" if held else "allow"), v.why + (f" (held as {', '.join(held)})" if held else "")
    return ("allow" if v.effect in scope.allow else "hold"), v.why


# ---------------------------------------------------------------- credentials that are not Silo's

CRED_HEADERS = {"authorization", "proxy-authorization", "cookie", "x-api-key", "private-token", "x-auth-token",
                "x-github-token", "x-amz-security-token"}
CRED_QUERY = {"access_token", "token", "api_key", "apikey", "key", "private_token"}


def credentials(req):
    """Every credential the request carries, as (where, value). Basic auth is decoded."""
    out = []
    for k, v in req.headers:
        lk = k.lower()
        if lk not in CRED_HEADERS:
            continue
        if lk in ("authorization", "proxy-authorization"):
            scheme, _, val = v.partition(" ")
            if scheme.lower() == "basic":
                try:
                    user, _, pw = base64.b64decode(val).decode().partition(":")
                    out += [(f"{k} basic user", user), (f"{k} basic password", pw)]
                    continue
                except (ValueError, UnicodeDecodeError):
                    pass
            out.append((k, val or v))
        else:
            out.append((k, v))
    for k, vals in parse_qs(req.target.partition("?")[2]).items():
        if k.lower() in CRED_QUERY:
            out += [(f"query {k}", x) for x in vals]
    return out


BASIC_USERS = {"x-access-token", "x-oauth-basic", "git", "oauth2", "token", "__token__", ""}


def foreign_credentials(req, placeholders):
    """Credentials on the request that are not one of Silo's placeholders."""
    found = credentials(req)
    passwords = {w.rsplit(" ", 1)[0]: v for w, v in found if w.endswith("basic password")}
    bad = []
    for where, val in found:
        if where.endswith("basic user"):
            # A user name is not a credential when the password is a placeholder, or the name is a
            # fixed word such as x-access-token. A token can also be sent as the user name.
            pw = passwords.get(where.rsplit(" ", 1)[0], "")
            if val.lower() in BASIC_USERS or any(p in pw for p in placeholders):
                continue
        if where.endswith("basic password") and val == "":
            continue
        if not any(p in val for p in placeholders):
            bad.append(where)
    return bad
