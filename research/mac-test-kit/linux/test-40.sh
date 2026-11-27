#!/bin/bash
# The Linux analogue of test 40, as #378 measured it: a network namespace stands for the computer,
# py/deadend.py for its card's other end, Unix sockets for vsock, vsockrelay for the relays, an
# OpenSSH server for the computer's own. The proxy may reach only an origin on this machine. Must
# end closed-holds; then, with a second link planted out of the namespace, leaks. Skips (exit 0)
# without root, namespaces or sshd.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit40.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="$tmp/root" KIT_ID=40
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT/bin"
cc -O2 -Wall -o "$KIT_WORK_ROOT/bin/vsockrelay" "$KIT_ROOT/c/vsockrelay.c" || { echo "FAIL: vsockrelay does not build"; exit 1; }
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/40-card-that-leads-nowhere.sh" < /dev/null > "$tmp/run.log" 2>&1
rc=$?
if grep -q '"kind":"skip"' "$KIT_OUT"; then
  echo "test 40's analogue skipped: $(grep '"kind":"skip"' "$KIT_OUT" | head -1)"; exit 0
fi
[ "$rc" = 0 ] || { echo "test 40 exited $rc"; tail -20 "$tmp/run.log"; exit 1; }
if ! grep -q '"key":"closed-holds"' "$KIT_OUT"; then
  echo "FAIL: expected closed-holds"; grep -E '"kind":"(outcome|fact|note)"' "$KIT_OUT" | tail -30; tail -20 "$tmp/run.log"; exit 1
fi
echo "ok   40: $(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-200)"
grep -q '"name":"program.none.curl","value":"exit [0-9]*, never reached the proxy"' "$KIT_OUT" ||
  { echo "FAIL: curl without the proxy variables should never reach the proxy"; exit 1; }
echo "ok   40: without the variables curl reached nothing"

# The planted fault: a second link out of the namespace, routed to this machine's address.
lan="$(python3 -c '
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
try:
    s.connect(("192.0.2.1", 9)); print(s.getsockname()[0])
except OSError: pass')"
[ -n "$lan" ] || { echo "skip the planted fault: this machine has no address"; exit 0; }
: > "$KIT_OUT"; rm -rf "$KIT_WORK"; mkdir -p "$KIT_WORK"
DE_PLANT_LEAK_TO="$lan" bash "$KIT_ROOT/tests/40-card-that-leads-nowhere.sh" < /dev/null > "$tmp/mutant.log" 2>&1
if grep -q '"key":"leaks"' "$KIT_OUT"; then
  echo "ok   40 with a second link planted: leaks ($(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-160))"
else
  echo "FAIL: the planted link was not caught"; grep '"kind":"outcome"' "$KIT_OUT"; tail -20 "$tmp/mutant.log"; exit 1
fi
