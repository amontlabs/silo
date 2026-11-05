#!/usr/bin/env bash
# Silo v1 phase 2 re-run, B: macOS restart attestation. Run:
#   bash docs/v1/checks/phase2_macos_restart_rerun.sh 2>&1 | tee docs/v1/evidence/phase-2/macos-restart-rerun.txt
# Only `silo` plus read-only host commands. No `silo start` of main after undo.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
MAIN=e2e-p2r-main; T1=e2e-p2r-t1
LABEL=org.silo.p2r.web
PROJ=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p2r.XXXXXX")
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { if [ "$2" = 0 ]; then echo "[$(ts)] PASS  $1  -- $3"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $1  -- $3"; FAILN=$((FAILN+1)); fi; }
nvms() { pgrep -f Virtualization.VirtualMachine | wc -l | tr -d ' '; }
vms() { echo "macOS VMs running (pgrep Virtualization.VirtualMachine): $(nvms)"; }
# watchdog: wd SECS cmd... kills the command (a silo client) after SECS and returns 124
wd() { local n=$1; shift; "$@" & local p=$!; ( sleep $n; kill -INT $p 2>/dev/null; sleep 5; kill $p 2>/dev/null ) & local w=$!; wait $p; local r=$?; kill $w 2>/dev/null; wait $w 2>/dev/null; [ $r -ge 128 ] && echo "WATCHDOG: command ended by signal after <=${n}s (rc=$r)"; return $r; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- bash -c "$*"; }
state() { "$SILO" status "$1" --json 2>/dev/null | python3 -c "import json,sys;print(json.load(sys.stdin)['state'])" 2>/dev/null; }
stop_wait() { "$SILO" stop "$1" >/dev/null 2>&1; for i in $(seq 60); do [ "$(state "$1")" = stopped ] && return 0; sleep 3; done; return 1; }
start_wait() { vms; [ "$(nvms)" -lt 2 ] || { echo "REFUSE to start $1: 2 macOS VMs already running"; return 9; }; "$SILO" start "$1" 2>&1 | grep -v "^Starting .*: [0-9]*s$"; return "${PIPESTATUS[0]}"; }
# the user's PATH has no brew in a non-login exec
BREW='eval "$(/opt/homebrew/bin/brew shellenv)";'
agent_pid() { x $MAIN "launchctl list $LABEL 2>/dev/null | awk '/\"PID\"/{gsub(/[;]/,\"\",\$3);print \$3}'"; }
wait_http() { # computer port [tries]
  for i in $(seq ${3:-40}); do
    out=$(x "$1" "curl -s -m 5 http://127.0.0.1:$2/" 2>/dev/null) && [ -n "$out" ] && { echo "$out"; return 0; }
    sleep 3
  done; return 1
}

cleanup() {
  say "cleanup"
  for c in $T1 $MAIN; do stop_wait $c; done
  for c in $T1 $MAIN; do for i in $(seq 40); do "$SILO" drop $c --force >/dev/null 2>&1 && break; "$SILO" status $c >/dev/null 2>&1 || break; sleep 3; done; done
  rm -rf "$PROJ"; "$SILO" list; "$SILO" list | grep -c e2e-p2r | sed "s/^/e2e-p2r computers left: /"; vms
  echo "== summary: $PASS passed, $FAILN failed"
}
trap cleanup EXIT
echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); git status --porcelain app: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app/Contents/MacOS/msb" --version 2>&1 | head -1)"
echo "host macOS $(sw_vers -productVersion) ($(uname -m)); silo $("$SILO" --version); date $(date -u +%FT%TZ)"
vms
echo "== main: create (or reuse), service, brew, repo"
if [ -z "$(state $MAIN)" ]; then
  t0=$(date +%s); "$SILO" create $MAIN --macos 2>&1 | grep -v "^Creating .*: [0-9]*s$"; echo "create rc=${PIPESTATUS[0]} in $(( $(date +%s) - t0 )) s"
fi
[ "$(state $MAIN)" = running ] || start_wait $MAIN
echo "guest: $(x $MAIN 'sw_vers | tr "\n" " "; id -un')"
if ! x $MAIN 'test -x /opt/homebrew/bin/brew'; then
  say "installing Homebrew as the user"
  x $MAIN 'NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)" > ~/brew-install.log 2>&1 &'
  for i in $(seq 60); do x $MAIN 'test -x /opt/homebrew/bin/brew && ! pgrep -f "[i]nstall.sh"' && break; sleep 5; done
fi
x $MAIN "$BREW brew --version | head -1; brew list --versions jq || echo 'jq: not installed'"
x $MAIN 'set -e
git config --global user.name p2m; git config --global user.email p2m@example.invalid
mkdir -p ~/srv ~/Library/LaunchAgents; cd ~/srv
[ -d .git ] || { git init -q -b main; echo "p2m service v1" > index.html; git add -A; git commit -qm "service v1"; }
cat > ~/Library/LaunchAgents/'$LABEL'.plist <<EOP
<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>'$LABEL'</string><key>ProgramArguments</key><array><string>/usr/bin/python3</string><string>-m</string><string>http.server</string><string>8080</string><string>--directory</string><string>$HOME/srv</string></array><key>KeepAlive</key><true/><key>RunAtLoad</key><true/></dict></plist>
EOP
U=$(id -u); launchctl bootout gui/$U/'$LABEL' 2>/dev/null || true
launchctl bootstrap gui/$U ~/Library/LaunchAgents/'$LABEL'.plist; true'
wait_http $MAIN 8080 >/dev/null; check "main service up on 8080" $? "$(x $MAIN 'curl -s -m 5 http://127.0.0.1:8080/')"
echo "main agent pid: $(agent_pid)"; x $MAIN 'cd ~/srv && git log --oneline'
x $MAIN 'sync'
mkdir -p "$PROJ/.silo"; printf 'keep:\n  restart: [%s]\n' "$LABEL" > "$PROJ/.silo/recipe.yaml"; cat "$PROJ/.silo/recipe.yaml"
stop_wait $MAIN; vms


echo "== fork"
cd "$PROJ"
"$SILO" fork $MAIN --name $T1 2>&1 | grep -v "^Forking .*: [0-9]*s$"; rc=${PIPESTATUS[0]}; check "fork $T1" $rc ""
[ $rc -eq 0 ] || exit 1
start_wait $T1; check "start $T1" $? ""
x $T1 'cd ~/srv && echo "p2m service v2" > index.html && git commit -qam "service v2" && git log --oneline | head -2'; check "fork: only the served repo content changed (plist untouched)" $? ""
x $T1 "cmp ~/Library/LaunchAgents/$LABEL.plist /dev/null >/dev/null 2>&1; grep -c 8080 ~/Library/LaunchAgents/$LABEL.plist"
echo "== keep with restart: [$LABEL]"
start_wait $MAIN; check "start main" $? ""
wait_http $MAIN 8080 >/dev/null; check "main serves v1 before keep" $? "$(x $MAIN 'curl -s -m 5 http://127.0.0.1:8080/')"
PID0=$(agent_pid); echo "agent pid before (agent_pid): $PID0"; x $MAIN "launchctl print gui/\$(id -u)/$LABEL | grep -E '^\s*(pid|state) ='"
PLIST0=$(x $MAIN "shasum ~/Library/LaunchAgents/$LABEL.plist")
"$SILO" fold $T1 --into $MAIN > "$PROJ/fold.txt" 2>&1; frc=$?; head -30 "$PROJ/fold.txt"
check "fold succeeded" $frc "rc=$frc"
PID1=$(agent_pid); echo "agent pid after: $PID1"; x $MAIN "launchctl print gui/\$(id -u)/$LABEL | grep -E '^\s*(pid|state) ='"
[ -n "$PID0" ] && [ -n "$PID1" ] && [ "$PID1" != "$PID0" ]; check "agent PID changed (restart step ran; plist not reloaded)" $? "$PID0 -> $PID1"
[ "$PLIST0" = "$(x $MAIN "shasum ~/Library/LaunchAgents/$LABEL.plist")" ]; check "plist unchanged by the keep" $? ""
wait_http $MAIN 8080 >/dev/null; out=$(x $MAIN 'curl -s -m 5 http://127.0.0.1:8080/'); echo "$out" | grep -q v2; check "main serves v2 after keep" $? "$out"
echo "== undo"
"$SILO" undo $MAIN > "$PROJ/undo.txt" 2>&1; urc=$?; head -20 "$PROJ/undo.txt"; check "undo succeeded" $urc "rc=$urc"
st=$(state $MAIN); [ "$st" = running ]; check "main running after undo (no script start)" $? "$st"
wait_http $MAIN 8080 60 >/dev/null; out=$(x $MAIN 'curl -s -m 5 http://127.0.0.1:8080/'); echo "$out" | grep -q v1; check "main serves old content (v1) after undo" $? "$out"
x $MAIN 'cd ~/srv && git log --oneline'
echo "agent pid after undo: $(agent_pid)"; [ -n "$(agent_pid)" ]; check "agent running after undo" $? "pid $(agent_pid)"
