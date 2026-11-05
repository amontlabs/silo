#!/usr/bin/env bash
# Silo v1 phase 3 VM check, C: folder forks (silo fork ./repo, silo fold, silo undo). Run:
#   bash docs/v1/checks/phase3_folder_check.sh 2>&1 | tee docs/v1/evidence/phase-3/folder-check.txt
# Only `silo` (incl. `silo exec` in the fork) and git on a temp repo this script creates (fixture + read-only observation).
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
FORK=e2e-p3-folder
ROOT=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p3-folder.XXXXXX"); H="$ROOT/p3repo"
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
g() { git -C "$H" "$@"; }
cleanup() {
  echo "[$(ts)] cleanup"
  "$SILO" stop $FORK >/dev/null 2>&1; for i in $(seq 20); do "$SILO" drop $FORK --force >/dev/null 2>&1 && break; "$SILO" status $FORK >/dev/null 2>&1 || break; sleep 3; done
  (cd "$ROOT" && "$SILO" recipe rm linux-dev) 2>&1; rm -rf "$ROOT"; "$SILO" list
  echo "== summary: $PASS passed, $FAILN failed"
}
trap cleanup EXIT
echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); tracked changes under app/: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1); silo: $("$SILO" --version); host macOS $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"
mkdir -p "$H"; g init -q -b main; g config user.name p3; g config user.email p3@example.invalid
echo one > "$H/a.txt"; g add a.txt; g commit -qm "first"; HEAD0=$(g rev-parse HEAD); echo "host repo $H at $HEAD0 on $(g branch --show-current)"
cd "$ROOT"
t=$(date +%s); "$SILO" fork ./p3repo --name $FORK > /tmp/p3-ffork.out 2>&1; check "silo fork ./p3repo" $? "$(( $(date +%s)-t )) s"; tail -4 /tmp/p3-ffork.out
echo "guest OS: $("$SILO" exec $FORK -- bash -c '. /etc/os-release; echo $PRETTY_NAME; echo home=$HOME; ls ~/p3repo')"
"$SILO" exec $FORK -- bash -c 'cd ~/p3repo && echo two > b.txt && git add b.txt && git -c user.name=fork -c user.email=fork@example.invalid commit -qm "from the fork" && git log --oneline | head -3'
check "commit made in the fork" $? ""
echo "-- refusal: dirty host tree"
echo dirty >> "$H/a.txt"
"$SILO" fold $FORK > /tmp/p3-f1.out 2>&1; rc=$?; cat /tmp/p3-f1.out | tail -3
[ $rc != 0 ] && [ "$(g rev-parse HEAD)" = "$HEAD0" ]; check "fold refused with uncommitted tracked changes, host HEAD unchanged" $? "rc=$rc"
g checkout -q -- a.txt
echo "-- silo fold"
t=$(date +%s); "$SILO" fold $FORK > /tmp/p3-f2.out 2>&1; rc=$?; tail -6 /tmp/p3-f2.out; check "silo fold $FORK exits 0" $rc "$(( $(date +%s)-t )) s"
echo "host log:"; g log --oneline --decorate | head -5; echo "refs:"; g for-each-ref | cut -c1-120
FOLDHEAD=$(g rev-parse HEAD)
g log --format=%s | grep -q "from the fork" && [ "$(cat "$H/b.txt" 2>/dev/null)" = two ]; check "host log shows the fork's commit and b.txt is there" $? ""
BK=$(g for-each-ref --format='%(refname) %(objectname)' refs/silo/backup/ | head -1); echo "backup: $BK"
[ "${BK##* }" = "$HEAD0" ]; check "backup ref refs/silo/backup/* marks the previous HEAD" $? "$BK"
g for-each-ref refs/silo/ | grep -q "refs/silo/$FORK/head"; check "refs/silo/$FORK/head exists" $? ""
echo "-- refusal: undo with uncommitted tracked changes"
echo dirty >> "$H/a.txt"
"$SILO" undo $FORK > /tmp/p3-u1.out 2>&1; rc=$?; tail -3 /tmp/p3-u1.out
[ $rc != 0 ] && [ "$(g rev-parse HEAD)" = "$FOLDHEAD" ]; check "undo refused with a dirty host tree, HEAD unchanged" $? "rc=$rc"
g checkout -q -- a.txt
echo "-- refusal: undo with new commits since the fold"
echo three > "$H/c.txt"; g add c.txt; g commit -qm "host moved on"; MOVED=$(g rev-parse HEAD)
"$SILO" undo $FORK > /tmp/p3-u2.out 2>&1; rc=$?; tail -3 /tmp/p3-u2.out
[ $rc != 0 ] && [ "$(g rev-parse HEAD)" = "$MOVED" ]; check "undo refused after new host commits, HEAD unchanged" $? "rc=$rc"
g reset -q --hard "$FOLDHEAD"
echo "-- silo undo"
"$SILO" undo $FORK > /tmp/p3-u3.out 2>&1; rc=$?; tail -4 /tmp/p3-u3.out; check "silo undo $FORK exits 0" $rc ""
[ "$(g rev-parse HEAD)" = "$HEAD0" ] && [ "$(g branch --show-current)" = main ] && [ -z "$(g status --porcelain)" ] && [ ! -e "$H/b.txt" ] && [ "$(cat "$H/a.txt")" = one ]; check "host branch restored exactly (HEAD $HEAD0, branch main, clean tree, no b.txt)" $? "HEAD now $(g rev-parse HEAD)"
g log --oneline | head -3
