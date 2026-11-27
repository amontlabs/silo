"""Classifies every request recorded by run_corpus.sh and decides it for a GitHub token limited to
victim/app.

    python3 classify_corpus.py OUT_DIR

The boundary's read of what GitHub already holds (tree_of) is played by the bare repositories the
fake GitHub served, in OUT_DIR/state/git. Each time it is used is counted: in Silo it is one
request to the host's API, made by the boundary with the real token.
"""

import base64
import json
import subprocess
import sys

from classify import Request, Scope, classify, decide, foreign_credentials

OUT = sys.argv[1]
LOOKUPS = []


def tree_of(place, commit):
    repo = place.split(":", 1)[1]
    bare = f"{OUT}/state/git/{repo}.git"
    LOOKUPS.append((place, commit[:10]))
    if commit == "default-branch":
        commit = "main"
    gh = subprocess.run(["git", "-C", bare, "rev-parse", "-q", "--verify", f"{commit}:.github"],
                        capture_output=True, text=True)
    if gh.returncode != 0:
        return ("", {})
    out = {}
    for d in ("workflows", "actions"):
        r = subprocess.run(["git", "-C", bare, "rev-parse", "-q", "--verify", f"{commit}:.github/{d}"],
                           capture_output=True, text=True)
        if r.returncode == 0:
            out[d] = r.stdout.strip()
    return (gh.stdout.strip(), out)


SCOPES = {  # one secret per host, each limited to what the victim owns
    "github": Scope(repos={"victim/app"}, repo_nodes={"R_kgDOAAAAAg": "victim/app"}),
    "registry.npmjs.org": Scope(packages={"npm/victim-pkg"}, reads="anywhere"),
    "upload.pypi.org": Scope(packages={"pypi/victim-pkg"}, reads="anywhere"),
    "s3": Scope(buckets={"victim-builds"}, reads="anywhere"),
}


def scope_for(host):
    if host.endswith("amazonaws.com"):
        return SCOPES["s3"]
    return SCOPES.get(host, SCOPES["github"])
recs = [json.loads(l) for l in open(f"{OUT}/corpus.jsonl")]
steps = [json.loads(l) for l in open(f"{OUT}/commands.jsonl")]
placeholders = ["FAKEplaceholder"]
tally = {}
for st in steps:
    for r in recs[st["first"]:st["last"]]:
        req = Request(r["method"], r["host"], r["target"], [tuple(h) for h in r["headers"]],
                      base64.b64decode(r["body_b64"] or ""))
        v = classify(req, tree_of=tree_of)
        d, why = decide(v, scope_for(r["host"]))
        # S3 requests are signed inside the computer, so their Authorization is never a placeholder:
        # see the doc. The check is shown for the hosts that take a bearer token.
        foreign = [] if r["host"].endswith("amazonaws.com") else foreign_credentials(req, placeholders)
        tally[d] = tally.get(d, 0) + 1
        print(f"{st['step']:26} {r['method']:6} {r['host']:24} {r['target'][:44]:44} -> {v.effect:7} "
              f"{d:5} {'FOREIGN ' + ','.join(foreign) if foreign else ''}| {why[:110]}")
print("decisions:", tally)
print("tree lookups made by the boundary:", len(LOOKUPS), LOOKUPS)
