"""Turns the CLI captures in captures/*.jsonl into labelled cases for measure.py.

Labels come from commands.tsv, written and committed before any capture ran. A command's
label goes to the requests that carry its step: every request that is not a read. A read
(GET, HEAD, a GraphQL query with no mutation) is labelled allow, and so is the one auxiliary
mutation the CLIs send around every command, Railway's cliEventTrack telemetry. Nothing else
is relabelled: if a command sent a request that is not its step, it carries the command's
label, and the README says so.

A command that never sent its step is listed in UNREACHED and adds no hold case.
"""
import base64
import glob
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
AUX_GQL = {"cliEventTrack"}
# Commands that stopped before their step, and why. Their non-read requests are not the step.
UNREACHED = {
    "fly-apps-move": "flyctl needs its WireGuard agent before moving an app; only validateWireGuardPeers was sent",
    "fly-mpg-destroy": "flyctl needs a macaroon token for Managed Postgres; nothing was sent",
    "vc-certs-rm": "the Vercel CLI asks for confirmation and has no flag to skip it",
    "rw-volume-add": "the Railway CLI needs a linked service",
}


def commands():
    out = {}
    for line in open(os.path.join(HERE, "commands.tsv")):
        if line.startswith("#") or not line.strip():
            continue
        name, cli, label, cmd = line.rstrip("\n").split("\t")
        out[name] = {"cli": cli, "label": label, "cmd": cmd}
    return out


def gql(e):
    # The capture server parses any {"query": ...} body as GraphQL, Supabase's SQL endpoint too.
    return e.get("graphql") if "graphql" in e["target"] else None


def is_read(e):
    g = gql(e)
    if g:
        return all(x.get("kind") != "mutation" for x in g)
    return e["method"] in ("GET", "HEAD", "OPTIONS")


def is_aux(e):
    g = gql(e) or []
    return bool(g) and all(x.get("firstRootField") in AUX_GQL for x in g)


def body_of(e):
    if e.get("body") is not None:
        return e["body"].encode()
    if "body_b64_head" in e:
        return base64.b64decode(e["body_b64_head"])
    return b""


PROVIDER = {"rw": "railway", "fly": "fly", "vc": "vercel", "wr": "cloudflare", "sb": "supabase"}


def cases():
    cmds = commands()
    out, seen = [], set()
    for f in sorted(glob.glob(os.path.join(HERE, "captures", "*.jsonl"))):
        cur = None
        for line in open(f):
            e = json.loads(line)
            if e.get("event") == "command_start":
                cur = e["name"]
                n = 0
                continue
            if e.get("event") != "request" or cur is None:
                continue
            body = body_of(e)
            c = cmds[cur]
            if is_read(e) or is_aux(e):
                label, why = "allow", "read or telemetry sent by `%s`" % c["cmd"]
                key = (e["method"], e["host"], e["target"], body)
                if key in seen:  # the same read repeats across commands; count it once
                    continue
                seen.add(key)
            elif cur in UNREACHED:
                label, why = "allow", "sent before the step was reached: " + UNREACHED[cur]
            else:
                label, why = c["label"], "`%s`" % c["cmd"]
            n += 1
            headers = [h for h in e.get("headers", []) if h[0].lower() not in ("content-encoding", "transfer-encoding")]
            out.append({"id": "%s #%d %s %s" % (cur, n, e["method"], e["target"][:60]),
                        "provider": PROVIDER[cur.split("-")[0]], "command": cur, "label": label, "why": why,
                        "req": {"host": e["host"], "method": e["method"], "target": e["target"],
                                "headers": headers, "body": body, "facts": {}}})
    return out


def reached():
    """Commands whose label is not allow, and whether their step reached the wire."""
    sent = {c["command"] for c in cases() if not c["why"].startswith("read or")}
    return {n: (n in sent and n not in UNREACHED) for n, c in commands().items() if c["label"] != "allow"}
