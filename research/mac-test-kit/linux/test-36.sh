#!/bin/bash
# The Linux analogue of test 36: gvproxy as released (test 07's hardened configuration), a network
# namespace for the computer whose network device is a unixgram socket held as the Virtualization
# framework holds it, an OpenSSH server in it, and py/36-server.py on this machine's address.
# #347's checks 6 to 9 must hold; after gvproxy restarts, the computer's network must stay dead
# when its socket goes straight to gvproxy, and come back through py/36-relay.py: the outcome is
# restart-needs-helper. Also runs py/36-decide.py's fixtures. Skips (exit 0) without root,
# namespaces, sshd or a network.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
python3 -I "$KIT_ROOT/py/36-decide.py" --selftest || exit 1
perl -c "$KIT_ROOT/guest/36-checks.pl" 2>/dev/null || { echo "FAIL: guest/36-checks.pl does not compile"; exit 1; }
tmp="$(mktemp -d /tmp/kit36.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="${KIT_35_HOME:-${TMPDIR:-/tmp}/silo-kit-35-home}" KIT_ID=36
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT"
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/36-stack-cost-and-restart.sh" < /dev/null > "$tmp/run.log" 2>&1
rc=$?
if grep -q '"kind":"skip"' "$KIT_OUT"; then
  echo "test 36's analogue skipped: $(grep '"kind":"skip"' "$KIT_OUT" | head -1)"; exit 0
fi
[ "$rc" = 0 ] || { echo "test 36 exited $rc"; tail -30 "$tmp/run.log"; exit 1; }
if grep -q '"key":"restart-needs-helper"' "$KIT_OUT"; then
  echo "ok   36: restart-needs-helper ($(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-200))"
else
  echo "FAIL: expected restart-needs-helper"; grep -E '"kind":"(outcome|fact|note)"' "$KIT_OUT" | tail -30; tail -20 "$tmp/run.log"; exit 1
fi
