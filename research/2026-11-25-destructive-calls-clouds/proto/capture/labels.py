"""Turns the CLI captures in captures/*.jsonl into labelled cases for measure.py. As #383's labels.py.

Labels come from commands.tsv, committed before any capture ran. A command's label goes to every
request it sent that is not a read. Reads are labelled allow: GET, HEAD, OPTIONS, a GraphQL request
with no mutation, an AWS Describe/List/Get action, Google's getIamPolicy and testIamPermissions, and
a poll of a Google operation (Compute's POST .../operations/<id>/wait; added after the first score,
which counted two such polls as the command's step).
Nothing else is relabelled: a request a command sent that is not its step (gcloud's rollback DELETE
after a failed upload check, a retry after Key Vault's 401 challenge) carries the command's label,
and the README says so.

When a command was run more than once while the mocks were fixed, only its last run counts.
Requests sent outside any command (az login --identity) are left out.

#353's server redacts every credential header, so the AWS Authorization header is rebuilt from the
credential scope the server logged (key "TESTKEY", signature "<redacted>"): the detector reads the
service from it, as it would from the real header.
"""
import base64
import glob
import json
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
UNREACHED = {
    "k-apply-zero": "kubectl validates the manifest against the server's OpenAPI document and gave up on the mock's; nothing was sent",
    "gh-project-delete": "gh could not tell from the mock whether acme is a user or an organization; nothing was sent",
}
READ_ACTION = re.compile(r"^(Describe|List|Get)")


def commands():
    out = {}
    for line in open(os.path.join(HERE, "commands.tsv")):
        if line.startswith("#") or not line.strip():
            continue
        name, cli, label, cmd = line.rstrip("\n").split("\t")
        out[name] = {"cli": cli, "label": label, "cmd": cmd}
    return out


def is_read(e):
    g = e.get("graphql")
    if g and e["target"].startswith("/graphql"):
        return all(x.get("kind") != "mutation" for x in g)
    if e["method"] in ("GET", "HEAD", "OPTIONS"):
        return True
    a = e.get("aws") or {}
    op = (a.get("X-Amz-Target") or "").split(".")[-1] or a.get("Action") or ""
    if READ_ACTION.match(op):
        return True
    path = e["target"].split("?")[0]
    # Google's long-running operations: gcloud polls them, with POST .../operations/<id>/wait for Compute
    return path.endswith((":getIamPolicy", ":testIamPermissions")) or "/operations/" in path


def body_of(e):
    if e.get("body") is not None:
        return e["body"].encode()
    if "body_b64_head" in e:
        return base64.b64decode(e["body_b64_head"])
    return b""


def headers_of(e):
    hs = [list(h) for h in e["headers"]]
    a = e.get("aws") or {}
    if a.get("sigv4_service"):
        hs = [h for h in hs if h[0].lower() != "authorization"]
        hs.append(["Authorization", "AWS4-HMAC-SHA256 Credential=TESTKEY/20261125/%s/%s/aws4_request, SignedHeaders=host, Signature=<redacted>"
                   % (a["sigv4_region"], a["sigv4_service"])])
    return hs


def runs(path):
    """{command name: [request events of its last run]}"""
    out, cur = {}, None
    for line in open(path):
        e = json.loads(line)
        ev = e.get("event")
        if ev == "command_start":
            cur = e["name"]
            out[cur] = []
        elif ev == "command_end":
            cur = None
        elif ev == "request" and cur:
            out[cur].append(e)
    return out


def cases():
    cmds = commands()
    out = []
    for path in sorted(glob.glob(os.path.join(HERE, "captures", "*.jsonl"))):
        for name, reqs in runs(path).items():
            c = cmds[name]
            for i, e in enumerate(reqs):
                read = is_read(e)
                label = "allow" if read else c["label"]
                out.append({"id": "%s #%d" % (name, i + 1), "provider": c["cli"], "command": name, "label": label,
                            "step": not read, "why": "a read" if read else c["cmd"],
                            "req": {"host": e["host"], "method": e["method"], "target": e["target"],
                                    "headers": headers_of(e), "body": body_of(e), "facts": {}}})
    return out


def reached():
    """Commands with at least one request that is not a read."""
    return sorted({c["command"] for c in cases() if c["step"]})


if __name__ == "__main__":
    import collections
    cs = cases()
    print(len(cs), "requests;", collections.Counter((c["provider"], c["label"].split(":")[0]) for c in cs))
    cmds = commands()
    r = set(reached())
    print("commands that sent no write:", sorted(n for n in cmds if n not in r))
