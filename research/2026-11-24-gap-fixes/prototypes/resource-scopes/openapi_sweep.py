"""Sends one made-up request for every operation of GitHub's REST description through classify.py,
and reports how a token limited to victim/app would be decided.

    python3 openapi_sweep.py

Every operation is tried twice: once on the named repository (or the account, for operations
outside repositories) and once on attacker/inbox. The body is {} unless the operation needs one
to be read, which this sweep does not try to guess, so "body" operations show their worst case.
"""

import collections
import re

from classify import REST, Request, Scope, classify, decide

SCOPE = Scope(repos={"victim/app"})
VALUES = {"owner": "victim", "repo": "app", "template_owner": "victim", "template_repo": "app",
          "path": "src/a.py", "ref": "heads/main", "branch": "main", "org": "acme", "username": "someone"}


def concrete(path, owner="victim", repo="app"):
    def sub(m):
        name = m.group(1)
        if name in ("owner", "template_owner"):
            return owner
        if name in ("repo", "template_repo"):
            return repo
        return VALUES.get(name, "1" if name.endswith(("_id", "number")) else "x")
    return re.sub(r"\{([^}]+)\}", sub, path)


effects, buckets, decisions, mismatched = collections.Counter(), collections.Counter(), collections.Counter(), []
outside = collections.Counter()
for op in REST:
    host = "uploads.github.com" if op["op"] == "repos/upload-release-asset" else "api.github.com"
    r = Request(op["method"], host, concrete(op["path"]), [("Content-Type", "application/json")], b"{}")
    v = classify(r)
    if not v.why.startswith(f"REST {op['op']}"):
        mismatched.append((op["method"], op["path"], v.why[:100]))
    effects[v.effect] += 1
    buckets[v.bucket] += 1
    d, _ = decide(v, SCOPE)
    decisions[(v.bucket, d)] += 1
    if op["scope"] == "repo":
        r2 = Request(op["method"], host, concrete(op["path"], "attacker", "inbox"), [("Content-Type", "application/json")], b"{}")
        outside[decide(classify(r2), SCOPE)[0]] += 1

print(f"{len(REST)} operations")
print("effects:", dict(effects.most_common()))
print("classes:", dict(buckets.most_common()))
print("decided for a token limited to victim/app, on victim/app or outside any repository:")
for (b, d), n in sorted(decisions.items()):
    print(f"   {b:20} {d:5} {n}")
print("the same repository operations on attacker/inbox:", dict(outside))
print(f"requests not read as the operation they were made from: {len(mismatched)}")
for m in mismatched:
    print("   ", *m)
