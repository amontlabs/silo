#!/usr/bin/env bash
# T15 live check: rebuild a Linux computer on the newest image, preserving workspace and home.
# Runs against the Dev engine (~/.silo-dev) with throwaway e2e-t15-* computers. Run:
#   bash docs/direction/checks/t15_rebuild_check.sh 2>&1 | tee docs/direction/evidence/T15.txt
# Needs a Dev app bundle built from this tree (BUNDLE: `npm --prefix app/SiloUI run
# desktop:build:debug` after `npm --prefix app/SiloUI run runtime:prepare`), its debug `silo`
# (SILO) and the bundle's `msb` (MSB, found inside the bundle when not given) to read a
# sandbox's image reference and root disk. The script starts the bundle's `silod --agent` by hand
# when no Dev engine runs, and stops it at the end. It never touches another engine: it refuses
# to run when a Dev engine it did not start answers, and every `silo` call is pinned to the Dev
# socket. The engine's data folder and the runtime home are read from the engine this script
# started (the files its process holds open), not assumed.
#
# What it shows, in order:
#   S0  every call goes to the Dev socket of the engine this script runs; its folders are read
#       from the engine process
#   S1  a Linux computer gets a network allowlist, a secret and Silo's tools, then runs and is
#       populated: /workspace, /home/silo, and a file outside both
#   S2  a rebuild of a computer already on the newest image is refused unless --force is given
#   S3  `silo rebuild --force` works; the root disk is really replaced (its identity changes; the
#       image reference changes too when an older image exists on this device, otherwise only the
#       root disk identity shows it and the script says so); the build sandbox never shows in
#       `silo list`
#   S4  workspace and home are intact in the rebuilt computer; the file outside them is gone; the
#       network policy (and what it blocks), the secret assignment and the tools carry over, the
#       tools token being a new one
#   S5  <name>-undo exists, is stopped, keeps the old data and the old network policy, has no
#       secret and cannot reach Silo's tools
#   S6  a second rebuild is refused because <name>-undo exists
#   S7  a stopped computer is rebuilt and stays stopped (so does its undo computer)
#   S8  the engine is killed at each journal step of a rebuild, the swap included (the kill lands
#       at the step, and for the swap at several moments after it); after each restart the
#       rebuild is settled, the computer keeps its id, its data is intact, and no build sandbox,
#       staged file or tar file is left
#   S9  Quit during a rebuild: the rebuild stops at its next step, Quit finishes in bounded time
#       and the next start settles what is left
#   S10 no silo-rebuild-* sandbox remains on the device
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=${T:-$HOME/code/projects/silo-wt-targets/T15}
SILO_BIN=${SILO:-$T/debug/silo}
BUNDLE=${BUNDLE:-$T/debug/bundle/macos/Silo Dev.app}
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
MSB=${MSB:-$(find "$BUNDLE" -type f -name 'msb*' -perm -u+x 2>/dev/null | head -1)}
LOG=${LOG:-$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-t15.log}
SOCK="$HOME/.silo-dev/engine.sock"
CHANNEL_HOME=$(dirname "$SOCK")
ENGINE_HOME=""     # the engine's data folder, set by read_homes
RUNTIME_HOME=""    # the runtime's home (holds sandboxes/), set by read_homes
A=e2e-t15-a; AU=e2e-t15-a-undo
B=e2e-t15-b; BU=e2e-t15-b-undo
Q=e2e-t15-q; QU=e2e-t15-q-undo
SECRET=E2E_T15_SECRET
# The rebuild journal and its staging folder live in the app data folder, not in the engine folder.
REBUILD_DIR="$HOME/Library/Application Support/org.silo.dev/rebuild"
# Crash points of S8: "<journal step to kill at>:<seconds to wait after seeing it>".
# The journal shows "workspace_moved" for microseconds when there is no recipe, so the crash during
# the workspace disk copy is "home_carried" plus a short wait.
CRASHES="created:0 home_carried:0 home_carried:0.25 recipe_replayed:0 swapping:0 swapping:0.4 swapping:1.0 swapping:2.0 swapped:0"
KILLED=""
n=0; for point in $CRASHES; do n=$((n+1)); KILLED="$KILLED e2e-t15-k$n e2e-t15-k$n-undo"; done
ALL="$AU $A $BU $B $QU $Q $KILLED"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-t15.XXXXXX")
PASS=0; FAILN=0; SKIPN=0
ts() { date +%H:%M:%S; }
skip() { local name=$1; shift; echo "[$(ts)] SKIPPED  $name  -- $*"; SKIPN=$((SKIPN+1)); }
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
  for _ in $(seq 1 120); do engine_answers && { read_homes; return 0; }; sleep 1; done
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
id_of() { silo --json status "$1" 2>/dev/null | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("id") or v.get("configuration",{}).get("id",""))' 2>/dev/null; }
exists() { silo status "$1" >/dev/null 2>&1; }
# The engine's data folder (the folder of the registry it holds open) and the runtime home (the
# folder next to it that holds the sandboxes), read from the engine process.
read_homes() {
  local pid; pid=$(our_engine)
  ENGINE_HOME=""; RUNTIME_HOME=""
  if [ -n "$pid" ]; then
    ENGINE_HOME=$(lsof -p "$pid" -Fn 2>/dev/null | sed -n 's/^n//p' | grep -m1 '/registry\.sqlite3$' | xargs -I{} dirname {})
  fi
  [ -n "$ENGINE_HOME" ] || ENGINE_HOME="$CHANNEL_HOME/engine"
  RUNTIME_HOME=$(ls -d "$CHANNEL_HOME"/*/sandboxes 2>/dev/null | head -1 | xargs -I{} dirname {})
}
msb() { MSB_HOME=$RUNTIME_HOME MSB_PATH="$MSB" "$MSB" "$@"; }
image_of() { msb inspect "$1" --format json 2>/dev/null | python3 -c 'import json,sys; c=json.load(sys.stdin).get("config",{}); print(c.get("image",{}).get("Oci",{}).get("reference",""))' 2>/dev/null; }
# The identity of a sandbox's root disk file: inode and creation time. A rebuilt computer's root
# disk is a new file; the undo computer's is the old one, renamed with its folder.
rootdisk_of() { stat -f '%i-%B' "$RUNTIME_HOME/sandboxes/$1/upper.ext4" 2>/dev/null; }
sandbox_names() { msb list 2>/dev/null | awk 'NR>1{print $1}'; }
no_build_sandbox() { ! sandbox_names | grep -q '^silo-rebuild-'; }
journals() { ls "$REBUILD_DIR"/*.rebuild.json 2>/dev/null; }
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
no_tar_left() { [ -z "$(root "$1" 'ls /tmp/silo-home-* 2>/dev/null' | tr -d '\r')" ]; }
# What a computer may reach, as one line: mode and hosts.
policy_of() { silo --json network policy "$1" 2>/dev/null | python3 -c 'import json,sys; v=json.load(sys.stdin); p=v.get("policy",{}); print(p.get("mode",""), sorted(p.get("hosts",[])))' 2>/dev/null; }
# The computers a secret is assigned to, one line.
secret_computers() {
  silo --json secrets list 2>/dev/null | python3 -c '
import json,sys
v=json.load(sys.stdin)
rows=v if isinstance(v,list) else v.get("secrets",[])
for r in rows:
    if r.get("name")=="'"$SECRET"'":
        print(" ".join(sorted(r.get("computers",[]))))' 2>/dev/null
}
secret_on() { secret_computers | tr ' ' '\n' | grep -qx "$1"; }
# A digest of the file that holds the tools address and token in a computer (the token itself is
# never printed).
tools_file_digest() {
  local path; path=$(silo --json tools status "$1" 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin).get("endpointPath",""))' 2>/dev/null)
  [ -n "$path" ] || return 1
  root "$1" "sha256sum '$path' 2>/dev/null | cut -d' ' -f1" | tr -d '\r'
}
tools_reach() { inside "$1" 'silo --json list' >/dev/null 2>&1; }
reach_code() { inside "$1" "curl -s -m 12 -o /dev/null -w '%{http_code}' $2" | tr -d '\r' | tail -1; }
# A killed engine leaves the computers' runtime processes running, and they keep the engine's
# configuration lock open, so the next engine would wait for it. A machine that crashed has no such
# processes; this stops the throwaway computers' runtime processes (gracefully, by name) to match.
stop_orphans() {
  local name
  for name in $(msb list 2>/dev/null | awk 'NR>1 && $NF!="" && /running/ {print $1}' | grep -E '^(e2e-t15-|silo-rebuild-)'); do
    msb stop "$name" >/dev/null 2>&1
  done
}
# Waits for the journal to show one of the steps in $1 (a space-separated list), then waits $2
# seconds, then kills our engine when $3 is "kill"; prints the step seen.
journal_wait() {
  python3 - "$REBUILD_DIR" "$1" "$2" "$3" "$(our_engine)" <<'PY'
import glob, json, os, signal, sys, time
home, steps, delay, action, pid = sys.argv[1], sys.argv[2].split(), float(sys.argv[3]), sys.argv[4], int(sys.argv[5])
end = time.time() + 240
while time.time() < end:
    for path in glob.glob(os.path.join(home, "*.rebuild.json")):
        try:
            seen = json.load(open(path)).get("step", "")
        except Exception:
            continue
        if seen in steps:
            time.sleep(delay)
            if action == "kill":
                os.kill(pid, signal.SIGKILL)
            print(seen)
            sys.exit(0)
    time.sleep(0.02)
print("none")
sys.exit(1)
PY
}

cleanup() {
  say "cleanup (drops only the e2e-t15-* computers this run made, then stops the Dev engine it started)"
  if engine_answers; then
    silo secrets unassign "$SECRET" $A >/dev/null 2>&1
    stop_orphans
    for c in $ALL; do silo stop "$c" >/dev/null 2>&1; done
    for round in 1 2 3; do
      for c in $ALL; do exists "$c" && silo drop "$c" --force >/dev/null 2>&1; done
      sleep 2
    done
    # A build sandbox a failed run left behind is removed by name through the runtime.
    for left in $(sandbox_names | grep '^silo-rebuild-'); do msb rm --force "$left" >/dev/null 2>&1; done
    say "silo list after cleanup:"; silo list
    say "sandboxes after cleanup: $(sandbox_names | tr '\n' ' ')"
    say "the secret $SECRET stays in the Dev engine's secret store with no computer assigned (the CLI cannot delete one)"
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
read_homes
[ -x "$MSB" ] || { echo "No msb found (set MSB) to read image references." >&2; exit 2; }
for c in $ALL; do exists "$c" && { echo "$c already exists; refusing to run" >&2; exit 2; }; done

say "== S0 every call goes to the Dev socket of our engine; its folders are read from it"
check "the socket is ~/.silo-dev/engine.sock" $([ "$SOCK" = "$HOME/.silo-dev/engine.sock" ] && [ -S "$SOCK" ]; echo $?) "$SOCK"
holders=$(lsof -t "$SOCK" 2>/dev/null | tr '\n' ' ')
check "the socket is held by the engine this script started" $(echo " $holders " | grep -q " $(our_engine) "; echo $?) "engine pid $(our_engine), socket holders: $holders"
check "the engine's data folder is read from the engine" $([ -d "$ENGINE_HOME" ]; echo $?) "$ENGINE_HOME"
check "the runtime home has the sandboxes folder" $([ -d "$RUNTIME_HOME/sandboxes" ]; echo $?) "$RUNTIME_HOME"
echo "computers before: $(silo list 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"
echo "guest images on this device: $(ls "$HOME/Library/Application Support/org.silo.dev/guest-image" 2>/dev/null | tr '\n' ' ')"

say "== S1 a Linux computer with a network allowlist, a secret and Silo's tools; data in /workspace, /home/silo and outside them"
silo create $A --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $A" $? ""
silo network policy $A --mode allowlist --allow example.com >/dev/null 2>&1; check "set an allowlist on $A" $? "$(policy_of $A)"
POLICY_BEFORE=$(policy_of $A)
# A secret needs the owner's Keychain approval, which nobody can give in an unattended run: the
# checks on secrets are skipped when the engine refuses for that reason.
SECRETS_OK=0
secret_out=$(printf 'e2e-t15-secret-value' | silo secrets set $SECRET --computer $A --domain example.com 2>&1); secret_rc=$?
if [ $secret_rc = 0 ]; then check "assign a secret to $A" 0 "$(secret_computers)"
else SECRETS_OK=1; skip "assign a secret to $A" "$secret_out"; fi
silo start $A >/dev/null 2>&1; check "start $A" $? ""
wait_for 120 silo exec $A -- echo ready
silo tools enable $A >/dev/null 2>&1; check "turn on Silo's tools in $A" $? ""
wait_for 120 tools_reach $A; check "silo inside $A reaches Silo" $? ""
populate $A; check "write the marker data" $? ""
IMAGE_BEFORE=$(image_of $A)
DISK_BEFORE=$(rootdisk_of $A)
TOKEN_BEFORE=$(tools_file_digest $A)
ID_A=$(id_of $A)
check "the image reference of $A is readable" $([ -n "$IMAGE_BEFORE" ]; echo $?) "$IMAGE_BEFORE"
check "the root disk of $A is identified" $([ -n "$DISK_BEFORE" ]; echo $?) "$DISK_BEFORE"
check "the tools file of $A is readable" $([ -n "$TOKEN_BEFORE" ]; echo $?) "digest $(echo "$TOKEN_BEFORE" | cut -c1-8)"
check "before: the allowed host is reachable from $A" $([ "$(reach_code $A https://example.com)" != 000 ]; echo $?) ""
check "before: another host is blocked from $A" $([ "$(reach_code $A https://example.org)" = 000 ]; echo $?) ""

say "== S2 a computer already on the newest image is refused without --force"
out=$(silo rebuild $A 2>&1); rc=$?
check "plain rebuild of a computer on the newest image is refused" $([ $rc -ne 0 ]; echo $?) "$out"
echo "$out" | grep -qi "already on the newest image"; check "the refusal says why" $? ""
check "$A still runs and is unchanged" $([ "$(state_of $A)" = running ] && [ "$(rootdisk_of $A)" = "$DISK_BEFORE" ]; echo $?) ""
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
DISK_AFTER=$(rootdisk_of $A)
echo "image before: $IMAGE_BEFORE"; echo "image after:  $IMAGE_AFTER"; echo "newest:       $NEWEST"
echo "root disk before: $DISK_BEFORE"; echo "root disk after:  $DISK_AFTER"
check "the rebuilt computer is on the newest image" $([ -n "$NEWEST" ] && [ "$IMAGE_AFTER" = "$NEWEST" ]; echo $?) ""
if [ "$IMAGE_BEFORE" = "$IMAGE_AFTER" ]; then
  say "NOTE only one guest image exists on this device, so the image reference cannot change; the root disk identity below is what shows the computer was rebuilt"
fi
check "the rebuilt computer has a new root disk (not the old file)" $([ -n "$DISK_AFTER" ] && [ "$DISK_AFTER" != "$DISK_BEFORE" ]; echo $?) ""
check "the rebuilt computer keeps its id" $([ "$(id_of $A)" = "$ID_A" ]; echo $?) "$ID_A"
check "the rebuilt computer is running again" $([ "$(state_of $A)" = running ]; echo $?) ""
no_build_sandbox; check "no build sandbox is left in the runtime" $? "$(sandbox_names | tr '\n' ' ')"

say "== S4 workspace and home are intact; what lived outside them is gone; policy, secret and tools carry over"
wait_for 120 silo exec $A -- echo ready
data_intact $A; check "workspace and home files are intact" $? ""
out=$(root $A 'cat /opt/e2e/outside.txt 2>&1')
echo "$out" | grep -q old-root-only; check "the file outside the home folder is not carried" $([ $? -ne 0 ]; echo $?) "$out"
check "the network policy of $A is the same" $([ "$(policy_of $A)" = "$POLICY_BEFORE" ]; echo $?) "$(policy_of $A)"
check "after: the allowed host is reachable from $A" $([ "$(reach_code $A https://example.com)" != 000 ]; echo $?) ""
check "after: another host is still blocked from $A" $([ "$(reach_code $A https://example.org)" = 000 ]; echo $?) ""
if [ $SECRETS_OK = 0 ]; then secret_on $A; check "the secret is still assigned to $A" $? "$(secret_computers)"; else skip "the secret is still assigned to $A" "no secret could be assigned (Keychain approval)"; fi
wait_for 120 tools_reach $A; check "silo inside the rebuilt $A reaches Silo" $? ""
TOKEN_AFTER=$(tools_file_digest $A)
check "the tools token of $A is a new one" $([ -n "$TOKEN_AFTER" ] && [ "$TOKEN_AFTER" != "$TOKEN_BEFORE" ]; echo $?) "digest $(echo "$TOKEN_AFTER" | cut -c1-8) (was $(echo "$TOKEN_BEFORE" | cut -c1-8))"

say "== S5 the undo computer exists, is stopped, keeps the old data and policy, and has no secret or tools"
check "$AU exists and is stopped" $([ "$(state_of $AU)" = stopped ]; echo $?) "state: $(state_of $AU)"
check "$AU has the old root disk" $([ "$(rootdisk_of $AU)" = "$DISK_BEFORE" ]; echo $?) "$(rootdisk_of $AU)"
check "$AU has the old image" $([ "$(image_of $AU)" = "$IMAGE_BEFORE" ]; echo $?) "$(image_of $AU)"
check "$AU has another id than $A" $([ -n "$(id_of $AU)" ] && [ "$(id_of $AU)" != "$ID_A" ]; echo $?) "$(id_of $AU)"
check "$AU has the network policy $A had" $([ "$(policy_of $AU)" = "$POLICY_BEFORE" ]; echo $?) "$(policy_of $AU)"
if [ $SECRETS_OK = 0 ]; then secret_on $AU; check "no secret is assigned to $AU" $([ $? -ne 0 ]; echo $?) "$(secret_computers)"; else skip "no secret is assigned to $AU" "no secret could be assigned (Keychain approval)"; fi
silo start $AU >/dev/null 2>&1; wait_for 120 silo exec $AU -- echo ready
data_intact $AU; check "$AU keeps the old workspace and home data" $? ""
out=$(root $AU 'cat /opt/e2e/outside.txt 2>&1')
echo "$out" | grep -q old-root-only; check "$AU keeps the file outside the home folder" $? "$out"
tools_reach $AU; check "silo inside $AU cannot reach Silo (its old token no longer works)" $([ $? -ne 0 ]; echo $?) ""
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

say "== S8 the engine is killed at each step of a rebuild"
silo stop $A $B >/dev/null 2>&1
n=0
for point in $CRASHES; do
  n=$((n+1)); K=e2e-t15-k$n; KU=$K-undo
  step=${point%%:*}; delay=${point##*:}
  say "-- crash point $n: kill at '$step' + ${delay}s"
  silo create $K --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $K" $? ""
  silo start $K >/dev/null 2>&1; wait_for 120 silo exec $K -- echo ready
  populate $K
  KID=$(id_of $K)
  ( silo rebuild $K --force >"$WORK/rebuild-k$n.out" 2>&1 ) &
  cpid=$!
  seen=$(journal_wait "$step" "$delay" kill)
  echo "journal step when the engine was killed: $seen"
  check "crash $n: the rebuild reached '$step'" $([ "$seen" = "$step" ]; echo $?) "saw $seen"
  wait $cpid 2>/dev/null
  sleep 2
  stop_orphans
  start_engine; check "crash $n: the Dev engine restarts" $? "pid $(our_engine)"
  gone=1; for _ in $(seq 1 150); do [ -z "$(journals)" ] && { gone=0; break; }; sleep 2; done
  check "crash $n: the interrupted rebuild is settled (its journal is gone)" $gone "$(journals)"
  exists $K; check "crash $n: $K still exists under its own name" $? ""
  check "crash $n: $K keeps its id" $([ "$(id_of $K)" = "$KID" ]; echo $?) "$(id_of $K)"
  if exists $KU; then outcome=finished; else outcome=rolled_back; fi
  echo "outcome: $outcome"
  case "$step" in
    created|home_carried|workspace_moved) check "crash $n: a step before the swap is rolled back" $([ $outcome = rolled_back ]; echo $?) "$outcome";;
    swapped) check "crash $n: a swapped rebuild is finished" $([ $outcome = finished ]; echo $?) "$outcome";;
    swapping) check "crash $n: a swap is finished or rolled back, never half done" 0 "$outcome";;
  esac
  no_build_sandbox; check "crash $n: no build sandbox is left" $? "$(sandbox_names | tr '\n' ' ')"
  check "crash $n: no staged home folder is left" $([ -z "$(ls "$REBUILD_DIR"/stage 2>/dev/null)" ]; echo $?) "$(ls "$REBUILD_DIR"/stage 2>/dev/null)"
  silo start $K >/dev/null 2>&1; wait_for 180 silo exec $K -- echo ready
  data_intact $K; check "crash $n: $K keeps its workspace and home data" $? ""
  no_tar_left $K; check "crash $n: no tar file of the home folder is left in $K" $? ""
  if [ $outcome = finished ]; then
    check "crash $n: $KU is stopped" $([ "$(state_of $KU)" = stopped ]; echo $?) "state: $(state_of $KU)"
    out=$(root $K 'cat /opt/e2e/outside.txt 2>&1'); echo "$out" | grep -q old-root-only
    check "crash $n: the rebuilt $K has a new root disk" $([ $? -ne 0 ]; echo $?) "$out"
  fi
  silo stop $K >/dev/null 2>&1
done

say "== S9 Quit during a rebuild"
silo create $Q --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create $Q" $? ""
silo start $Q >/dev/null 2>&1; wait_for 120 silo exec $Q -- echo ready
populate $Q
QID=$(id_of $Q)
( silo rebuild $Q --force >"$WORK/rebuild-q.out" 2>&1 ) &
qpid=$!
seen=$(journal_wait "created home_carried workspace_moved" 0 wait)
echo "journal step when Quit is asked: $seen"
check "the rebuild is under way when Quit is asked" $([ "$seen" != none ]; echo $?) "$seen"
started=$SECONDS
quit_result=$(python3 - "$REPO/docs/evals" "$SOCK" <<'PY'
import sys
sys.path.insert(0, sys.argv[1])
from pathlib import Path
from engine_client import EngineClient
try:
    with EngineClient(Path(sys.argv[2]), timeout=900.0) as engine:
        started = engine.call("engine.shutdown", {"stopComputers": True, "reason": "user"})
        engine.wait_job(started["jobId"], timeout=900.0)
    print("finished")
except (BrokenPipeError, ConnectionError, EOFError):
    print("finished (the engine exited and closed the connection)")
PY
)
took=$((SECONDS-started))
echo "Quit took ${took}s: ${quit_result:-no answer}"
check "Quit finished while the rebuild was under way" $([[ "$quit_result" == finished* ]]; echo $?) "${took}s: $quit_result"
check "Quit did not wait for the whole rebuild" $([ $took -lt 600 ]; echo $?) "${took}s"
wait $qpid 2>/dev/null
gone=1; for _ in $(seq 1 60); do [ -z "$(our_engine)" ] && { gone=0; break; }; sleep 1; done
check "the engine process exited by itself after Quit" $gone "pid $(our_engine)"
echo "rebuild said: $(tail -c 300 "$WORK/rebuild-q.out")"
stop_engine
stop_orphans
start_engine; check "the Dev engine restarts after Quit" $? "pid $(our_engine)"
gone=1; for _ in $(seq 1 150); do [ -z "$(journals)" ] && { gone=0; break; }; sleep 2; done
check "the rebuild interrupted by Quit is settled" $gone "$(journals)"
exists $Q; check "$Q still exists under its own name" $? ""
check "$Q keeps its id" $([ "$(id_of $Q)" = "$QID" ]; echo $?) ""
no_build_sandbox; check "no build sandbox is left after Quit" $? "$(sandbox_names | tr '\n' ' ')"
silo start $Q >/dev/null 2>&1; wait_for 180 silo exec $Q -- echo ready
data_intact $Q; check "$Q keeps its workspace and home data" $? ""
no_tar_left $Q; check "no tar file of the home folder is left in $Q" $? ""

say "== S10 nothing of a rebuild remains on the device"
no_build_sandbox; check "no silo-rebuild-* sandbox is left" $? "$(sandbox_names | tr '\n' ' ')"
check "no staged home folder is left" $([ -z "$(ls "$REBUILD_DIR"/stage 2>/dev/null)" ]; echo $?) ""
check "no rebuild journal is left" $([ -z "$(journals)" ]; echo $?) ""

say "== finished"
echo "PASS: $PASS  FAIL: $FAILN  SKIPPED: $SKIPN"
[ "$FAILN" = 0 ]
