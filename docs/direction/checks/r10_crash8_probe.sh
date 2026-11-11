#!/usr/bin/env bash
# R10 live probe: the engine is killed in the middle of a rebuild's swap, and the first command into
# the rebuilt computer must answer promptly after the restart. Dev engine only (~/.silo-dev),
# one throwaway computer e2e-r10-k. Run:
#   bash docs/direction/checks/r10_crash8_probe.sh 2>&1 | tee app/SiloUI/target/verification/r10/probe.txt
# T=<folder> uses another build folder (debug/silo and debug/bundle/macos/Silo Dev.app inside it).
# STEP and DELAY choose the kill point (default: the journal shows "swapping", plus 2.0 s); WHEN=starting
# waits after that until the runtime lists e2e-r10-k as starting.
# KEEP_ON_FAIL=1 leaves the engine and the computers in place when a check fails.
# ORPHANS=stop (default) stops the computers' runtime processes after the kill, as t15 does;
# ORPHANS=keep leaves them as a crashed engine would.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=${T:-$REPO/app/SiloUI/target}
SILO_BIN=$T/debug/silo
BUNDLE="$T/debug/bundle/macos/Silo Dev.app"
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
MSB="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/msb"
LOG=${LOG:-$REPO/app/SiloUI/target/verification/silod-dev-r10.log}
SOCK="$HOME/.silo-dev/engine.sock"
REBUILD_DIR="$HOME/Library/Application Support/org.silo.dev/rebuild"
STEP=${STEP:-swapping}; DELAY=${DELAY:-2.0}; ORPHANS=${ORPHANS:-stop}
K=e2e-r10-k; KU=$K-undo
silo() { "$SILO_BIN" --socket "$SOCK" "$@"; }
say() { echo "[$(date +%H:%M:%S)] $*"; }
PASS=0; FAILN=0
check() { if [ "$2" = 0 ]; then say "PASS  $1  -- ${3:-}"; PASS=$((PASS+1)); else say "FAIL  $1  -- ${3:-}"; FAILN=$((FAILN+1)); fi; }
our_engine() { pgrep -f "^$SILOD --agent" | head -1; }
engine_answers() { silo list >/dev/null 2>&1; }
start_engine() { mkdir -p "$(dirname "$LOG")"; "$SILOD" --agent >>"$LOG" 2>&1 & for _ in $(seq 1 120); do engine_answers && return 0; sleep 1; done; return 1; }
stop_engine() { local pid; pid=$(our_engine); [ -n "$pid" ] || return 0; kill -TERM "$pid"; for _ in $(seq 1 180); do kill -0 "$pid" 2>/dev/null || return 0; sleep 1; done; return 1; }
RH=""
msb() { MSB_HOME=$RH MSB_PATH="$MSB" "$MSB" "$@"; }
mine() { msb list 2>&1 | grep -E '^(NAME|e2e-r10-|silo-rebuild-)'; }
wait_for() { local limit=$1; shift; local end=$((SECONDS+limit)); while [ $SECONDS -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 3; done; return 1; }
journals() { ls "$REBUILD_DIR"/*.rebuild.json 2>/dev/null; }
cleanup() {
  if [ "${KEEP_ON_FAIL:-}" = 1 ] && [ "$FAILN" -gt 0 ]; then say "a check failed: the Dev engine and e2e-r10-k are left for inspection (KEEP_ON_FAIL=1)"; return; fi
  say "cleanup"
  if engine_answers; then
    for n in $(msb list 2>/dev/null | awk 'NR>1 && /running|starting/ {print $1}' | grep -E '^(e2e-r10-|silo-rebuild-)'); do msb stop "$n" >/dev/null 2>&1; done
    silo stop $K $KU >/dev/null 2>&1
    for _ in 1 2 3; do for c in $K $KU; do silo status "$c" >/dev/null 2>&1 && silo drop "$c" --force >/dev/null 2>&1; done; sleep 2; done
    for left in $(msb list 2>/dev/null | awk 'NR>1{print $1}' | grep -E '^(silo-rebuild-|e2e-r10-)'); do msb rm --force "$left" >/dev/null 2>&1; done
    say "silo list after cleanup:"; silo list
  fi
  stop_engine && say "Dev engine stopped" || say "Dev engine did not stop in time"
}
trap cleanup EXIT
say "== header: $(git -C "$REPO" rev-parse --short HEAD) $(git -C "$REPO" branch --show-current); $("$SILO_BIN" --version); kill at '$STEP' + ${DELAY}s; orphans: $ORPHANS"
if engine_answers && [ -z "$(our_engine)" ]; then echo "A Dev engine this script did not start answers; refusing." >&2; trap - EXIT; exit 2; fi
[ -n "$(our_engine)" ] || { start_engine; check "the Dev engine starts" $? "pid $(our_engine)"; }
RH=$(ls -d "$HOME"/.silo-dev/*/sandboxes 2>/dev/null | head -1 | xargs -I{} dirname {})
silo status $K >/dev/null 2>&1 && { echo "$K already exists; refusing" >&2; trap - EXIT; exit 2; }
S=$SECONDS; silo create $K --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $K" $? "$((SECONDS-S))s"
silo start $K >/dev/null 2>&1; wait_for 120 silo exec $K -- echo ready
silo exec $K --timeout 60 -- sudo -u silo -H bash -c 'echo workspace-payload > /workspace/testfile.txt' >/dev/null 2>&1
KID=$(silo --json status $K 2>/dev/null | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("id") or v.get("configuration",{}).get("id",""))')
( silo rebuild $K --force >/dev/null 2>&1 ) & CP=$!
python3 - "$REBUILD_DIR" "$STEP" "$DELAY" "$(our_engine)" "${WHEN:-}" "$MSB" "$RH" <<'PY'
import glob, json, os, signal, subprocess, sys, time
home, step, delay, pid, when, msb, rh = sys.argv[1], sys.argv[2], float(sys.argv[3]), int(sys.argv[4]), sys.argv[5], sys.argv[6], sys.argv[7]
def listed():
    env = dict(os.environ, MSB_HOME=rh, MSB_PATH=msb)
    return subprocess.run([msb, "list"], env=env, capture_output=True, text=True).stdout
end = time.time() + 240
while time.time() < end:
    for path in glob.glob(os.path.join(home, "*.rebuild.json")):
        try:
            seen = json.load(open(path)).get("step", "")
        except Exception:
            continue
        if seen == step:
            time.sleep(delay)
            while when and time.time() < end:
                if any(l.split()[:1] == ["e2e-r10-k"] and when in l for l in listed().splitlines()):
                    break
            os.kill(pid, signal.SIGKILL)
            print("killed at", seen)
            sys.exit(0)
    time.sleep(0.02)
print("never saw", step)
sys.exit(1)
PY
check "the rebuild reached '$STEP'" $? ""
wait $CP 2>/dev/null; sleep 2
say "sandboxes right after the kill:"; mine
if [ "$ORPHANS" = stop ]; then for n in $(msb list 2>/dev/null | awk 'NR>1 && /running/ {print $1}' | grep -E '^(e2e-r10-|silo-rebuild-)'); do msb stop "$n" >/dev/null 2>&1; done; say "orphans stopped:"; mine; fi
S=$SECONDS; start_engine; check "the engine restarts" $? "$((SECONDS-S))s"
S=$SECONDS; gone=1; for _ in $(seq 1 150); do [ -z "$(journals)" ] && { gone=0; break; }; sleep 1; done
check "the interrupted rebuild is settled (its journal is gone)" $gone "$((SECONDS-S))s"
say "sandboxes after recovery:"; mine
say "silo list:"; silo list
S=$SECONDS; out=$(silo start $K 2>&1); rc=$?; say "silo start $K -> rc $rc after $((SECONDS-S))s: $(echo "$out" | head -3)"
S=$SECONDS; out=$(silo exec $K --timeout 60 -- echo ready 2>&1); rc=$?; D=$((SECONDS-S))
say "first silo exec into $K -> rc $rc after ${D}s: $(echo "$out" | head -3)"
check "the first command into the rebuilt computer answers within 60 s" $([ $rc = 0 ] && [ $D -lt 60 ]; echo $?) "rc $rc, ${D}s"
out=$(silo exec $K --timeout 60 -- sudo -u silo -H bash -c 'cat /workspace/testfile.txt' 2>&1 | sed '/^step [0-9]*$/d')
check "the workspace file is intact" $([ "$out" = workspace-payload ]; echo $?) "$out"
check "the computer keeps its id" $([ "$(silo --json status $K 2>/dev/null | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("id") or v.get("configuration",{}).get("id",""))')" = "$KID" ]; echo $?) "$KID"
say "PASS: $PASS FAIL: $FAILN"
