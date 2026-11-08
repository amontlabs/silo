#!/usr/bin/env bash
# Phase F3 live check: share a fork as a file, with a fake Claude Code login on it.
# Runs against the Dev engine through ~/.local/bin/silo-dev, with throwaway e2e-share-* computers.
# Needs a running computer in the Dev engine to fork from: `main` unless BASE=<computer> names another
# (for example a throwaway e2e-share-base). The computer the import is kept into is e2e-share-target,
# a second fork of it, so the base itself is never changed.
# Optional: BEFORE_BYTES=<size of an export of the same computer made before F3> prints the
# size change. DEVICE=<connected device> also checks that --device is refused.
# Output goes to $OUT (default: ./f3-share-out).
set -u
SILO=${SILO:-$HOME/.local/bin/silo-dev}
OUT=${OUT:-./f3-share-out}
mkdir -p "$OUT"
BASE=${BASE:-main}
FILE="$OUT/e2e-share-src.silo-backup"
pass=0; fail=0
check() { # check NAME STATUS [DETAIL]
  if [ "$2" = 0 ]; then pass=$((pass + 1)); echo "PASS  $1 ${3:-}"; else fail=$((fail + 1)); echo "FAIL  $1 ${3:-}"; fi
}
json() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
state() { "$SILO" status "$1" --json 2>/dev/null | json 'd["state"]'; }
xs() { "$SILO" exec "$1" -- sh -c "$2"; }
cleanup() {
  for c in e2e-share-src e2e-share-copy e2e-share-target e2e-share-copy2; do
    "$SILO" stop "$c" >/dev/null 2>&1 # drop refuses a running computer
    "$SILO" drop "$c" >/dev/null 2>&1
  done
  rm -f "$FILE"
}

echo "== S0 start from nothing"
cleanup
[ "$(state "$BASE")" = running ] || { echo "$BASE is not running in the Dev engine; start it first (or set BASE=<running computer>)"; exit 2; }

echo "== S1 a fork with a fake Claude Code login and a marker file"
"$SILO" fork "$BASE" --name e2e-share-target >/dev/null 2>&1; "$SILO" start e2e-share-target >/dev/null 2>&1
"$SILO" fork "$BASE" --name e2e-share-src >/dev/null 2>&1; "$SILO" start e2e-share-src >/dev/null 2>&1
check "S1 both forks run" $([ "$(state e2e-share-src)" = running ] && [ "$(state e2e-share-target)" = running ]; echo $?)
xs e2e-share-src 'mkdir -p ~/.claude && echo "{\"fake\":\"not-a-real-login\"}" > ~/.claude/.credentials.json; echo from-the-sender > ~/share-marker.txt; echo sender > ~/share-conflict.txt'
sleep 5

echo "== S2 export without --yes and without a terminal: the warning, and no file"
"$SILO" export e2e-share-src -o "$FILE" < /dev/null > "$OUT/s2.out" 2> "$OUT/s2.err"; rc=$?
cat "$OUT/s2.err"
check "S2 refused without a terminal" $([ $rc != 0 ]; echo $?) "(exit $rc)"
check "S2 names the Claude Code login" $(grep -q "a Claude Code login" "$OUT/s2.err"; echo $?)
check "S2 wrote no file" $([ ! -e "$FILE" ]; echo $?)

echo "== S3 export with --yes: the login is reported and stays in the file"
t0=$(date +%s)
"$SILO" export e2e-share-src -o "$FILE" --yes --json > "$OUT/s3.json" 2> "$OUT/s3.err"; rc=$?
echo "export took $(( $(date +%s) - t0 )) s"
check "S3 export succeeded" $rc
size=$(stat -f %z "$FILE" 2>/dev/null || stat -c %s "$FILE")
echo "export size: $size bytes"
[ -n "${BEFORE_BYTES:-}" ] && echo "before F3: $BEFORE_BYTES bytes; after: $size bytes"
check "S3 --json lists the login" $(json 'any("Claude Code" in l for l in d["logins"])' < "$OUT/s3.json" | grep -q True; echo $?)

echo "== S4 import under a new name"
"$SILO" import "$FILE" --name e2e-share-copy --json > "$OUT/s4.json" 2> "$OUT/s4.err"; rc=$?
check "S4 import succeeded" $rc
check "S4 the copy is stopped" $([ "$(state e2e-share-copy)" = stopped ]; echo $?)
"$SILO" start e2e-share-copy >/dev/null 2>&1
check "S4 the marker came along" $(xs e2e-share-copy 'cat ~/share-marker.txt' | grep -q from-the-sender; echo $?)
check "S4 the fake login is still in the copy (never filtered)" $(xs e2e-share-copy 'test -f ~/.claude/.credentials.json'; echo $?)
"$SILO" import "$FILE" --json > "$OUT/s4b.json" 2>/dev/null
check "S4 a second import without --name picks a free name" $([ "$(json 'd["name"]' < "$OUT/s4b.json")" != e2e-share-src ]; echo $?) "($(json 'd["name"]' < "$OUT/s4b.json" 2>/dev/null))"
"$SILO" drop "$(json 'd["name"]' < "$OUT/s4b.json" 2>/dev/null)" --force >/dev/null 2>&1

echo "== S5 the copy has a fork-time record: diff shows the sender's work"
"$SILO" diff e2e-share-copy --json > "$OUT/s5.json" 2> "$OUT/s5.err"; rc=$?
check "S5 silo diff works on the imported computer" $rc
grep -q "share-marker.txt" "$OUT/s5.json"; check "S5 the diff lists the marker file" $?

echo "== S6 fold --into without a target says it does not know"
"$SILO" fold e2e-share-copy > "$OUT/s6.out" 2> "$OUT/s6.err"; rc=$?
check "S6 refused" $([ $rc != 0 ]; echo $?)
grep -qi "does not know where" "$OUT/s6.err" "$OUT/s6.out"; check "S6 says where it was forked from is unknown" $?

# Review the copy again right before keeping it: a keep is pinned to the digest of the last
# `silo diff` of the fork, and the copy has run since S5 reviewed it.
"$SILO" diff e2e-share-copy --json > "$OUT/s7-review.json" 2> "$OUT/s7-review.err"
echo "== S7 fold into the receiver's main (e2e-share-target): kept three-way"
"$SILO" fold e2e-share-copy --into e2e-share-target --dry-run --json > "$OUT/s7a.json" 2> "$OUT/s7a.err"
check "S7 the dry run reports a plan" $(json 'd["status"]=="dry-run"' < "$OUT/s7a.json" | grep -q True; echo $?)
"$SILO" fold e2e-share-copy --into e2e-share-target --json > "$OUT/s7.json" 2> "$OUT/s7.err"
check "S7 kept" $(json 'd["status"]=="kept"' < "$OUT/s7.json" | grep -q True; echo $?)
check "S7 the marker is in the target" $(xs e2e-share-target 'cat ~/share-marker.txt' | grep -q from-the-sender; echo $?)

echo "== S8 conflict: the receiver changed the same file differently"
"$SILO" import "$FILE" --name e2e-share-copy2 >/dev/null 2>&1; "$SILO" start e2e-share-copy2 >/dev/null 2>&1
xs e2e-share-target 'echo receiver > ~/share-conflict.txt'
sleep 5
"$SILO" fold e2e-share-copy2 --into e2e-share-target --json > "$OUT/s8.json" 2> "$OUT/s8.err"
check "S8 refused as divergent" $(json 'd["status"]=="refused"' < "$OUT/s8.json" | grep -q True; echo $?)
grep -q "share-conflict.txt" "$OUT/s8.json"; check "S8 names the conflicting file" $?
check "S8 the target keeps the receiver's version" $(xs e2e-share-target 'cat ~/share-conflict.txt' | grep -q receiver; echo $?)

echo "== S9 a checkpoint export, and --device"
cp=$("$SILO" checkpoint e2e-share-src --name e2e-share-cp --json 2>/dev/null | json 'd["name"]')
"$SILO" export e2e-share-src --checkpoint "$cp" -o "$OUT/cp.silo-backup" --yes > /dev/null 2> "$OUT/s9a.err"; rc=$?
if [ $rc != 0 ] && grep -q "free space" "$OUT/s9a.err"; then
  echo "SKIP  S9 checkpoint export: the free-space check refused it: $(tail -1 "$OUT/s9a.err")"
else
  check "S9 a checkpoint exports by name" $rc
fi
if [ -n "${DEVICE:-}" ]; then
  "$SILO" --device "$DEVICE" export e2e-share-src -o "$OUT/dev.silo-backup" > /dev/null 2> "$OUT/s9.err"
  check "S9 --device is refused with the later-phase message" $(grep -q "cannot be used with them yet" "$OUT/s9.err"; echo $?)
else
  echo "SKIP  S9 --device (set DEVICE=<a connected device> to run it)"
fi

cleanup
rm -f "$OUT/cp.silo-backup"
echo "== done: $pass passed, $fail failed (evidence in $OUT)"
[ "$fail" = 0 ]
