"""Settles each disagreement between #383's labels (A) and the blind second labelling (B).

Each disagreement gets one label under the wording of `outcome-of-a-call` in force ("exposed" is
"readable by anyone"). A rule per kind of disagreement decides most; OVERRIDES name the cases
where the rule's side is wrong, each with its reason. Where both agree the label stands.
Writes settled.jsonl (wording in force) and settled-proposed.jsonl (#383's proposed wording:
an `allow` that B marked as giving access to someone named becomes hold:exposed).
Run after compare.py has written results/disagreements.jsonl, then run compare.py again.
"""
import json, os

HERE = os.path.dirname(os.path.abspath(__file__))

# (A, B) -> (side, reason). side "a" or "b" or a label.
BY_KIND = {
    ("hold:exposed", "allow"): ("b", "access given to someone named, not readable by anyone: allow under the wording in force (see settled-proposed)"),
    ("hold:exposed", "either"): ("b", "an update that can widen or narrow; the body decides, and no variant was given"),
    ("either", "allow"): ("b", "removes, revokes, restarts, mints for the caller or sets a harmless value; nothing to hold"),
    ("allow", "deploy"): ("b", "runs code or configuration"),
    ("deploy", "allow"): ("b", "a version, build record or setting that is not live until a separate deploy"),
    ("deploy", "either"): ("b", "repoints what runs; the body decides"),
    ("either", "deploy"): ("b", "ships configuration to what runs (secrets, bindings, flags, a firewall version)"),
    ("either", "hold:exposed"): ("b", "opens to anyone with the URL or link"),
    ("allow", "hold:exposed"): ("b", "gives a private service a public address"),
    ("either", "hold:gone"): ("b", "deletes something the API cannot bring back"),
    ("either", "hold:money"): ("b", "a paid resource or a size that cannot shrink"),
    ("allow", "hold:money"): ("b", "billed resource or charges money"),
    ("either", "hold:outage"): ("b", "what calls it stops working until someone acts"),
    ("allow", "hold:outage"): ("b", "a public address stops serving"),
    ("allow", "either"): ("b", "a setting that can block clients, raise cost or widen access; value decides"),
    ("hold:gone", "either"): ("b", "replaces a whole section; harmless or total, the body decides, and no variant was given"),
    ("hold:gone", "hold:outage"): ("b", "what is removed is a runner; the code or data lives elsewhere"),
    ("hold:gone", "hold:recoverable"): ("b", "the same API has an endpoint that brings it back"),
    ("hold:money", "either"): ("b", "a limit that can be raised or lowered"),
    ("hold:outage", "allow"): ("b", "a member removed, a restart or a cache dropped: recovers by itself or is redone in a moment"),
    ("hold:outage", "either"): ("b", "an update; the value decides whether anything breaks"),
    ("hold:outage", "hold:gone"): ("b", "the deleted record or key holds a value the API cannot give back"),
    ("hold:recoverable", "hold:gone"): ("a", "the provider documents a restore window (#353 paas.md)"),
    ("hold:gone", "deploy"): ("b", "ships migrations or a whole configuration to production"),
}

OVERRIDES = {  # substring of id -> (label, reason)
    "railway twoFactorInfoDelete": ("hold:exposed", "a protection turned off: held under what it opens the way to"),
    "railway passkeyDelete": ("hold:exposed", "a protection turned off"),
    "railway emailChangeInitiate": ("hold:exposed", "starts moving the account to another address"),
    "railway emailChangeConfirm": ("hold:exposed", "the account moves to another address"),
    "cloudflare DELETE /client/v4/zones/{zone_id}/rulesets/{ruleset_id}|": ("hold:exposed", "removes WAF rules: a protection turned off"),
    "/rulesets/{ruleset_id}/rules/": ("either", "one rule of a ruleset; a WAF rule or a harmless one, the request does not say"),
    "readonly/temporary-disable": ("either", "lifts the read-only mode Supabase sets on a full disk"),
    "fly POST /v1/apps/{app}/secrets #2": ("either", "setting an existing name overwrites its value"),
    "fly-gql setSecrets #2": ("either", "setting an existing name overwrites its value"),
    "workers/workers/{worker_id}/preview": ("allow", "a preview, not production"),
    "railway environmentStageChanges #1": ("hold:gone", "the commit that follows does not say what was staged, so the stage is the last place the delete is visible"),
    "supabase DELETE /v1/projects/{ref}/functions": ("hold:gone", "the function's source is not always kept elsewhere"),
    "rulesets/{ruleset_id}/versions": ("allow", "an old version of a ruleset, history only"),
    "cloudflare PATCH /client/v4/zones/{zone_id} #1": ("hold:outage", "paused zone: the site is served without Cloudflare; nothing becomes readable"),
}


def main():
    dis = [json.loads(l) for l in open(os.path.join(HERE, "results", "disagreements.jsonl"))]
    out, prop = [], []
    for d in dis:
        label = reason = by = None
        for k, (lab, why) in OVERRIDES.items():
            key = d["id"] + "|"
            if k in key:
                label, reason, by = lab, why, "override"
                break
        if label is None:
            side, reason = BY_KIND[(d["a"], d["b"])]
            label = d[side] if side in ("a", "b") else side
            by = side
        out.append({"id": d["id"], "label": label, "a": d["a"], "b": d["b"], "by": by, "why": reason})
    keep = {d["id"] for d in dis}
    # agreements: the shared label
    import sys
    sys.path.insert(0, HERE)
    import compare
    A = {c["id"]: c["label"] for c in compare.surface.cases()}
    Brec = compare.load_jsonl(sorted(__import__("glob").glob(os.path.join(HERE, "labels-b", "*.jsonl"))))
    full = {i: {"id": i, "label": A[i], "a": A[i], "b": A[i], "by": "agree"} for i in A if i not in keep}
    for r in out:
        full[r["id"]] = r
    with open(os.path.join(HERE, "settled.jsonl"), "w") as f:
        for i in sorted(full):
            f.write(json.dumps(full[i]) + "\n")
    with open(os.path.join(HERE, "settled-proposed.jsonl"), "w") as f:
        n = 0
        for i in sorted(full):
            r = dict(full[i])
            if r["label"] == "allow" and Brec[i].get("named"):
                r["label"], r["why"], n = "hold:exposed", "access given to someone named", n + 1
            f.write(json.dumps(r) + "\n")
    import collections
    print(len(out), collections.Counter(r["by"] for r in out), "named upgrades:", n)


if __name__ == "__main__":
    main()
