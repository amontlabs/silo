#!/bin/bash
# Test 46 on Linux, with linux/fake-tmutil (a snapshot is hard links): it must end
# snapshot-holds-space with the space back when the snapshot goes; with the snapshot refused,
# no-snapshot.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit46.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="$tmp/root" KIT_ID=46
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1 KIT_46_MB=64 KIT_46_SETTLE=1
export KIT_TMUTIL="$KIT_ROOT/linux/fake-tmutil" FAKE_TM_DATA="$tmp/w/data"
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT/bin"
cc -O2 -o "$KIT_WORK_ROOT/bin/cloneattrs" "$KIT_ROOT/c/cloneattrs.c" || { echo "FAIL: cloneattrs does not build"; exit 1; }
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/46-local-snapshots-and-free-space.sh" < /dev/null > "$tmp/run.log" 2>&1
grep -q '"key":"snapshot-holds-space"' "$KIT_OUT" || { echo "FAIL: expected snapshot-holds-space"; grep -E '"kind":"(outcome|measure|fact)"' "$KIT_OUT"; tail "$tmp/run.log"; exit 1; }
echo "ok   46: $(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-200)"
: > "$KIT_OUT"; rm -rf "$KIT_WORK"; mkdir -p "$KIT_WORK"
FAKE_TM_REFUSE=1 bash "$KIT_ROOT/tests/46-local-snapshots-and-free-space.sh" < /dev/null > "$tmp/run2.log" 2>&1
grep -q '"key":"no-snapshot"' "$KIT_OUT" || { echo "FAIL: expected no-snapshot"; grep '"kind":"outcome"' "$KIT_OUT"; exit 1; }
echo "ok   46 with the snapshot refused: no-snapshot"
