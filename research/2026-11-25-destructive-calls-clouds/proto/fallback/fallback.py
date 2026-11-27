"""A generic fallback for writes no rule names: it reads only the method, the path, the operation
name the request carries, and a few body markers, and knows no provider.

Written and committed before it was scored on anything (see the README). It answers the question
"if a rule set has no rule for this request, could Silo still tell a destructive call from a
harmless one?" It is scored alone, and behind the provider rules (rules first, the fallback only
when no rule matched).

    classify(req) -> {"decision": "hold" | "allow", "outcome": str | None, "why": str, "name": str}

The request has the shape #353's detector takes: host, method, target, headers, body, facts.
"""
import json
import os
import re
import sys

OLD = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..",
                                    "2026-11-24-destructive-calls", "proto", "detector"))
if OLD not in sys.path:
    sys.path.insert(0, OLD)
import detector as D  # noqa: E402  only its request reading (Ctx), not its rules

READ_METHODS = {"GET", "HEAD", "OPTIONS"}

# First word of an operation name that only reads.
READ_VERBS = {"get", "list", "describe", "read", "search", "query", "fetch", "lookup", "find", "head",
              "check", "validate", "test", "verify", "preview", "estimate", "simulate", "scan", "select",
              "poll", "receive", "download", "export", "watch", "view", "show", "count", "batchget",
              "dryrun", "diff", "explain", "decrypt", "detect", "discover", "filter", "inspect", "whoami"}

# Words that undo or stop an undo; an operation named with one is not destructive by its name.
UNDO = {"cancel", "undo", "undelete", "recover", "unarchive", "unpause", "resume", "unsuspend",
        "uncordon", "unlock", "restore", "untrash", "reactivate", "abort"}

GONE = {"delete", "destroy", "drop", "purge", "truncate", "wipe", "erase", "remove", "terminate",
        "deletecollection", "rm", "trash", "expunge", "nuke", "flush", "clear", "overwrite"}
OUTAGE = {"stop", "kill", "shutdown", "poweroff", "deallocate", "suspend", "pause", "disable", "detach",
          "deregister", "unregister", "unlink", "uninstall", "evict", "revoke", "reset", "rotate",
          "regenerate", "archive", "failover", "cordon", "drain", "disconnect", "unassign", "unbind",
          "unpublish", "unset", "downgrade", "lock", "block", "ban", "expire", "invalidate", "rollback"}
MONEY = {"purchase", "buy", "subscribe", "upgrade", "reserve", "charge", "refund", "payout", "transfer"}
# Nouns that, with a verb that creates or changes, give access to someone.
ACCESS = {"token", "tokens", "key", "keys", "accesskey", "apikey", "credential", "credentials",
          "invitation", "invitations", "invite", "member", "members", "membership", "collaborator",
          "collaborators", "rolebinding", "rolebindings", "clusterrolebinding", "clusterrolebindings",
          "roleassignment", "roleassignments", "permission", "permissions", "iampolicy", "acl", "acls",
          "sshkey", "deploykey", "webhook", "webhooks", "hook", "hooks", "drain", "drains", "share",
          "public", "publicaccess", "ingress", "grant", "grants", "loginprofile", "password",
          "serviceaccountkey", "sas", "bypass", "firewall", "allowlist", "whitelist", "cors"}
ACCESS_VERBS = {"grant", "invite", "share", "authorize", "allow", "publish", "expose", "impersonate",
                "make", "open"}
CREATE_VERBS = {"create", "add", "put", "set", "update", "patch", "generate", "issue", "attach", "assign",
                "enable", "upsert", "replace", "modify", "insert", "new", "post", "bind"}
# Things whose loss is small: removing one is not held.
SMALL = {"tag", "tags", "label", "labels", "reaction", "reactions", "star", "stars", "starred", "assignee",
         "assignees", "notification", "notifications", "subscription-notification", "annotation",
         "annotations", "draft", "drafts", "emoji", "pin", "pins", "bookmark", "bookmarks", "favorite",
         "favorites", "favourite", "upvote", "thread", "lease", "watch", "watchers", "session", "sessions",
         "cache", "caches", "preference", "preferences", "view", "views", "filter", "filters",
         "tail", "tails", "event", "events", "metadata", "description", "readme", "avatar"}
ZERO_FIELDS = re.compile(r"(^|\.)(replicas|replica_?count|count|desired_?count|desiredcapacity|numreplicas|"
                         r"instances?|min_?size|max_?size|size|capacity|scale|machines)$", re.I)
OFF_FIELDS = re.compile(r"(^|\.)(deletion_?protection(_?enabled)?|termination_?protection|"
                        r"enable_?deletion_?protection|versioning|backup_?retention(_?period)?|"
                        r"point_?in_?time_?recovery(_?enabled)?|soft_?delete(_?enabled)?|"
                        r"purge_?protection|require_?ssl|ssl_?enforcement|enforce_?ssl|mfa(_?delete)?)$", re.I)
PUBLIC_VALUES = {"public", "allusers", "allauthenticatedusers", "0.0.0.0/0", "::/0", "*", "internet",
                 "anonymous", "blob", "container", "everyone"}
PUBLIC_FIELDS = re.compile(r"(^|\.)(visibility|access|publicaccess|public_?access(_?level)?|acl|"
                           r"source_?address_?prefix(es)?|cidr_?ip|cidr(_?block)?|ip_?ranges?|source_?ranges?|"
                           r"member|members|principal|allowed_?ips?|default_?action|public_?network_?access|"
                           r"allow_?blob_?public_?access|is_?public|public|private|type|cidr_?ipv6)$", re.I)


def words(name):
    """Words of an operation name: camelCase, snake_case, kebab-case, dots, colons, slashes."""
    name = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
    name = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", name)
    return [w for w in re.split(r"[^A-Za-z0-9]+", name.lower()) if w]


def looks_like_id(seg):
    """A path segment that names one thing rather than a kind of thing."""
    if re.fullmatch(r"v\d+(alpha\d*|beta\d*)?|\d{4}-\d{2}-\d{2}(-preview)?", seg):
        return False   # an API version
    if re.search(r"\d", seg) or len(seg) > 40 or "@" in seg or "." in seg or "=" in seg:
        return True
    return False


def operation_names(ctx):
    """[(kind, name)] the request carries. Kinds: graphql, aws, rpc (custom verb or JSON-RPC), path."""
    out = []
    reads_only = None
    j = None
    try:
        j = ctx.json()
    except D.Unknown:
        j = None
    # GraphQL: every root field of every mutation in the document
    items = j if isinstance(j, list) else [j]
    for it in items:
        if isinstance(it, dict) and isinstance(it.get("query"), str):
            try:
                from graphql import parse
                doc = parse(it["query"])
            except Exception:
                out.append(("graphql", "?unparsable"))
                continue
            for d in doc.definitions:
                op = getattr(d, "operation", None)
                if op is None:
                    continue
                if op.value != "mutation":
                    reads_only = True if reads_only is None else reads_only
                    continue
                reads_only = False
                for sel in d.selection_set.selections:
                    if hasattr(sel, "name") and sel.name is not None:
                        out.append(("graphql", sel.name.value))
    # JSON-RPC (MCP): the tool or the method
    if isinstance(j, dict) and isinstance(j.get("method"), str) and "jsonrpc" in j:
        if j["method"] == "tools/call" and isinstance(j.get("params"), dict):
            out.append(("rpc", str(j["params"].get("name", ""))))
        else:
            out.append(("rpc", j["method"]))
    # AWS: X-Amz-Target, Action=, Smithy RPC v2 path
    for v in D.header(ctx.req, "x-amz-target"):
        out.append(("aws", v.split(".")[-1].strip()))
    acts = list(ctx.qget("Action"))
    if ctx.method == "POST" and not out:
        try:
            acts += [v for k, v in ctx.form() if k == "Action"]
        except D.Unknown:
            pass
    for a in acts:
        out.append(("aws", a))
    raw = D._clean_path_keep_case(ctx.raw_path)
    if len(raw) == 4 and raw[0].lower() == "service" and raw[2].lower() == "operation":
        out.append(("aws", raw[3]))
    # Google-style custom verb: /v1/projects/p/secrets/s:disable, or a last segment ":batchDelete"
    if raw and ":" in raw[-1]:
        out.append(("rpc", raw[-1].rsplit(":", 1)[1]))
    return out, reads_only, j


def path_nouns(ctx):
    """The path's literal segments, last first, without ids and versions."""
    raw = D._clean_path_keep_case(ctx.raw_path)
    segs = [s.split(":")[0] for s in raw]
    return [s.lower() for s in reversed(segs) if s and not looks_like_id(s)]


def flat(j):
    try:
        return D.flatten_args(j) if isinstance(j, (dict, list)) else {}
    except Exception:
        return {}


def body_markers(ctx, j):
    """(outcome, why) from what the body sets, whatever the operation."""
    fields = flat(j)
    if not fields:
        try:
            fields = dict(ctx.form())
        except D.Unknown:
            fields = {}
    for k, v in fields.items():
        key = k.split(".")[-1]
        sv = str(v).lower() if not isinstance(v, (dict, list)) else None
        if sv is None:
            continue
        if ZERO_FIELDS.search(k) and sv in ("0", "0.0"):
            return "outage", "sets %s to 0" % k
        if OFF_FIELDS.search(k) and sv in ("false", "off", "disabled", "0", "suspended"):
            return "outage", "switches off %s" % k
        if PUBLIC_FIELDS.search(k) and sv in PUBLIC_VALUES:
            if key.lower() == "type" and sv not in ("public",):
                continue
            return "exposed", "sets %s to %s" % (k, v)
        if key.lower() in ("private",) and sv == "false":
            return "exposed", "sets private to false"
        if key.lower() in ("force",) and sv == "true" and ctx.method in ("PATCH", "PUT", "POST"):
            return "recoverable", "sends force=true"
    return None, None


def by_words(ws):
    """(outcome, verb) from the words of one operation name, or (None, None)."""
    if not ws:
        return None, None
    s = set(ws)
    if ws[0] in READ_VERBS or (ws[0] == "batch" and len(ws) > 1 and ws[1] in READ_VERBS):
        return "read", ws[0]
    if s & UNDO:
        return None, None
    hit = [w for w in ws if w in GONE]
    if hit:
        return "gone", hit[0]
    hit = [w for w in ws if w in OUTAGE]
    if hit:
        return "outage", hit[0]
    hit = [w for w in ws if w in MONEY]
    if hit:
        return "money", hit[0]
    if s & ACCESS_VERBS:
        return "exposed", sorted(s & ACCESS_VERBS)[0]
    if s & ACCESS and (s & CREATE_VERBS or len(ws) == 1):
        return "exposed", sorted(s & ACCESS)[0]
    if "public" in s:
        return "exposed", "public"
    return None, None


def small_object(ws, nouns):
    """True when the thing acted on is in SMALL: the last noun of the name, or of the path."""
    cand = [w for w in ws if w not in GONE | OUTAGE | READ_VERBS | CREATE_VERBS | {"batch", "bulk", "all"}]
    if cand:
        return cand[-1] in SMALL or cand[-1].rstrip("s") in SMALL
    return bool(nouns) and (nouns[0] in SMALL)


def classify(req):
    ctx = D.Ctx(dict(req))
    methods = ctx.methods()
    try:
        names, reads_only, j = operation_names(ctx)
    except D.Unknown as e:
        return {"decision": "hold", "outcome": None, "why": "body unreadable: %s" % e, "name": ""}
    nouns = path_nouns(ctx)
    # 1. a name the request carries decides first
    verdicts = []
    for kind, n in names:
        ws = words(n)
        out, verb = by_words(ws)
        if out in (None, "read"):
            verdicts.append((out, n, verb))
            continue
        if out in ("gone", "outage") and small_object(ws, []):
            verdicts.append((None, n, "small"))
            continue
        verdicts.append((out, n, verb))
    held = [v for v in verdicts if v[0] not in (None, "read")]
    mark, mwhy = body_markers(ctx, j) if (methods - READ_METHODS) else (None, None)
    if held:
        o, n, verb = held[0]
        return {"decision": "hold", "outcome": o, "why": "the operation %s says %s" % (n, verb), "name": n}
    if mark:
        return {"decision": "hold", "outcome": mark, "why": "the body %s" % mwhy,
                "name": ",".join(n for _, n in names)}
    if names and all(v[0] == "read" for v in verdicts):
        return {"decision": "allow", "outcome": None, "why": "the operation reads", "name": names[0][1]}
    if reads_only and not names:
        return {"decision": "allow", "outcome": None, "why": "a GraphQL query, no mutation", "name": ""}
    if names and any(k in ("graphql", "aws", "rpc") for k, _ in names):
        return {"decision": "allow", "outcome": None, "why": "the operation's name says nothing destructive",
                "name": ",".join(n for _, n in names)}
    # 2. no name: the method and the path
    if not (methods - READ_METHODS):
        return {"decision": "allow", "outcome": None, "why": "a read method", "name": ""}
    last = nouns[0] if nouns else ""
    ws = words(last)
    if "DELETE" in methods:
        # the last literal word may be the thing's own name (/labels/bug), so look at two
        small = [n for n in nouns[:2] if n in SMALL or n.rstrip("s") in SMALL]
        if small:
            return {"decision": "allow", "outcome": None, "why": "DELETE of a small thing (%s)" % small[0], "name": last}
        return {"decision": "hold", "outcome": "gone", "why": "DELETE %s" % ("/".join(reversed(nouns)) or "/"), "name": last}
    # the path's last literal word as a verb: /machines/m1/stop, /vms/v/powerOff, /keys/regenerate
    out, verb = by_words(ws)
    if out not in (None, "read") and not small_object(ws, nouns[1:]):
        return {"decision": "hold", "outcome": out, "why": "the path ends in %s" % last, "name": last}
    if out == "read":
        return {"decision": "allow", "outcome": None, "why": "the path ends in %s" % last, "name": last}
    # a create or change of an access thing: POST /teams/t/members, PUT .../roleAssignments/x
    for nn in nouns[:2]:
        if nn in ACCESS or nn.rstrip("s") in ACCESS:
            if methods & {"POST", "PUT", "PATCH"}:
                return {"decision": "hold", "outcome": "exposed", "why": "%s on %s" % ("/".join(sorted(methods & {"POST", "PUT", "PATCH"})), nn), "name": nn}
    return {"decision": "allow", "outcome": None, "why": "a write with no sign of loss", "name": last}
