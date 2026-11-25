#!/bin/bash
# Test 38 has no Linux analogue (a full volume under Apple's Virtualization framework): this checks
# its decision on every outcome (py/38-decide.py's fixtures) and that the test skips cleanly here.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
python3 -I "$KIT_ROOT/py/38-decide.py" --selftest || exit 1
python3 -I "$KIT_ROOT/py/37-decide.py" --selftest || exit 1
python3 -I "$KIT_ROOT/py/assertions.py" --selftest || exit 1
[ "$(uname -s)" = Darwin ] && exit 0
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
KIT_ID=38 KIT_WORK="$tmp" KIT_OUT="$tmp/o.jsonl" KIT_FAKE=1 KIT_YES=1 bash "$KIT_ROOT/tests/38-full-volume.sh" < /dev/null > "$tmp/log" 2>&1
grep -q '"kind":"skip"' "$tmp/o.jsonl" || { echo "FAIL: test 38 did not skip on Linux"; cat "$tmp/log"; exit 1; }
echo "ok   38: skips on Linux; its decision gives every outcome"
