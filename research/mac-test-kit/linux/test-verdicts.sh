#!/bin/bash
# ./kit verdicts on made-up runs: the newest outcome wins across runs, an outcome's line narrows
# the options, a fact's line too, two tests that leave nothing in common give a conflict, and a
# decision some of whose tests have not answered leans.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
ev() { # DIR TEST KIND T JSON-FIELDS
  printf '{"test":"%s","kind":"%s","t":%s,%s}\n' "$2" "$3" "$4" "$5" >> "$1/results.jsonl"
}
mkdir -p "$tmp/1" "$tmp/2"
ev "$tmp/1" 07 outcome 100 '"key":"gvproxy-now","text":"x"'
ev "$tmp/1" 35 outcome 100 '"key":"sealed-holds","text":"x"'
ev "$tmp/1" 36 outcome 100 '"key":"transparent","text":"x"'
ev "$tmp/1" 37 fact 100 '"name":"vm_holds_awake","value":"yes"'
ev "$tmp/1" 37 outcome 101 '"key":"wakes-whole","text":"x"'
ev "$tmp/1" 16 fact 100 '"name":"vz_holds_assertion","value":"no"'
ev "$tmp/1" 16 outcome 101 '"key":"quiet","text":"x"'
ev "$tmp/2" 35 outcome 200 '"key":"sealed-leaks","text":"x"'
ev "$tmp/2" 01 outcome 200 '"key":"inconclusive","text":"x"'
python3 - "$KIT_ROOT" "$tmp" <<'PY'
import sys
sys.path.insert(0, sys.argv[1] + "/py")
import kitreport
root, tmp = sys.argv[1], sys.argv[2]
want = {
    # run 2's sealed-leaks is newer than run 1's sealed-holds
    "network-stack": ("leaning", "configured-gvproxy"),  # 40 to 42 have not answered
    # 37 says the framework holds it awake, 16 says it does not
    "mac-kept-awake": ("conflict", None),
    # 07 and 35 answered and narrow nothing here; 00 01 13 not (01 inconclusive)
    "main-on-macos-first": ("leaning", "a-macos-boundary"),
}
_, rows = kitreport.verdicts(root, [tmp + "/1", tmp + "/2"], out=False)
by = {v["id"]: v for v in rows}
bad = 0
for k, (status, option) in want.items():
    v = by[k]
    if (v["status"], v["option"]) != (status, option):
        print("FAIL %s: %s %s, expected %s %s (%s)" % (k, v["status"], v["option"], status, option, v["text"]))
        bad += 1
    else:
        print("ok   %s: %s %s" % (k, status, option or ""))
if "01 (inconclusive)" not in by["main-on-macos-first"]["waiting"]:
    print("FAIL an inconclusive outcome was counted as an answer"); bad += 1
# Run 1 alone: the sealed gvproxy stands.
_, rows = kitreport.verdicts(root, [tmp + "/1"], out=False)
v = {r["id"]: r for r in rows}["network-stack"]
if v["option"] != "sealed-gvproxy":
    print("FAIL run 1 alone gave %s" % v["option"]); bad += 1
sys.exit(1 if bad else 0)
PY
