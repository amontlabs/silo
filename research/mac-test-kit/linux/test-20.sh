#!/bin/bash
# The Linux analogue of test 20: peercred built here, its Linux path (SO_PEERCRED, SO_PEERPIDFD,
# /proc) run for real, the other user played by nobody through setpriv when run as root.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit20.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export KIT_WORK_ROOT="$tmp/home" KIT_ID=20 KIT_WORK="$tmp/home/work/20" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT/bin"
cc -O2 -Wall -o "$KIT_WORK_ROOT/bin/peercred" "$KIT_ROOT/c/peercred.c" || exit 1
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/20-engine-socket-peer.sh" < /dev/null || { echo "test 20 failed"; cat "$KIT_OUT"; exit 1; }
python3 - "$KIT_OUT" "$(id -u)" <<'EOF'
import json, sys
facts, outcome = {}, None
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e["kind"] == "fact": facts[e["name"]] = e["value"]
    if e["kind"] == "outcome": outcome = e
bad = []
if not facts.get("direct.identity", "").startswith("uid %s pid " % sys.argv[2]): bad.append("direct.identity %r" % facts.get("direct.identity"))
if "/sleep" not in facts.get("exec_sleep.program", "").split("after exec")[-1]: bad.append("exec_sleep.program %r" % facts.get("exec_sleep.program"))
if "peercred" not in facts.get("orphan.chain", ""): bad.append("orphan.chain %r" % facts.get("orphan.chain"))
if sys.argv[2] == "0":
    if "connect: Permission denied" not in facts.get("other_user.private_socket", ""): bad.append("private %r" % facts.get("other_user.private_socket"))
    if "answer: refused" not in facts.get("other_user.open_socket", ""): bad.append("open %r" % facts.get("other_user.open_socket"))
if not outcome or outcome["key"] != "inconclusive": bad.append("outcome %r" % outcome)
for k in sorted(facts): print("%s = %s" % (k, facts[k][:160]))
for b in bad: print("FAIL", b)
print("outcome:", outcome and outcome["key"], "|", outcome and outcome["text"])
sys.exit(1 if bad else 0)
EOF
