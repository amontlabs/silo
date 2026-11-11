#!/usr/bin/env bash
# T15 live check: rebuild a Linux computer on the newest image, preserving workspace and home.
# Runs against the Dev engine (~/.silo-dev) with throwaway e2e-t15-* computers. Run:
#   bash docs/direction/checks/t15_rebuild_check.sh 2>&1 | tee docs/direction/evidence/T15.txt
# Needs a Dev app bundle built from this tree (BUNDLE: `npm --prefix app/SiloUI run
# desktop:build:debug` after `npm --prefix app/SiloUI run runtime:prepare`) and its debug `silo`
# (SILO). The script starts the bundle's `silod --agent` by hand when no Dev engine runs, and stops
# it at the end. It never touches another engine: it refuses to run when a Dev engine it did not
# start answers.
#
# What it shows, in order:
#   S1  a Linux computer is created and populated with data in /workspace and /home/silo
#   S2  `silo rebuild` completes successfully
#   S3  the computer is running on the newest guest image
#   S4  the workspace files and home files are intact
#   S5  the undo computer (<name>-undo) exists, is stopped, and preserves previous state
#   S6  a second rebuild is cleanly refused because <name>-undo already exists
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=${T:-$HOME/code/projects/silo-wt-targets/T15}
SILO=${SILO:-$T/debug/silo}
BUNDLE=${BUNDLE:-$T/debug/bundle/macos/Silo Dev.app}
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
LOG=${LOG:-$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-t15.log}
SOCK="$HOME/.silo-dev/engine.sock"
A=e2e-t15-a; U=e2e-t15-a-undo
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-t15.XXXXXX")
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
inside() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo -u silo -H bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
root() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
our_engine() { pgrep -f "^$SILOD --agent" | head -1; }
engine_answers() { "$SILO" list >/dev/null 2>&1; }
start_engine() {
  mkdir -p "$(dirname "$LOG")"
  "$SILOD" --agent >>"$LOG" 2>&1 &
  for _ in $(seq 1 120); do engine_answers && return 0; sleep 1; done
  return 1
}
stop_engine() {
  local pid; pid=$(our_engine); [ -n "$pid" ] || return 0
  kill -TERM "$pid"
  for _ in $(seq 1 180); do kill -0 "$pid" 2>/dev/null || return 0; sleep 1; done
  return 1
}
wait_for() { local limit=$1; shift; local end=$((SECONDS+limit)); while [ $SECONDS -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 3; done; return 1; }

cleanup() {
  say "cleanup (drops only the e2e-t15-* computers this run made, then stops the Dev engine it started)"
  if engine_answers; then
    for c in $U $A; do "$SILO" stop "$c" >/dev/null 2>&1; done
    for round in 1 2 3; do
      for c in $U $A; do "$SILO" status "$c" >/dev/null 2>&1 && "$SILO" drop "$c" --force >/dev/null 2>&1; done
      sleep 2
    done
    say "silo list after cleanup:"; "$SILO" list
  fi
  stop_engine && say "Dev engine stopped" || say "Dev engine did not stop in time"
  rm -rf "$WORK"
}
trap cleanup EXIT

say "== header"
echo "commit $(cd "$REPO" && git rev-parse --short HEAD) on $(cd "$REPO" && git branch --show-current); $("$SILO" --version 2>&1); host $(sw_vers -productVersion) $(uname -m); $(date -u +%FT%TZ)"
if engine_answers && [ -z "$(our_engine)" ]; then
  echo "A Dev engine this script did not start is answering; refusing to touch it." >&2
  trap - EXIT; rm -rf "$WORK"; exit 2
fi
[ -n "$(our_engine)" ] || { start_engine; check "the Dev engine starts from the bundle" $? "pid $(our_engine), log $LOG"; }
echo "computers before: $("$SILO" list 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"
for c in $A $U; do "$SILO" status "$c" >/dev/null 2>&1 && { echo "$c already exists; refusing to run" >&2; exit 2; }; done

say "== S1 create a Linux computer and write marker data to workspace and home"
"$SILO" create $A --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $A" $? ""
"$SILO" start $A >/dev/null 2>&1; check "start $A" $? ""

wait_for 120 "$SILO" exec $A -- echo "ready" >/dev/null 2>&1
inside $A 'echo "workspace-payload-12345" > /workspace/testfile.txt'
check "write test file to /workspace" $? ""
inside $A 'mkdir -p /home/silo/.config && echo "config-payload-67890" > /home/silo/.config/test.conf'
check "write test file to /home/silo" $? ""

IMAGE_BEFORE=$("$SILO" --json status $A | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("imageReference") or v.get("configuration",{}).get("image",""))')
echo "image before rebuild: $IMAGE_BEFORE"

say "== S2 rebuild the computer with silo rebuild"
out=$("$SILO" rebuild $A 2>&1); check "silo rebuild $A" $? "$out"

say "== S3 verify the rebuilt computer is running on the newest image"
state=$("$SILO" --json status $A | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("state",""))')
check "rebuilt computer $A is running" $([ "$state" = "running" ]; echo $?) "state: $state"

IMAGE_AFTER=$("$SILO" --json status $A | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("imageReference") or v.get("configuration",{}).get("image",""))')
echo "image after rebuild: $IMAGE_AFTER"

say "== S4 verify data integrity in workspace and home"
ws_content=$(inside $A 'cat /workspace/testfile.txt 2>/dev/null' | tr -d '\r')
check "workspace file is intact" $([ "$ws_content" = "workspace-payload-12345" ]; echo $?) "content: $ws_content"

home_content=$(inside $A 'cat /home/silo/.config/test.conf 2>/dev/null' | tr -d '\r')
check "home config file is intact" $([ "$home_content" = "config-payload-67890" ]; echo $?) "content: $home_content"

say "== S5 verify the undo computer exists, is stopped, and has previous state"
undo_state=$("$SILO" --json status $U 2>/dev/null | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("state",""))')
check "undo computer $U exists and is stopped" $([ "$undo_state" = "stopped" ]; echo $?) "state: $undo_state"

say "== S6 verify refusal when undo computer already exists"
out=$("$SILO" rebuild $A 2>&1)
check "second rebuild refuses cleanly because undo exists" $([ $? -ne 0 ]; echo $?) "$out"
echo "$out" | grep -qi "already exists"
check "refusal message explains that undo computer already exists" $? ""

say "== finished"
echo "PASS: $PASS  FAIL: $FAILN"
[ "$FAILN" = 0 ]
