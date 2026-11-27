"""Which requests to a seen host carry data out, to whose place, under whose credential.

A request is read as the boundary would read it after ending the encrypted connection: method,
host from the handshake, target, headers, body. For each request it gives:

- kind: "upload" (puts data on the host), "read", or "unknown-write" (a write it cannot place);
- place: where the data lands, as "github:<owner>", "gist:<owner or ?>", "gcs:<bucket>";
- credential: what the request carries and, where the host can tell, whose it is.

Then it decides, for a computer whose accounts are given, under two ways of tying uploads to them:

- "place": an upload is allowed only to a place in your accounts; an upload it cannot place is held.
- "place+credential": the same, and the credential must be one of Silo's placeholders or one the
  host says is yours (`whose()`), else the upload is refused.

Every input is a byte the boundary read, or the host's own answer about its own objects, as
`registry-traffic-read` takes a package's age from the registry. No opinion is involved.
"""

import base64
import json
import re
from urllib.parse import parse_qs, unquote, urlsplit

GITHUB = {"github.com", "api.github.com", "uploads.github.com", "gist.github.com"}
GCS_JSON = {"storage.googleapis.com", "www.googleapis.com"}


def hdr(rec, name):
    for k, v in rec["headers"]:
        if k.lower() == name:
            return v
    return None


def body(rec):
    return base64.b64decode(rec["body_b64"]) if rec.get("body_b64") else b""


def credential(rec, tokens_seen):
    """(form, value) of the credential a request carries, or ("none", None)."""
    auth = hdr(rec, "authorization")
    q = parse_qs(urlsplit(rec["target"]).query)
    if auth:
        scheme, _, val = auth.partition(" ")
        s = scheme.lower()
        if s == "basic":
            user, _, pw = base64.b64decode(val).decode("latin-1").partition(":")
            # A fixed user name such as x-access-token is not a credential; a token sent as the
            # user name is one (as #350's foreign_credentials() reads it).
            return "basic", pw or user
        if s in ("bearer", "token"):
            if rec["host"] == "ghcr.io" and val in tokens_seen:
                return "registry-token", tokens_seen[val]  # traded for this credential at /token
            return s, val
        return "authorization", val
    for k in ("access_token", "token"):
        if k in q:
            return "query-" + k, q[k][0]
    if "X-Goog-Credential" in q:
        return "signed-url", q["X-Goog-Credential"][0].split("/")[0]
    if "key" in q:
        return "api-key", q["key"][0]  # identifies a Google Cloud project, not an account
    if hdr(rec, "cookie"):
        return "cookie", hdr(rec, "cookie")
    return "none", None


def graphql_place(b):
    try:
        doc = json.loads(b or b"{}")
    except ValueError:
        return "unknown-write", None
    q = doc.get("query", "")
    v = doc.get("variables") or {}
    if not q.lstrip().startswith("mutation"):
        return "read", None
    inp = v.get("input") or {}
    owner = v.get("owner") or inp.get("owner")
    if owner:
        return "upload", f"github:{owner.lower()}"
    # A mutation that names its target by a node id needs the host's answer (#350's lookup).
    for k in ("repositoryId", "subjectId", "issueId", "pullRequestId", "ownerId"):
        if k in inp:
            return "upload", "node:" + inp[k]
    return "upload", "github:?node"


BUCKET = re.compile(r"[a-z0-9][a-z0-9._-]{1,220}[a-z0-9]")


def classify(rec):
    kind, place = _classify(rec)
    if place and place.startswith("gcs:") and not BUCKET.fullmatch(place[4:]):
        return "unknown-write", None  # not a bucket name Google would accept as it reads
    return kind, place


def _classify(rec):
    """(kind, place). place may contain "{cred}" when the host files the data under the credential's account."""
    host, m = rec["host"], rec["method"]
    raw = urlsplit(rec["target"]).path
    q0 = parse_qs(urlsplit(rec["target"]).query)
    overrides = [h for h in ("x-http-method-override", "x-http-method", "x-method-override") if hdr(rec, h)]
    if overrides or "_method" in q0:
        return "unknown-write", None  # a GET the host may run as a POST
    checked = raw
    if host in GCS_JSON and "/o/" in raw:
        checked = raw.split("/o/", 1)[0]  # GCS object names carry %2F by design; the bucket comes first
    if m not in ("GET", "HEAD", "OPTIONS") and (
            re.search(r"%2f|%2e|%5c|\\\\", checked, re.I) or "//" in checked
            or any(seg in (".", "..") for seg in checked.split("/"))):
        return "unknown-write", None  # a path the host may read differently from the rule
    if raw.startswith("/resumable/upload/"):
        raw = raw[len("/resumable"):]
    path = unquote(raw)
    parts = [unquote(p) for p in raw.split("/") if p]  # split first: an encoded slash stays in its segment
    q = parse_qs(urlsplit(rec["target"]).query)
    if m in ("GET", "HEAD", "OPTIONS"):
        return "read", None

    if host in ("github.com", "gist.github.com"):
        if path.endswith("/git-upload-pack"):
            return "read", None
        if path.endswith("/git-receive-pack") or path.endswith("/info/lfs/objects/batch"):
            if path.endswith("/batch") and b'"upload"' not in body(rec):
                return "read", None
            if host == "gist.github.com":
                return "upload", "gist:{cred}"
            return "upload", f"github:{parts[0].lower()}"
        return "unknown-write", None
    if host == "api.github.com":
        if path == "/graphql":
            return graphql_place(body(rec))
        if parts[:1] == ["repos"] and len(parts) >= 3:
            return "upload", f"github:{parts[1].lower()}"
        if parts[:1] == ["gists"]:
            return "upload", "gist:{cred}"
        if parts[:2] == ["user", "repos"]:
            return "upload", "github:{cred}"
        if parts[:1] == ["orgs"] and len(parts) >= 2:
            return "upload", f"github:{parts[1].lower()}"
        return "unknown-write", None
    if host == "uploads.github.com":
        if parts[:1] == ["repos"] and len(parts) >= 3:
            return "upload", f"github:{parts[1].lower()}"
        return "unknown-write", None
    if host == "ghcr.io":
        if path == "/token":
            return "read", None  # trades a credential for a registry token; puts nothing on the host
        if parts[:1] == ["v2"] and len(parts) >= 4:
            return "upload", f"github:{parts[1].lower()}"  # a GHCR namespace is a GitHub account
        return "unknown-write", None

    if host in GCS_JSON and parts[:3] in (["upload", "storage", "v1"],) and len(parts) >= 5:
        return "upload", f"gcs:{parts[4]}"
    if host in GCS_JSON and parts[:2] == ["storage", "v1"] and len(parts) >= 4 and parts[2] == "b":
        # copy and rewrite name the destination bucket after copyTo/rewriteTo
        for i, p in enumerate(parts):
            if p in ("copyTo", "rewriteTo") and len(parts) > i + 2:
                return "upload", f"gcs:{parts[i + 2]}"
        return "upload", f"gcs:{parts[3]}"
    if host == "storage.googleapis.com" and parts:
        return "upload", f"gcs:{parts[0]}"  # XML API, path style
    if host.endswith(".storage.googleapis.com"):
        return "upload", f"gcs:{host[:-len('.storage.googleapis.com')]}"  # XML API, host style
    return "unknown-write", None


class Accounts:
    """What the user said is theirs, and Silo's own placeholders."""

    def __init__(self, github=(), buckets=(), identities=(), placeholders=()):
        self.github = {g.lower() for g in github}
        self.buckets = set(buckets)
        self.identities = {i.lower() for i in identities}
        self.placeholders = set(placeholders)

    def owns(self, place):
        kind, _, name = place.partition(":")
        if kind in ("github", "gist"):
            return name in self.github
        if kind == "gcs":
            return name in self.buckets
        return False


def registry_tokens(records, whose):
    """GHCR trades a credential for an opaque token at /token, and the boundary sees both sides:
    {opaque token: whose credential was traded for it}."""
    out = {}
    for r in records:
        if r["host"] == "ghcr.io" and urlsplit(r["target"]).path == "/token" and r.get("answer"):
            form, val = credential(r, {})
            if form == "none" and r.get("body_b64"):  # docker's OAuth form: password=...
                form_body = parse_qs(body(r).decode("latin-1"))
                val = (form_body.get("password") or form_body.get("refresh_token") or [None])[0]
                form = "basic" if val else "none"
            try:
                tok = json.loads(r["answer"]).get("token")
            except ValueError:
                continue
            out[tok] = whose(form, val, "ghcr.io") if val else None
    return out


def decide(records, accounts, whose, node_owner=lambda node_id: None):
    """Yields (record, kind, place, credential form, who, decision by place, decision by place+credential).

    whose(form, value, host) is the host's answer to "whose credential is this", or None when the
    host cannot tell. The boundary would ask it once per credential and keep the answer in the record.
    """
    records = list(records)
    tokens_seen = registry_tokens(records, whose)
    for rec in records:
        form, val = credential(rec, tokens_seen)
        if form == "registry-token":
            who = val
        elif val is not None and val in accounts.placeholders:
            who = "silo"
        elif val is not None:
            who = whose(form, val, rec["host"])
        else:
            who = None
        kind, place = classify(rec)
        if place and place.startswith("node:"):
            owner = node_owner(place[5:])  # GitHub's answer to nodes(ids:), asked by the boundary
            place = f"github:{owner.lower()}" if owner else "github:?node"
        if place and "{cred}" in place:
            account = "silo" if who == "silo" else (who or "?").split("@")[0].lower()
            place = place.replace("{cred}", account)
        if kind == "read":
            yield rec, kind, place, form, who, "allow", "allow"
            continue
        if kind == "unknown-write":
            yield rec, kind, place, form, who, "hold", "hold"
            continue
        if place.endswith(":?") or place.endswith(":?node"):
            by_place = "hold"
        elif place.endswith(":silo"):
            by_place = "allow"  # filed under the account whose secret Silo holds
        else:
            by_place = "allow" if accounts.owns(place) else "refuse"
        mine = who == "silo" or (who is not None and (who.lower() in accounts.identities
                                                       or who.lower() in accounts.github))
        if by_place == "refuse":
            by_both = "refuse"
        elif form in ("none", "api-key"):
            by_both = by_place  # nothing to tie: the host takes it only if the place lets anyone write
        else:
            by_both = by_place if mine else "refuse"
        yield rec, kind, place, form, who, by_place, by_both
