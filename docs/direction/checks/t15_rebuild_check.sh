#!/usr/bin/env bash
# T15 live check: rebuild a Linux computer on the newest image, preserving workspace and home.
# Runs against the Dev engine (~/.silo-dev) with throwaway e2e-t15-* computers. Run:
#   bash docs/direction/checks/t15_rebuild_check.sh 2>&1 | tee docs/direction/evidence/T15.txt
# Needs a Dev app bundle built from this tree (BUNDLE: `npm --prefix app/SiloUI run
# desktop:build:debug` after `npm --prefix app/SiloUI run runtime:prepare`), its debug `silo`
# (SILO) and the bundle's `msb` (MSB, found inside the bundle when not given) to read a
# sandbox's image reference. The script starts the bundle's `silod --agent` by hand when no Dev
# engine runs, and stops it at the end. It never touches another engine: it refuses to run when
# a Dev engine it did not start answers, and every `silo` call is pinned to the Dev socket.
#
# What it shows, in order:
#   S0  every call goes to the Dev socket (~/.silo-dev/engine.sock) of the engine this script runs
#   S1  a running Linux computer is populated: /workspace, /home/silo, and a file outside both
#   S2  a rebuild of a computer already on the newest image is refused unless --force is given
#   S3  `silo rebuild --force` works; the image reference before and after is compared with the
#       newest one the rebuild reports; the build sandbox never shows in `silo list`
#   S4  workspace and home are intact in the rebuilt computer; the file outside them is gone
#   S5  <name>-undo exists, is stopped, and keeps the old data (including the outside file)
#   S6  a second rebuild is refused because <name>-undo exists
#   S7  a stopped computer is rebuilt and stays stopped (so does its undo computer)
#   S8  the engine is killed in the middle of a rebuild; after restart the rebuild is settled
#       (rolled back or finished), the data is intact and no build sandbox or staged file is left
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=${T:-$HOME/code/projects/silo-wt-targets/T15}
SILO_BIN=${SILO:-$T/debug/silo}
BUNDLE=${BUNDLE:-$T/debug/bundle/macos/Silo Dev.app}
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
MSB=${MSB:-$(find "$BUNDLE" -type f -name 'msb*' -perm -u+x 2>/dev/null | head -1)}
LOG=${LOG:-$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-t15.log}
SOCK="$HOME/.silo-dev/engine.sock"
ENGINE_HOME="$HOME/.silo-dev/engine"
A=e2e-t15-a; AU=e2e-t15-a-undo
B=e2e-t15-b; BU=e2e-t15-b-undo
C=e2e-t15-c; CU=e2e-t15-c-undo
ALL="$AU $A $BU $B $CU $C"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-t15.XXXXXX")
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
# Every call to the engine goes through here, pinned to the Dev socket.
silo() { "$SILO_BIN" --socket "$SOCK" "$@"; }
inside() { local c=$1; shift; silo exec "$c" --timeout 170 -- sudo -u silo -H bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
root() { local c=$1; shift; silo exec "$c" --timeout 170 -- sudo bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
our_engine() { pgrep -f "^$SILOD --agent" | head -1; }
engine_answers() { silo list >/dev/null 2>&1; }
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
state_of() { silo --json status "$1" 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin).get("state",""))' 2>/dev/null; }
exists() { silo status "$1" >/dev/null 2>&1; }
# The image reference a sandbox was built on, read from the runtime of the Dev engine.
runtime_home() { ls -d "$HOME"/.silo-dev/*/sandboxes 2>/dev/null | head -1 | xargs -I{} dirname {}; }
msb() { MSB_HOME=$(runtime_home) MSB_PATH="$MSB" "$MSB" "$@"; }
image_of() { msb inspect "$1" --format json 2>/dev/null | python3 -c 'import json,sys; c=json.load(sys.stdin).get("config",{}); print(c.get("image",{}).get("Oci",{}).get("reference",""))' 2>/dev/null; }
sandbox_names() { msb list 2>/dev/null | awk 'NR>1{print $1}'; }
no_build_sandbox() { ! sandbox_names | grep -q '^silo-rebuild-'; }
populate() {
  inside "$1" 'echo "workspace-payload-12345" > /workspace/testfile.txt && mkdir -p /home/silo/.config && echo "config-payload-67890" > /home/silo/.config/test.conf' >/dev/null
  root "$1" 'mkdir -p /opt/e2e && echo old-root-only > /opt/e2e/outside.txt' >/dev/null
}
data_intact() {
  local ws home
  ws=$(inside "$1" 'cat /workspace/testfile.txt 2>/dev/null' | tr -d '\r')
  home=$(inside "$1" 'cat /home/silo/.config/test.conf 2>/dev/null' | tr -d '\r')
  [ "$ws" = "workspace-payload-12345" ] && [ "$home" = "config-payload-67890" ]
}

cleanup() {
  say "cleanup (drops only the e2e-t15-* computers this run made, then stops the Dev engine it started)"
  if engine_answers; then
    for c in $ALL; do silo stop "$c" >/dev/null 2>&1; done
    for round in 1 2 3; do
      for c in $ALL; do exists "$c" && silo drop "$c" --force >/dev/null 2>&1; done
      sleep 2
    done
    say "silo list after cleanup:"; silo list
  fi
  stop_engine && say "Dev engine stopped" || say "Dev engine did not stop in time"
  rm -rf "$WORK"
}
trap cleanup EXIT

say "== header"
echo "commit $(cd "$REPO" && git rev-parse --short HEAD) on $(cd "$REPO" && git branch --show-current); $("$SILO_BIN" --version 2>&1); host $(sw_vers -productVersion) $(uname -m); $(date -u +%FT%TZ)"
if engine_answers && [ -z "$(our_engine)" ]; then
  echo "A Dev engine this script did not start is answering; refusing to touch it." >&2
  trap - EXIT; rm -rf "$WORK"; exit 2
fi
[ -n "$(our_engine)" ] || { start_engine; check "the Dev engine starts from the bundle" $? "pid $(our_engine), log $LOG"; }
[ -x "$MSB" ] || { echo "No msb found (set MSB) to read image references." >&2; exit 2; }
for c in $ALL; do exists "$c" && { echo "$c already exists; refusing to run" >&2; exit 2; }; done

say "== S0 every call goes to the Dev socket of our engine"
check "the socket is ~/.silo-dev/engine.sock" $([ "$SOCK" = "$HOME/.silo-dev/engine.sock" ] && [ -S "$SOCK" ]; echo $?) "$SOCK"
holders=$(lsof -t "$SOCK" 2>/dev/null | tr '\n' ' ')
check "the socket is held by the engine this script started" $(echo " $holders " | grep -q " $(our_engine) "; echo $?) "engine pid $(our_engine), socket holders: $holders"
echo "computers before: $(silo list 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"

say "== S1 create a running Linux computer; write data to /workspace, /home/silo and outside them"
silo create $A --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $A" $? ""
silo start $A >/dev/null 2>&1; check "start $A" $? ""
wait_for 120 silo exec $A -- echo ready
populate $A; check "write the marker data" $? ""
IMAGE_BEFORE=$(image_of $A)
check "the image reference of $A is readable" $([ -n "$IMAGE_BEFORE" ]; echo $?) "$IMAGE_BEFORE"

say "== S2 a computer already on the newest image is refused without --force"
out=$(silo rebuild $A 2>&1); rc=$?
check "plain rebuild of a computer on the newest image is refused" $([ $rc -ne 0 ]; echo $?) "$out"
echo "$out" | grep -qi "already on the newest image"; check "the refusal says why" $? ""
check "$A still runs and is unchanged" $([ "$(state_of $A)" = running ] && [ "$(image_of $A)" = "$IMAGE_BEFORE" ]; echo $?) ""
exists $AU; check "no undo computer was made by the refusal" $([ $? -ne 0 ]; echo $?) ""

say "== S3 rebuild with --force; the build sandbox stays out of the list"
( silo --json rebuild $A --force >"$WORK/rebuild-a.json" 2>"$WORK/rebuild-a.err"; echo $? >"$WORK/rebuild-a.rc" ) &
rpid=$!
seen_build=0
while kill -0 $rpid 2>/dev/null; do
  silo list 2>/dev/null | grep -q 'silo-rebuild-' && seen_build=1
  sleep 2
done
wait $rpid
check "silo rebuild --force $A" $(cat "$WORK/rebuild-a.rc") "$(tail -c 300 "$WORK/rebuild-a.err")"
check "the build sandbox never showed in silo list" $seen_build ""
NEWEST=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("image",""))' "$WORK/rebuild-a.json" 2>/dev/null)
IMAGE_AFTER=$(image_of $A)
echo "image before: $IMAGE_BEFORE"; echo "image after:  $IMAGE_AFTER"; echo "newest:       $NEWEST"
check "the rebuilt computer is on the newest image" $([ -n "$NEWEST" ] && [ "$IMAGE_AFTER" = "$NEWEST" ]; echo $?) ""
check "the rebuilt computer is running again" $([ "$(state_of $A)" = running ]; echo $?) ""
no_build_sandbox; check "no build sandbox is left in the runtime" $? "$(sandbox_names | tr '\n' ' ')"

say "== S4 workspace and home are intact; what lived outside them is gone from the rebuilt computer"
data_intact $A; check "workspace and home files are intact" $? ""
out=$(root $A 'cat /opt/e2e/outside.txt 2>&1')
echo "$out" | grep -q old-root-only; check "the file outside the home folder is not carried" $([ $? -ne 0 ]; echo $?) "$out"

say "== S5 the undo computer exists, is stopped, and keeps the old data"
check "$AU exists and is stopped" $([ "$(state_of $AU)" = stopped ]; echo $?) "state: $(state_of $AU)"
check "$AU has the old image" $([ "$(image_of $AU)" = "$IMAGE_BEFORE" ]; echo $?) "$(image_of $AU)"
silo start $AU >/dev/null 2>&1; wait_for 120 silo exec $AU -- echo ready
data_intact $AU; check "$AU keeps the old workspace and home data" $? ""
out=$(root $AU 'cat /opt/e2e/outside.txt 2>&1')
echo "$out" | grep -q old-root-only; check "$AU keeps the file outside the home folder" $? "$out"
silo stop $AU >/dev/null 2>&1

say "== S6 a second rebuild is refused because the undo computer exists"
out=$(silo rebuild $A --force 2>&1); rc=$?
check "second rebuild is refused" $([ $rc -ne 0 ]; echo $?) "$out"
echo "$out" | grep -qi "already exists"; check "the refusal says the undo computer exists" $? ""

say "== S7 a stopped computer is rebuilt and stays stopped"
silo create $B --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $B" $? ""
silo start $B >/dev/null 2>&1; wait_for 120 silo exec $B -- echo ready
populate $B; silo stop $B >/dev/null 2>&1
check "$B is stopped before the rebuild" $([ "$(state_of $B)" = stopped ]; echo $?) ""
out=$(silo rebuild $B --force 2>&1); check "silo rebuild --force $B (stopped)" $? "$out"
check "$B is still stopped" $([ "$(state_of $B)" = stopped ]; echo $?) "state: $(state_of $B)"
check "$BU is stopped" $([ "$(state_of $BU)" = stopped ]; echo $?) "state: $(state_of $BU)"
silo start $B >/dev/null 2>&1; wait_for 120 silo exec $B -- echo ready
data_intact $B; check "$B keeps its workspace and home data" $? ""

say "== S8 the engine is killed in the middle of a rebuild"
silo create $C --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $C" $? ""
silo start $C >/dev/null 2>&1; wait_for 120 silo exec $C -- echo ready
populate $C
( silo rebuild $C --force >"$WORK/rebuild-c.out" 2>&1 ) &
cpid=$!
step=""
for _ in $(seq 1 240); do
  journal=$(ls "$ENGINE_HOME"/rebuild/*.rebuild.json 2>/dev/null | head -1)
  if [ -n "$journal" ]; then
    step=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("step",""))' "$journal" 2>/dev/null)
    case "$step" in home_carried|workspace_moved|recipe_replayed|swapping) break;; esac
  fi
  sleep 1
done
echo "journal step when the engine is killed: ${step:-none}"
check "the rebuild reached a step worth interrupting" $([ -n "$step" ]; echo $?) "step: $step"
kill -KILL "$(our_engine)" 2>/dev/null
wait $cpid 2>/dev/null
sleep 2
start_engine; check "the Dev engine restarts" $? "pid $(our_engine)"
gone=1; for _ in $(seq 1 120); do ls "$ENGINE_HOME"/rebuild/*.rebuild.json >/dev/null 2>&1 || { gone=0; break; }; sleep 2; done
check "the interrupted rebuild is settled (its journal is gone)" $gone ""
exists $C; check "$C still exists under its own name" $? ""
if exists $CU; then echo "outcome: finished (the swap had begun); $CU exists"; else echo "outcome: rolled back; no $CU"; fi
no_build_sandbox; check "no build sandbox is left" $? "$(sandbox_names | tr '\n' ' ')"
check "no staged home folder is left" $([ -z "$(ls "$ENGINE_HOME"/rebuild/stage 2>/dev/null)" ]; echo $?) "$(ls "$ENGINE_HOME"/rebuild/stage 2>/dev/null)"
silo start $C >/dev/null 2>&1; wait_for 180 silo exec $C -- echo ready
data_intact $C; check "$C keeps its workspace and home data" $? ""
if exists $CU; then check "$CU is stopped" $([ "$(state_of $CU)" = stopped ]; echo $?) "state: $(state_of $CU)"; fi

say "== finished"
echo "PASS: $PASS  FAIL: $FAILN"
[ "$FAILN" = 0 ]
