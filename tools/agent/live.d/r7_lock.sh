# R7 live check, ported from docs/direction/checks/r7_lock_check.sh (not yet run through `live`):
# running computers must not keep the engine's command lock after an engine crash.
# Dev engine only (~/.silo-dev), throwaway computers named by live_computer_named.
#   tools/agent/live r7_lock [--from STEP]      T=<build folder> holds debug/silo and the Dev bundle
CHECK_DESC="engine crash must not leave computers holding the command lock (Dev engine, macOS)"
STEPS=(preflight engine create crash_stop crash_create quit)

T=${T:-$REPO/app/SiloUI/target}
SILO_BIN=$T/debug/silo
BUNDLE="$T/debug/bundle/macos/Silo Dev.app"
ENGINE_DIR="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS"
SILOD="$ENGINE_DIR/silod"; MSB="$ENGINE_DIR/msb"
SOCK=$(live_dev_sock)
silo() { "$SILO_BIN" --socket "$SOCK" "$@"; }

our_engine() { live_our_engine "$SILOD"; }
# Starts the engine; prints seconds until the socket answers and until `silo list` succeeds.
start_engine() {
  "$SILOD" --agent >>"$LOGS/silod-dev.log" 2>&1 &
  local s=$SECONDS answered="" out rc
  while [ $((SECONDS-s)) -lt "${SETTLE_LIMIT:-240}" ]; do
    out=$(silo list 2>&1); rc=$?
    if [ -z "$answered" ] && [ -S "$SOCK" ] && { [ $rc = 0 ] || ! echo "$out" | grep -qi 'not running\|connect'; }; then answered=$((SECONDS-s)); fi
    [ $rc = 0 ] && { echo "answered=${answered:-?}s list_ok=$((SECONDS-s))s"; return 0; }
    sleep 0.5
  done
  echo "answered=${answered:-never} list_ok=never(>${SETTLE_LIMIT:-240}s)"; return 1
}
ensure_engine() { silo list >/dev/null 2>&1 || start_engine; }
lock_file() { find "$HOME/Library/Application Support/org.silo.dev" -name .silo-configuration-worker.lock 2>/dev/null | head -1; }
holders() { lsof "$(lock_file)" 2>/dev/null | awk 'NR>1{print $1"("$2")"}' | sort -u | tr '\n' ' '; }
mine() {
  local rh; rh=$(ls -d "$HOME"/.silo-dev/*/sandboxes 2>/dev/null | head -1 | xargs -I{} dirname {})
  MSB_HOME=$rh MSB_PATH="$MSB" "$MSB" list 2>&1 | grep -E "^(NAME|e2e-r7-)"
}

# Kills the engine while "$@" runs; the lock must be free afterwards and the engine must settle.
crash() { # $1 label, $2.. command run in the background during the kill
  local label=$1 H R; shift
  live_say "== crash scenario: $label"
  local epid; epid=$(our_engine)
  ( "$@" >/dev/null 2>&1 ) & local cp=$!
  sleep "${KILL_AFTER:-3}"
  live_say "lock holders mid-command: $(holders)"
  kill -KILL "$epid"; live_say "SIGKILL sent to engine $epid"; sleep 1; wait $cp 2>/dev/null
  mine; H=$(holders); live_say "lock holders after the kill: ${H:-none}"
  [ -z "$H" ]; live_check "$label: no process holds the lock file after the kill" $? "${H:-none}"
  R=$(start_engine); live_check "$label: engine settles at once" $([ "$R" != "${R#*list_ok=never}" ] && echo 1 || echo 0) "$R"
  silo list
}
timed() { local what=$1 s=$SECONDS; shift; "$@" >/dev/null 2>&1; live_check "$what" $? "$((SECONDS-s))s"; }

step_preflight() {
  live_need_exec "$SILO_BIN" "debug silo of the build under test (T=)"
  live_need_exec "$SILOD" "Dev bundle"; live_need_exec "$MSB"
  live_assert_dev_sock "$SOCK"; live_check_sock_path "$SOCK"
  live_need_path "$HOME/Library/Application Support/org.silo.dev/guest-image" "Dev guest images"
  for c in a b c d e; do live_computer_named "$c" >/dev/null; done
  live_say "header: commit $(git -C "$REPO" rev-parse --short HEAD); $("$SILO_BIN" --version)"
  if silo list >/dev/null 2>&1 && [ -z "$(our_engine)" ]; then live_die "a Dev engine not started by this check answers"; fi
}
step_engine() { live_say "engine start: $(start_engine)"; live_set engine_pid "$(our_engine)"; lock_file | grep . || live_die "no lock file found"; }
step_create() {
  ensure_engine
  timed "create a" silo create "$(live_computer_named a)"; timed "create b" silo create "$(live_computer_named b)"
  timed "start a" silo start "$(live_computer_named a)"; timed "start b" silo start "$(live_computer_named b)"
  mine; live_say "lock holders at rest: $(holders)"
}
step_crash_stop() {
  ensure_engine
  crash "kill during silo stop (b)" silo stop "$(live_computer_named b)"
  silo status "$(live_computer_named b)" | head -3
  timed "create after the crash" silo create "$(live_computer_named d)"
  timed "stop after the crash" silo stop "$(live_computer_named b)"
  timed "start after the crash" silo start "$(live_computer_named b)"
}
step_crash_create() {
  ensure_engine
  KILL_AFTER=${CREATE_KILL_AFTER:-4} crash "kill during silo create (c)" silo create "$(live_computer_named c)"
  timed "create after the second crash" silo create "$(live_computer_named e)"
  timed "stop the computer that ran through both crashes" silo stop "$(live_computer_named a)"
}
step_quit() {
  ensure_engine
  local s=$SECONDS; kill -TERM "$(our_engine)"
  for _ in $(seq 1 240); do [ -z "$(our_engine)" ] && break; sleep 0.5; done
  live_check "Quit finishes" $([ -z "$(our_engine)" ] && echo 0 || echo 1) "$((SECONDS-s))s"
}

# Runs on every exit: stop and drop the throwaway computers, then stop the engine this check started.
cleanup() {
  [ -x "$SILO_BIN" ] || return 0
  if silo list >/dev/null 2>&1; then
    for c in $(live_computers); do silo stop "$c" >/dev/null 2>&1; done
    for _ in 1 2; do for c in $(live_computers); do silo status "$c" >/dev/null 2>&1 && silo drop "$c" --force >/dev/null 2>&1; done; sleep 2; done
    silo list
  fi
  local pid; pid=$(our_engine)
  [ -n "$pid" ] && kill -TERM "$pid" && for _ in $(seq 1 60); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
}
