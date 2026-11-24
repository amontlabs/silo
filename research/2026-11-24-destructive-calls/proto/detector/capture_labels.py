"""Turns the real CLI captures in ../capture/captures/*.jsonl into labelled corpus cases.

The labels here are written by hand from what each CLI command does, not from the detector:
a capture is "hold" when it is a DELETE on a control-plane API (Heroku dyno restarts excepted),
a GraphQL mutation that removes something, a git push that deletes a ref, an AWS Delete*, or a
Stripe money call. Credentials in captures are already redacted by the capture server.
"""
import base64
import glob
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
CAPS = os.path.join(HERE, "..", "capture", "captures")

REMOVING_FIELDS = {"volumeDelete", "environmentDelete", "deploymentRemove", "serviceDelete", "projectDelete"}


def label(e):
    m, host, target = e["method"], e["host"], e["target"]
    if host == "github.com" and target.endswith("git-receive-pack") and m == "POST":
        for line in e.get("git_pktlines", []):
            if isinstance(line, str) and len(line) > 82 and line[41:81] == "0" * 40:
                return "hold", "git push deleting a ref"
        return "allow", "git push"
    if e.get("graphql"):
        for g in e["graphql"]:
            if g.get("kind") == "mutation" and g.get("firstRootField") in REMOVING_FIELDS:
                return "hold", "mutation " + g["firstRootField"]
        return "allow", "graphql " + ",".join(str(g.get("firstRootField")) for g in e["graphql"])
    if host == "api.heroku.com" and m == "DELETE" and "/dynos" in target:
        return "allow", "dyno restart"
    if m == "DELETE":
        return "hold", "DELETE from a CLI delete command"
    aws = e.get("aws") or {}
    op = aws.get("Action") or (aws.get("X-Amz-Target") or "").split(".")[-1]
    if op.startswith("Delete"):
        return "hold", "AWS " + op
    if host == "api.stripe.com" and m == "POST" and target.startswith("/v1/refunds"):
        return "hold", "Stripe refund (key mode unknown in the capture)"
    return "allow", "%s from a CLI read" % m


def body_of(e):
    if e.get("git_pktlines"):
        out = b""
        for line in e["git_pktlines"]:
            if isinstance(line, str) and not line.startswith("0000"):
                b = line.replace("\\0", "\0").encode()
                out += b"%04x" % (len(b) + 4) + b
        return out + b"0000"
    if "body" in e and e["body"] is not None:
        return e["body"].encode()
    if "body_b64_head" in e:
        return base64.b64decode(e["body_b64_head"])
    return b""


def cases():
    out = []
    seen = set()
    for f in sorted(glob.glob(os.path.join(CAPS, "*.jsonl"))):
        cli = os.path.basename(f)[:-6]
        for line in open(f):
            try:
                e = json.loads(line)
            except ValueError:
                continue
            if e.get("event") != "request":
                continue
            body = body_of(e)
            key = (e["method"], e["host"], e["target"], body)
            if key in seen:
                continue
            seen.add(key)
            lab, note = label(e)
            headers = [h for h in e.get("headers", []) if h[0].lower() not in ("content-encoding", "transfer-encoding")]
            facts = {}
            if e.get("body_len") and len(body) < e["body_len"] and "body" not in e and not e.get("git_pktlines"):
                facts["body_complete"] = False
            out.append({"id": "capture-%s-%03d" % (cli, len(out) + 1), "family": "capture-" + cli, "kind": "capture",
                        "label": lab, "note": note,
                        "req": {"host": e["host"], "method": e["method"], "target": e["target"], "headers": headers,
                                "body_b64": base64.b64encode(body).decode(), "facts": facts}})
    return out


if __name__ == "__main__":
    cs = cases()
    print(len(cs), "distinct captured requests")
