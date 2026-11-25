#!/usr/bin/env python3
"""List GitHub GraphQL mutations that delete, remove, revoke, archive, close,
disable, cancel, unlock, merge, transfer or change visibility.

Input: GitHub's public schema, fetched from
  https://docs.github.com/public/fpt/schema.docs.graphql
Usage:
  python3 -I github_graphql_destructive.py <schema.docs.graphql> > github_graphql_destructive.json

No dependencies. It reads only the `type Mutation { ... }` block and, for each
field, the description in triple quotes just above it and the input type name.
"""
import hashlib
import json
import re
import sys

PREFIXES = {
    "delete": "delete",
    "remove": "remove",
    "revoke": "revoke",
    "archive": "archive",
    "close": "close",
    "disable": "disable",
    "cancel": "cancel",
    "abort": "cancel",
    "clear": "clear",
    "unlink": "remove",
    "transfer": "transfer",
    "dismiss": "dismiss",
    "reject": "dismiss",
    "unpublish": "remove",
    "unarchive": None,
    "merge": "merge",
    "regenerate": "revoke",
}
# Mutations whose name does not say it but which can widen access or break things.
WATCH = {
    "updateRepository": "has no visibility field (checked); renames and turns features off",
    "updateEnterpriseMembersCanDeleteRepositoriesSetting": "enterprise policy on repo deletion",
    "updateRefs": "batch ref updates; can delete refs (all-zero afterOid) and force-move them",
    "updateRef": "can force-move a ref (force: true)",
    "updateBranchProtectionRule": "can weaken protection",
    "updateEnterpriseAllowPrivateRepositoryForkingSetting": "enterprise policy",
    "createCommitOnBranch": "writes to a branch; not destructive by itself",
    "updateRepositoryRuleset": "can weaken rulesets",
    "updateIpAllowListEnabledSetting": "can turn off IP allow list",
    "updateOrganizationAllowPrivateRepositoryForkingSetting": "org policy",
    "updateTeamsRepository": "changes team permissions on repos",
    "inviteEnterpriseAdmin": "grants enterprise admin",
    "updateEnterpriseMembersCanChangeRepositoryVisibilitySetting": "enterprise policy",
}


def main(path):
    text = open(path, encoding="utf-8").read()
    m = re.search(r"^type Mutation[^\n{]*\{\n(.*?)^\}", text, re.S | re.M)
    if not m:
        sys.exit("no Mutation type found")
    body = m.group(1)
    # A field is: optional """desc""" then `  name(` ... `): Payload`
    field_re = re.compile(
        r'(?:  """\n(?P<desc>.*?)\n  """\n)?  (?P<name>[a-zA-Z0-9_]+)\(\n(?P<args>.*?)\n  \): (?P<ret>[A-Za-z0-9_]+)(?P<dirs>[^\n]*)',
        re.S,
    )
    all_names = []
    out = []
    for f in field_re.finditer(body):
        name = f.group("name")
        all_names.append(name)
        desc = " ".join(l.strip() for l in (f.group("desc") or "").splitlines()).strip()
        inp = re.search(r"input: ([A-Za-z0-9_]+)!", f.group("args"))
        lower = name.lower()
        kind = None
        for p, k in PREFIXES.items():
            if lower.startswith(p):
                kind = k
                break
        reason = WATCH.get(name)
        if kind is None and reason is None:
            continue
        preview = "@preview" in f.group("args") or "@preview" in (f.group("desc") or "")
        out.append({
            "mutation": name,
            "kind": kind or "watch",
            "description": desc,
            "input": inp.group(1) if inp else None,
            "category": (re.search(r'@docsCategory\(name: "([^"]+)"', f.group("dirs")) or [None, None])[1],
            "note": reason,
        })
    # Input fields that matter for a few watched mutations.
    def input_fields(tname):
        mm = re.search(r"^input %s[^\n{]*\{\n(.*?)^\}" % re.escape(tname), text, re.S | re.M)
        if not mm:
            return []
        return re.findall(r"^  ([a-zA-Z0-9_]+): ", mm.group(1), re.M)

    for e in out:
        if e["kind"] == "watch" and e["input"]:
            e["input_fields"] = input_fields(e["input"])
    result = {
        "source": "https://docs.github.com/public/fpt/schema.docs.graphql",
        "read": "2026-11-24",
        "sha256": hashlib.sha256(text.encode()).hexdigest(),
        "has_deleteRepository": "deleteRepository" in all_names,
        "total_mutations": len(all_names),
        "selected": len(out),
        "by_kind": {},
        "mutations": sorted(out, key=lambda e: (e["kind"], e["mutation"])),
    }
    for e in out:
        result["by_kind"][e["kind"]] = result["by_kind"].get(e["kind"], 0) + 1
    json.dump(result, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv[1])
