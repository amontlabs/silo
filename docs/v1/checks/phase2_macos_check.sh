#!/usr/bin/env bash
# Silo v1 phase 2 VM check, macOS half (Dev engine). Run:
#   bash docs/v1/checks/phase2_macos_check.sh 2>&1 | tee docs/v1/evidence/phase-2/macos-check.txt
# Only `silo` (incl. silo exec) plus read-only host commands, and `claude -p` for the live task.
# Never more than 2 macOS VMs of ours at once (Apple licence). Computers: e2e-p2m-* only.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
MAIN=e2e-p2m-main; T1=e2e-p2m-t1; T2=e2e-p2m-t2
LABEL=org.silo.p2m.web
PROJ=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p2m.XXXXXX")
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { if [ "$2" = 0 ]; then echo "[$(ts)] PASS  $1  -- $3"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $1  -- $3"; FAILN=$((FAILN+1)); fi; }
nvms() { pgrep -f Virtualization.VirtualMachine | wc -l | tr -d ' '; }
vms() { echo "macOS VMs running (pgrep Virtualization.VirtualMachine): $(nvms)"; }
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
  for c in $T1 $T2 $MAIN; do stop_wait $c; done
  for c in $T1 $T2 $MAIN; do for i in $(seq 40); do "$SILO" drop $c --force >/dev/null 2>&1 && break; "$SILO" status $c >/dev/null 2>&1 || break; sleep 3; done; done
  rm -rf "$PROJ"; "$SILO" list; "$SILO" list | grep -c e2e-p2m | sed "s/^/e2e-p2m computers left: /"; "$SILO" checkpoints $MAIN 2>&1 | head -2; vms
}
trap cleanup EXIT

echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); git status --porcelain app: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app/Contents/MacOS/msb" --version 2>&1 | head -1)"
echo "host macOS $(sw_vers -productVersion) ($(uname -m)); silo $("$SILO" --version); claude $(claude --version); date $(date -u +%FT%TZ)"
vms; "$SILO" list

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

echo "== forks (from project dir)"
cd "$PROJ"
t1=$(date +%s); "$SILO" fork $MAIN --name $T1 2>&1 | grep -v "^Forking .*: [0-9]*s$"; rc=${PIPESTATUS[0]}
check "fork $T1" $rc "$(( $(date +%s) - t1 )) s"
[ $rc -eq 0 ] || { echo "ABORT: fork failed"; echo "== summary PASS=$PASS FAIL=$FAILN"; exit 1; }
"$SILO" fork $MAIN --name $T2 2>&1 | grep -v "^Forking .*: [0-9]*s$"; check "fork $T2 (idle, for --baseline)" ${PIPESTATUS[0]} ""
"$SILO" list | grep e2e-p2m; "$SILO" checkpoints $MAIN
start_wait $T1; check "start $T1" $? ""; vms

echo "== live task (claude -p on host, silo exec into $T1)"
MODEL=sonnet
TASK="You are working on a macOS computer named $T1, reachable ONLY with this command prefix: $SILO exec $T1 --timeout 170 -- bash -c '<script>'. Never use any other computer. In that computer (user silo): (1) ~/Library/LaunchAgents/$LABEL.plist runs a python http.server on port 8080 from ~/srv; edit the plist to serve on port 8081 instead and add an EnvironmentVariables dict with SILO_P2M_MODE=fork, then reload it with launchctl bootout/bootstrap on gui/\$(id -u) and check it answers on 8081. (2) Run brew install jq; Homebrew is at /opt/homebrew/bin/brew and is not on PATH, so first run: eval \"\$(/opt/homebrew/bin/brew shellenv)\". (3) In ~/srv (a git repo) add a file NOTES.md saying the service now listens on 8081 and commit it with git. Report briefly what you did."
cd "$PROJ"; t0=$(date +%s)
claude -p "$TASK" --model $MODEL --output-format json --max-turns 25 \
  --allowedTools "Bash($SILO exec $T1 *)" > "$PROJ/claude.json" 2> "$PROJ/claude.err"; crc=$?
echo "claude rc=$crc in $(( $(date +%s) - t0 )) s"
python3 - "$PROJ/claude.json" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
print("model(s):",", ".join(d.get("modelUsage",{}).keys()),"turns:",d.get("num_turns"),"cost_usd:",d.get("total_cost_usd"),"is_error:",d.get("is_error"))
print("agent result:",(d.get("result") or "")[:1200])
PY
head -c 400 "$PROJ/claude.err"
echo "-- observe task result read-only"
x $T1 'cat ~/Library/LaunchAgents/'$LABEL'.plist | grep -E "8081|8080|SILO_P2M"; '"$BREW"' brew list --versions jq; cd ~/srv && git log --oneline && git status --short'

echo "== diff"
vms
"$SILO" diff $T1 > "$PROJ/diff.txt" 2> "$PROJ/diff.err"; drc=$?; cat "$PROJ/diff.txt"; echo "(stderr:)"; cut -c1-300 "$PROJ/diff.err" | tail -5
nl=$(wc -l < "$PROJ/diff.txt" | tr -d ' ')
[ "$drc" = 0 ] && [ "$nl" -le 15 ]; check "diff text <=15 lines" $? "rc=$drc lines=$nl"
grep -qiE "LaunchAgents|$LABEL" "$PROJ/diff.txt"; check "diff shows the LaunchAgent change" $? ""
grep -qi 'jq' "$PROJ/diff.txt"; check "diff shows brew jq" $? "$(grep -i jq "$PROJ/diff.txt" | head -1)"
grep -qiE 'dependenc' "$PROJ/diff.txt"; check "diff collapses jq dependencies (+N dependencies)" $? "$(grep -ioE '\(\+[0-9]+ dependenc[a-z]*\)' "$PROJ/diff.txt" | head -1)"
grep -qiE 'srv|commit|NOTES' "$PROJ/diff.txt"; check "diff shows the commit / repo change" $? "$(grep -iE 'srv|commit|NOTES' "$PROJ/diff.txt" | head -2 | tr '\n' '|')"
"$SILO" diff $T1 --json > "$PROJ/diff.json" 2>/dev/null; echo "json rc=$? bytes=$(wc -c < "$PROJ/diff.json" | tr -d ' ')"
python3 - "$PROJ/diff.json" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
print("json top-level keys:",sorted(d.keys()))
for k in d:
    if k.startswith("baseline") or k=="reviewDigest": print(k,"=",str(d[k])[:200])
print("json size by key:",{k:len(json.dumps(v)) for k,v in d.items()})
s=json.dumps(d)
print("json mentions launchagent/label, jq, srv:", "p2m.web" in s, "jq" in s, "srv" in s)
PY
echo "-- without baseline subtraction (--no-baseline), for churn size"
"$SILO" diff $T1 --no-baseline --max-lines 200 2>&1 | tee "$PROJ/diff-nobase.txt" | head -60; echo "no-baseline lines: $(wc -l < "$PROJ/diff-nobase.txt" | tr -d ' ')"
echo "-- diff --baseline on the used fork (expected to refuse: it has run past its idle time); main stopped, $T1 running"
vms; t0=$(date +%s); "$SILO" diff $T1 --baseline > "$PROJ/diffb1.txt" 2> "$PROJ/diffb1.err"; echo "rc=$? in $(( $(date +%s) - t0 )) s"; cat "$PROJ/diffb1.txt"; echo "(stderr:)"; cut -c1-300 "$PROJ/diffb1.err" | tail -6; vms
echo "-- diff again after that (does it use a cached baseline?)"
"$SILO" diff $T1 2>/dev/null | tee "$PROJ/diff2.txt"; echo "lines: $(wc -l < "$PROJ/diff2.txt" | tr -d ' ')"
"$SILO" diff $T1 --json > "$PROJ/diff2.json" 2>/dev/null
python3 -c "import json;d=json.load(open('$PROJ/diff2.json'));print({k:d[k] for k in d if k.startswith('baseline')})"
REVIEW=$(python3 -c "import json;print(json.load(open('$PROJ/diff2.json')).get('reviewDigest',''))")

echo "== keep: main before"
start_wait $MAIN; check "start main" $? ""; vms
wait_http $MAIN 8080 >/dev/null; check "main serves 8080 before keep" $? ""
PID0=$(agent_pid); echo "main agent pid before: $PID0"
echo "main before: $(x $MAIN "$BREW brew list --versions jq 2>&1 | head -1; grep -c 8081 ~/Library/LaunchAgents/$LABEL.plist; cd ~/srv; git log --oneline | head -3 | tr '\n' '|'")"
x $MAIN '! test -e /opt/homebrew/bin/jq && ! test -d /opt/homebrew/Cellar/jq'; check "Homebrew jq absent on main before keep (the system has /usr/bin/jq)" $? ""
cd "$PROJ"
echo "-- fold --dry-run"; "$SILO" fold $T1 --into $MAIN --dry-run 2>&1 | head -30
REVIEW=$("$SILO" diff $T1 --json 2>/dev/null | python3 -c "import json,sys;print(json.load(sys.stdin).get('reviewDigest',''))")
t0=$(date +%s); "$SILO" fold $T1 --into $MAIN ${REVIEW:+--reviewed $REVIEW} > "$PROJ/fold.txt" 2>&1; frc=$?; head -40 "$PROJ/fold.txt"
if [ $frc -eq 2 ] && grep -q "changed since it was reviewed" "$PROJ/fold.txt"; then
  check "fold --reviewed with a digest taken seconds earlier (live macOS fork)" 1 "refused: digest drifted"
  "$SILO" diff $T1 --json 2>/dev/null | python3 -c "import json,sys;print('digest now',json.load(sys.stdin).get('reviewDigest'))"; echo "was $REVIEW"
  t0=$(date +%s); "$SILO" fold $T1 --into $MAIN > "$PROJ/fold.txt" 2>&1; frc=$?; head -40 "$PROJ/fold.txt"
fi
echo "fold rc=$frc in $(( $(date +%s) - t0 )) s"
check "fold succeeded" $frc "rc=$frc"
x $MAIN '/opt/homebrew/bin/jq --version'; check "Homebrew jq installed on main" $? "$(x $MAIN '/opt/homebrew/bin/jq --version 2>&1')"
x $MAIN "grep -q 8081 ~/Library/LaunchAgents/$LABEL.plist && grep -q SILO_P2M_MODE ~/Library/LaunchAgents/$LABEL.plist"; check "LaunchAgent change applied on main (8081 + env var)" $? ""
x $MAIN 'cd ~/srv && git log --oneline && test -f NOTES.md'; check "repo commit on main" $? ""
wait_http $MAIN 8081 >/dev/null; check "main serves 8081" $? "$(x $MAIN 'curl -s -m 5 http://127.0.0.1:8081/ | head -1')"
PID1=$(agent_pid); echo "main agent pid after: $PID1 (before $PID0)"
[ -n "$PID1" ] && [ "$PID1" != "$PID0" ]; check "agent restarted with a new PID" $? "$PID0 -> $PID1"
x $MAIN "launchctl print gui/\$(id -u)/$LABEL | grep -E 'SILO_P2M|pid ='"
"$SILO" checkpoints $MAIN

echo "== undo"
t0=$(date +%s); "$SILO" undo $MAIN 2>&1 | head -30; urc=${PIPESTATUS[0]}; echo "undo rc=$urc in $(( $(date +%s) - t0 )) s"
check "undo succeeded" $urc "rc=$urc"
[ "$(state $MAIN)" = running ] || { echo "main state after undo: $(state $MAIN); starting"; start_wait $MAIN; }
wait_http $MAIN 8080 60 >/dev/null; check "agent back on 8080 after undo" $? "$(x $MAIN 'curl -s -m 5 http://127.0.0.1:8080/ | head -1')"
x $MAIN '! curl -s -m 5 http://127.0.0.1:8081/ >/dev/null'; check "nothing on 8081 after undo" $? ""
x $MAIN "! grep -q 8081 ~/Library/LaunchAgents/$LABEL.plist && ! grep -q SILO_P2M_MODE ~/Library/LaunchAgents/$LABEL.plist && ! test -f ~/srv/NOTES.md"; check "plist and NOTES.md change gone" $? ""
x $MAIN '! test -e /opt/homebrew/bin/jq && ! test -d /opt/homebrew/Cellar/jq'; check "Homebrew jq gone after undo" $? "$(x $MAIN 'ls /opt/homebrew/bin/jq /opt/homebrew/Cellar 2>&1 | head -3 | tr "\n" " "')"
x $MAIN 'cd ~/srv && git log --oneline'
stop_wait $MAIN; stop_wait $T1; vms

[ -z "${P2M_QUICK:-}" ] || { echo "== summary PASS=$PASS FAIL=$FAILN (quick: baseline section skipped)"; exit 0; }
echo "== diff --baseline on an idle fork ($T2), main and $T1 stopped"
start_wait $T2; check "start $T2 (idle)" $? ""; say "idle 90 s"; sleep 90; vms
t0=$(date +%s); "$SILO" diff $T2 --baseline > "$PROJ/diffb.txt" 2> "$PROJ/diffb.err"; brc=$?
cat "$PROJ/diffb.txt"; echo "(stderr:)"; cut -c1-300 "$PROJ/diffb.err" | tail -5; echo "baseline rc=$brc in $(( $(date +%s) - t0 )) s"; vms
"$SILO" diff $T2 --baseline --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print({k:d[k] for k in d if k.startswith('baseline')})"
check "diff --baseline ran on idle fork" $brc "rc=$brc lines=$(wc -l < "$PROJ/diffb.txt" | tr -d ' ')"
echo "== summary PASS=$PASS FAIL=$FAILN"
