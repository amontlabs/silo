#!/usr/bin/env bash
# R7 live check: running computers must not keep the engine's command lock after an engine crash.
# Dev engine only (~/.silo-dev), throwaway e2e-r7-* computers. Run:
#   bash docs/direction/checks/r7_lock_check.sh 2>&1 | tee docs/direction/evidence/R7.txt
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=$REPO/app/SiloUI/target
SILO_BIN=$T/debug/silo
BUNDLE="$T/debug/bundle/macos/Silo Dev.app"
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
MSB="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/msb"
SOCK="$HOME/.silo-dev/engine.sock"
LOG=$T/verification/silod-dev-r7.log
silo() { "$SILO_BIN" --socket "$SOCK" "$@"; }
say() { echo "[$(date +%H:%M:%S)] $*"; }
PASS=0; FAILN=0
check() { if [ "$2" = 0 ]; then say "PASS  $1  -- ${3:-}"; PASS=$((PASS+1)); else say "FAIL  $1  -- ${3:-}"; FAILN=$((FAILN+1)); fi; }
our_engine() { pgrep -f "^$SILOD --agent" | head -1; }
ALL="e2e-r7-a e2e-r7-b e2e-r7-c e2e-r7-d e2e-r7-e"
# Starts the engine; prints seconds until the socket answers and seconds until `silo list` succeeds.
start_engine() {
  "$SILOD" --agent >>"$LOG" 2>&1 &
  local s=$SECONDS answered="" last="" out
  while [ $((SECONDS-s)) -lt ${SETTLE_LIMIT:-240} ]; do
    out=$(silo list 2>&1); rc=$?
    if [ -z "$answered" ] && [ -S "$SOCK" ] && { [ $rc = 0 ] || ! echo "$out" | grep -qi 'not running\|connect'; }; then answered=$((SECONDS-s)); fi
    [ "$out" != "$last" ] && [ $rc != 0 ] && { say "  list says: $(echo "$out" | head -1)"; last=$out; }
    [ $rc = 0 ] && { echo "answered=${answered:-?}s list_ok=$((SECONDS-s))s"; return 0; }
    sleep 0.5
  done
  echo "answered=${answered:-never} list_ok=never(>${SETTLE_LIMIT:-240}s)"; return 1
}
cleanup() {
  say "cleanup"
  if silo list >/dev/null 2>&1; then
    for c in $ALL; do silo stop "$c" >/dev/null 2>&1; done
    for r in 1 2; do for c in $ALL; do silo status "$c" >/dev/null 2>&1 && silo drop "$c" --force >/dev/null 2>&1; done; sleep 2; done
    say "silo list after cleanup:"; silo list
  fi
  pid=$(our_engine); [ -n "$pid" ] && kill -TERM "$pid" && for _ in $(seq 1 60); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
  say "Dev engine still running after cleanup: ${pid:+checked} $(our_engine || true)"
}
trap cleanup EXIT
say "== header: commit $(git -C "$REPO" rev-parse --short HEAD) $(git -C "$REPO" branch --show-current); $("$SILO_BIN" --version); $(date -u +%FT%TZ)"
if silo list >/dev/null 2>&1 && [ -z "$(our_engine)" ]; then say "ABORT: a Dev engine not started by this script answers"; exit 2; fi
say "engine start: $(start_engine)"
LOCK=$(find "$HOME/Library/Application Support/org.silo.dev" -name .silo-configuration-worker.lock 2>/dev/null | head -1)
say "lock file: ${LOCK:-NOT FOUND}"
RH=$(ls -d "$HOME"/.silo-dev/*/sandboxes 2>/dev/null | head -1 | xargs -I{} dirname {})
msb() { MSB_HOME=$RH MSB_PATH="$MSB" "$MSB" "$@"; }
holders() { lsof "$LOCK" 2>/dev/null | awk 'NR>1{print $1"("$2")"}' | sort -u | tr '\n' ' '; }
mine() { msb list 2>&1 | grep -E '^(NAME|e2e-r7-)'; }
say "== create two Linux computers, run both"
for c in e2e-r7-a e2e-r7-b; do S=$SECONDS; silo create "$c" >/dev/null 2>&1; check "create $c" $? "$((SECONDS-S))s"; done
for c in e2e-r7-a e2e-r7-b; do S=$SECONDS; silo start "$c" >/dev/null 2>&1; check "start $c" $? "$((SECONDS-S))s"; done
mine; say "lock holders at rest: ${holders:-none}"

crash() { # $1 label, $2.. command to run in the background while the engine is killed
  local label=$1; shift
  say "== crash scenario: $label"
  local EPID; EPID=$(our_engine)
  ( "$@" >/dev/null 2>&1 ) & local CP=$!
  sleep ${KILL_AFTER:-3}
  say "lock holders mid-command: $(holders)"
  kill -KILL "$EPID"; say "SIGKILL sent to engine $EPID"; sleep 1; wait $CP 2>/dev/null
  mine
  local H; H=$(holders); say "lock holders after the kill: ${H:-none}"
  [ -z "$H" ]; check "$label: no process holds the lock file after the kill" $? "${H:-none}"
  local T0=$SECONDS R; R=$(start_engine); check "$label: engine settles at once" $([ "$R" != "${R#*list_ok=never}" ] && echo 1 || echo 0) "$R"
  silo list
}
# 1: a stop of b is in flight
crash "kill during silo stop e2e-r7-b" silo stop e2e-r7-b
silo status e2e-r7-b | head -3
S=$SECONDS; silo create e2e-r7-d >/dev/null 2>&1; check "create after the crash" $? "$((SECONDS-S))s"
S=$SECONDS; silo stop e2e-r7-b >/dev/null 2>&1; check "stop after the crash" $? "$((SECONDS-S))s"
S=$SECONDS; silo start e2e-r7-b >/dev/null 2>&1; check "start after the crash" $? "$((SECONDS-S))s"
# 2: a create of another computer is in flight
KILL_AFTER=4 crash "kill during silo create e2e-r7-c" silo create e2e-r7-c
S=$SECONDS; silo create e2e-r7-e >/dev/null 2>&1; check "create after the second crash" $? "$((SECONDS-S))s"
S=$SECONDS; silo stop e2e-r7-a >/dev/null 2>&1; check "stop the computer that ran through both crashes" $? "$((SECONDS-S))s"
say "== Quit (TERM to the engine)"
S=$SECONDS; kill -TERM "$(our_engine)"; for _ in $(seq 1 240); do pgrep -f "^$SILOD --agent" >/dev/null || break; sleep 0.5; done
pgrep -f "^$SILOD --agent" >/dev/null; check "Quit finishes" $([ $? = 0 ] && echo 1 || echo 0) "$((SECONDS-S))s"
say "restart for cleanup: $(start_engine)"
say "PASS: $PASS FAIL: $FAILN"
