#!/bin/bash
# Test 44 on Linux, with Playwright's headless Chromium and tcpdump: the page must report in and
# end flags-hold. Skips without root, tcpdump or that Chromium.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit44.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="$tmp/root" KIT_ID=44
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1 KIT_44_SECONDS=15
mkdir -p "$KIT_WORK"
[ "$(id -u)" = 0 ] && command -v tcpdump >/dev/null || { echo "test 44's analogue skipped: needs root and tcpdump"; exit 0; }
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/44-preview-browser.sh" < /dev/null > "$tmp/run.log" 2>&1
if grep -q '"kind":"skip"' "$KIT_OUT"; then echo "test 44's analogue skipped: $(grep '"kind":"skip"' "$KIT_OUT")"; exit 0; fi
grep -q '"key":"flags-hold"' "$KIT_OUT" || { echo "FAIL: expected flags-hold"; grep -E '"kind":"(outcome|fact)"' "$KIT_OUT"; tail "$tmp/run.log"; exit 1; }
echo "ok   44: $(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-220)"
