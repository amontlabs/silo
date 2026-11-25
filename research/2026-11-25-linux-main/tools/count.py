#!/usr/bin/env python3
"""Counts the open decisions behind option A and option B of the first build, by following
`needs` through a plan tree, as #373 did. Run it on #348's tree, which holds #348's new entries:

  git worktree add ../t348 <#348's head> && python3 count.py ../t348

A: #348's 17 features without `a-path-from-a-checkpoint` and `files-imported-from-your-mac`, plus
`model-traffic-read`, `model-calls-in-the-record`, `spend-per-conversation` and a first receipt
(a stand-in that needs the receipt decisions of `receipts-of-work`). Cuts, as #373 proposes:
`model-traffic-read` without `secrets-held-outside`, `logins` without `fork-and-sign-in`.

B: the same, with `computers-main-on-a-mac` replaced everywhere by a Linux main (a stand-in that
needs what the macOS main needs apart from `first-run-time-macos` and `macos-vm-driver`, what
`computers-linux-beside-main` needs apart from the macOS main and the recipe, and
`main-on-linux`), plus `processes-in-the-record` without its macOS needs.
"""
import json, glob, os, sys

root = sys.argv[1]
E = {}
for f in glob.glob(root + '/plan/**/*.json', recursive=True):
    if '/decisions/' in f or '/features/' in f:
        k = os.path.basename(f)[:-5]
        E[k] = json.load(open(f))
        E[k]['_kind'] = 'd' if '/decisions/' in f else 'f'

F348 = ["access-model-checked", "engine-program-and-clients", "engine-matches-wanted",
        "computers-main-on-a-mac", "engine-outlived-by-computers", "agent-programs-run-unmodified",
        "agent-programs-reach-every-computer", "computers-stopped-and-paused",
        "checkpoints-taken-on-their-own", "thinning-old-checkpoints", "restore-to-a-checkpoint",
        "record-of-a-computer", "file-changes-in-the-record", "files-imported-from-your-mac",
        "silo-command", "agents-work-through-the-night", "a-path-from-a-checkpoint"]
A = [f for f in F348 if f not in ("a-path-from-a-checkpoint", "files-imported-from-your-mac")]
A += ["model-traffic-read", "model-calls-in-the-record", "spend-per-conversation", "RECEIPT"]
CUT_A = {"model-traffic-read": {"secrets-held-outside"}, "logins": {"fork-and-sign-in"}}
E["RECEIPT"] = {"_kind": "f", "needs": [n for n in E["receipts-of-work"]["needs"] if n in (
    "what-a-receipt-is", "what-a-receipt-holds", "agent-summary-beside-receipt",
    "conversation-text-kept", "record-of-a-computer", "spend-per-conversation",
    "model-calls-in-the-record")]}

def closure(features, cut):
    seen, todo = set(), list(features)
    while todo:
        x = todo.pop()
        if x in seen:
            continue
        seen.add(x)
        todo += [n for n in E[x].get("needs", []) if n not in cut.get(x, ())]
    return seen

def report(name, s):
    d = sorted(x for x in s if E[x]['_kind'] == 'd' and 'verdict' not in E[x])
    f = sorted(x for x in s if E[x]['_kind'] == 'f')
    print(f"{name}: {len(f)} features, {len(d)} open decisions")
    return set(d)

a = report("A", closure(A, CUT_A))

mac = E["computers-main-on-a-mac"]["needs"]
beside = E["computers-linux-beside-main"]["needs"]
E["LINUXMAIN"] = {"_kind": "f", "needs": sorted(set(
    [n for n in mac if n not in ("first-run-time-macos", "macos-vm-driver")] +
    [n for n in beside if n not in ("computers-main-on-a-mac", "linux-beside-a-macos-main",
                                    "main-on-macos-first", "made-from-a-recipe")] +
    ["main-on-linux"]))}
for k, v in E.items():
    if k != "LINUXMAIN" and "needs" in v:
        v["needs"] = ["LINUXMAIN" if n == "computers-main-on-a-mac" else n for n in v["needs"]]
B = ["LINUXMAIN" if f == "computers-main-on-a-mac" else f for f in A] + ["processes-in-the-record"]
CUT_B = dict(CUT_A, **{"processes-in-the-record": {"processes-from-inside-macos", "macos-ships-facts-first"}})
b = report("B", closure(B, CUT_B))
print("only in A:", ", ".join(sorted(a - b)))
print("only in B:", ", ".join(sorted(b - a)))
