#!/bin/bash
# The Linux analogue of test 07: gvproxy's Linux build (downloaded and checked against the
# release's sha256), a network namespace standing in for the computer, an OpenSSH server in it.
# Checks the configurations, the API calls, the Unix socket forward, the bursts and the reach
# probes, and that gvproxy as it comes lets the computer reach 127.0.0.1 and its API while the
# hardened configuration does not. Skips (exit 0) without root, network namespaces or sshd.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit07.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="$tmp/home" KIT_ID=07 KIT_WORK="$tmp/home/work/07" KIT_OUT="$tmp/results.jsonl"
export KIT_FAKE=1 KIT_YES=1 KIT_07_CONNS="${KIT_07_CONNS:-400}"
mkdir -p "$KIT_WORK"
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/07-gvproxy-macos-guest.sh" < /dev/null
rc=$?
if grep -q '"kind":"skip"' "$KIT_OUT"; then
  echo "test 07's analogue skipped: $(grep '"kind":"skip"' "$KIT_OUT" | head -1)"
  exit 0
fi
[ "$rc" = 0 ] || { echo "test 07 exited $rc"; exit 1; }
python3 - "$KIT_OUT" <<'EOF'
import json, sys
facts, outcome = {}, None
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e["kind"] == "fact": facts[e["name"]] = e["value"]
    if e["kind"] == "outcome": outcome = e
want = {
    "gh.reach.alias_loopback": "no", "gd.reach.alias_loopback": "reached",
    "gh.reach.lan_any": "reached", "gh.reach.lan_loopback": "no",
}
bad = []
for k, v in want.items():
    if not facts.get(k, "").startswith(v): bad.append("%s is %r, expected %s" % (k, facts.get(k), v))
if "socket made: yes" not in facts.get("gd.api_from_guest", ""): bad.append("as it comes, the computer could not make a socket through the API: %r" % facts.get("gd.api_from_guest"))
if "socket made: no" not in facts.get("gh.api_from_guest", ""): bad.append("hardened, the computer made a socket through the API: %r" % facts.get("gh.api_from_guest"))
if not outcome or outcome["key"] != "gvproxy-now": bad.append("outcome %r" % outcome)
for b in bad: print("FAIL", b)
print("outcome:", outcome and outcome["key"], "|", outcome and outcome["text"][:600])
sys.exit(1 if bad else 0)
EOF
