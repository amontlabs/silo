#!/bin/bash
# Test 45 on Linux: no disk loses power here, so the "cut" kills the writer; every acknowledged
# line must be there (full-keeps-acked). Then plugwriter check must find a line damaged in the
# middle and an acknowledged line cut off.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit45.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="$tmp/root" KIT_ID=45
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1 KIT_45_VOLUME="$tmp/vol"
export KIT_45_WRITE_SECONDS=1 KIT_45_CUT="pkill -9 -f 'plugwriter write $tmp/vol'"
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT/bin" "$tmp/vol"
cc -O2 -o "$KIT_WORK_ROOT/bin/plugwriter" "$KIT_ROOT/c/plugwriter.c" || { echo "FAIL: plugwriter does not build"; exit 1; }
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/45-record-power-cut.sh" < /dev/null > "$tmp/run.log" 2>&1
grep -q '"key":"full-keeps-acked"' "$KIT_OUT" || { echo "FAIL: expected full-keeps-acked"; grep -E '"kind":"(outcome|fact)"' "$KIT_OUT"; tail "$tmp/run.log"; exit 1; }
echo "ok   45: $(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-200)"
pw="$KIT_WORK_ROOT/bin/plugwriter"
( "$pw" write "$tmp/r" fsync > "$tmp/acks" ) & p=$!; sleep 0.5; kill "$p"; wait "$p" 2>/dev/null
last="$(tail -1 "$tmp/acks" | awk '{print $2}')"
printf 'Z' | dd of="$tmp/r" bs=1 seek=$((512 * 3 + 40)) conv=notrunc 2>/dev/null
r="$("$pw" check "$tmp/r" "$last")"
echo "$r" | grep -q '"first_damaged":3,' && echo "$r" | grep -q '"acked_lost":1}' || { echo "FAIL: a damaged middle line not found: $r"; exit 1; }
truncate -s $((512 * 10)) "$tmp/r"
r="$("$pw" check "$tmp/r" "$last")"
echo "$r" | grep -q "\"acked_lost\":$((last + 1 - 10 + 1))}" || { echo "FAIL: lines cut off not counted: $r"; exit 1; }
echo "ok   45: plugwriter check finds a damaged middle line and acknowledged lines cut off"
