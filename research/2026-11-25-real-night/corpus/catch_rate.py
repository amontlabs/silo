"""The morning review's claim checker against public incidents.

    python3 catch_rate.py [--cases]

Each case of cases.json is an agent's words from a public issue and what an outside record would
have held, rebuilt from the issue's own evidence. Each version of the checker reads the case as it
reads a night's record, and the case counts as:
  caught     a sentence is a mismatch (the record contradicts it)
  flagged    no mismatch, but a sentence is unbacked (nothing in the record shows it)
  missed     neither
For a true claim (a control), a mismatch is a false alarm.
"""
import collections, hashlib, json, os, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "2026-11-24-morning-review", "proto"))
import claims

CASES = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "cases.json")))


def evidence(case):
    r = case["record"]
    cmds = [{"command": c["command"] + (" | tail" if "piped" in (c.get("note") or "") and "|" not in c["command"] else ""), "exit": c.get("exit"), "t": 2 * i}
            for i, c in enumerate(r.get("commands", []))]
    edits = [{"path": f["path"], "change": f["change"], "inside": f.get("inside_repo") is not False, "t": None} for f in r.get("files", [])]
    return {"commands": cmds, "edits": edits, "left_running": r.get("processes_left", []),
            "connections": [{"host": n, "decision": "allowed", "method": None} for n in r.get("network", [])], "cwd": "the repo"}


def half(case):
    """dev: the half claims@0.3 was tuned on; test: the half it never saw."""
    return "dev" if int(hashlib.sha1(case["id"].encode()).hexdigest(), 16) % 2 == 0 else "test"


def receipt(case, ev):
    """No words read: what a receipt's outcome lines alone would warn of. Its last command failed,
    files were deleted or changed outside the folder, a process was left running, a connection was
    refused or held."""
    warn = []
    if ev["commands"] and ev["commands"][-1]["exit"] not in (0, None):
        warn.append("last command failed")
    if any(e["change"] == "deleted" for e in ev["edits"]):
        warn.append("files deleted")
    if any(not e["inside"] for e in ev["edits"]):
        warn.append("files outside")
    if ev["left_running"]:
        warn.append("left running")
    return [{"says": "mismatch", "claim": "; ".join(warn)}] if warn else []


def outcome(case, verdicts):
    says = [v["says"] for v in verdicts]
    if case["truth"] == "true":
        return "false alarm" if "mismatch" in says else "quiet"
    if "mismatch" in says:
        return "caught"
    if "unbacked" in says:
        return "flagged"
    return "missed"


def main():
    show = "--cases" in sys.argv
    checks = {**claims.VERSIONS, "receipt only": "receipt", "0.3 or receipt": "both"}
    table = collections.defaultdict(collections.Counter)
    for case in CASES:
        ev = evidence(case)
        row = []
        for name, check in checks.items():
            vs = (receipt(case, ev) if check == "receipt" else claims.check_03(case["claim"], ev) + receipt(case, ev) if check == "both"
                  else check(case["claim"], ev))
            o = outcome(case, vs)
            row.append(o)
            if case["truth"] == "unclear":
                continue
            for scope in ("all", half(case), f"record shows it: {case['outside_record_shows_it']}", "verbatim" if case["claim_verbatim"] else "paraphrased"):
                table[(case["truth"], scope)][(name, o)] += 1
        if show:
            print(f"{case['id']:<26} {half(case):<5}{case['truth']:<7} {case['outside_record_shows_it']:<6} " + " ".join(f"{o:<11}" for o in row) + f" {case['claim'][:60]!r}")
    print()
    print(f"{'false claims':<26} {'n':>3}  " + "  ".join(f"{v:<17}" for v in checks))
    for (truth, scope), c in sorted(table.items()):
        n = sum(v for (name, o), v in c.items() if name == "claims@0.1")
        if truth == "false":
            cells = [f"{c[(nm, 'caught')]:>2} +{c[(nm, 'flagged')]:>2} / {n:<2}" for nm in checks]
        else:
            if scope != "all":
                continue
            print(f"{'true claims (controls)':<26} {n:>3}  " + "  ".join(f"{c[(nm, 'false alarm')]:>2} false alarms   " for nm in checks))
            continue
        print(f"{scope:<26} {n:>3}  " + "  ".join(f"{x:<17}" for x in cells))
    print("\ncaught + flagged / n: caught is a mismatch, flagged is unbacked. claims@0.3 was tuned on the dev half: read it on test.")


if __name__ == "__main__":
    main()
