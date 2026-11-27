#!/bin/bash
# The Linux analogue of test 42: gvproxy's Linux build at two MTUs, a network namespace whose card is
# linux/fake-guest-net.py (Python, so the numbers mean nothing), transfers against this machine's
# loopback. Checks that every step runs and an outcome other than inconclusive comes out.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit42.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="${KIT_35_HOME:-${TMPDIR:-/tmp}/silo-kit-35-home}" KIT_ID=42
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1 KIT_42_MB=8
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT"
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/42-datagram-throughput.sh" < /dev/null > "$tmp/run.log" 2>&1
rc=$?
if grep -q '"kind":"skip"' "$KIT_OUT"; then echo "test 42's analogue skipped: $(grep '"kind":"skip"' "$KIT_OUT" | head -1)"; exit 0; fi
[ "$rc" = 0 ] || { echo "test 42 exited $rc"; tail -20 "$tmp/run.log"; exit 1; }
grep -q '"kind":"outcome"' "$KIT_OUT" && ! grep -q '"key":"inconclusive"' "$KIT_OUT" ||
  { echo "FAIL: no outcome, or inconclusive"; grep -E '"kind":"(outcome|note|measure)"' "$KIT_OUT" | tail; tail "$tmp/run.log"; exit 1; }
grep -q '"name":"gvproxy_mtu9000_in"' "$KIT_OUT" || { echo "FAIL: no measure at MTU 9000"; exit 1; }
echo "ok   42: $(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"key":"//' | cut -c1-200)"
