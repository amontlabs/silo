#!/usr/bin/env bash
# Silo v1 phase 1 VM check, macOS half (Dev engine). Run:
#   bash docs/v1/checks/phase1_macos_check.sh 2>&1 | tee docs/v1/evidence/phase-1/macos-check.txt
# Only `silo` (incl. silo exec) plus read-only host commands. One macOS VM of ours at a time.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
SRC=e2e-p1-mac; FORK=e2e-p1-macfork
PROJ=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p1-mac.XXXXXX")
LOGS=~/Library/Logs/org.silo.dev/macos-computers
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { if [ "$2" = 0 ]; then echo "[$(ts)] PASS  $1  -- $3"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $1  -- $3"; FAILN=$((FAILN+1)); fi; }
vms() { echo "macOS VMs running (pgrep Virtualization.VirtualMachine): $(pgrep -f Virtualization.VirtualMachine | wc -l | tr -d ' ')"; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- bash -c "$*"; }
cleanup() {
  say "cleanup"
  for c in $FORK $SRC; do "$SILO" stop $c >/dev/null 2>&1; "$SILO" drop $c --force >/dev/null 2>&1; done
  rm -rf "$PROJ"; "$SILO" list; "$SILO" checkpoints $SRC 2>&1 | head -2; vms
}
trap cleanup EXIT
cid() { "$SILO" status "$1" --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])"; }
ident() { x $1 'echo "host=$(hostname) local=$(scutil --get LocalHostName 2>/dev/null) uuid=$(ioreg -d2 -c IOPlatformExpertDevice | awk -F\" "/IOPlatformUUID/{print \$4}") hostkey=$(ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub | cut -d" " -f2)"'; }

echo "== header"
echo "engine commit: $(git -C "$REPO" rev-parse HEAD)"
echo "msb: $(python3 -c "import json;d=json.load(open('$REPO/app/SiloUI/runtime-inputs.json'));print(d['microsandboxVersion'],'patches:',len(d['patches']))")"
echo "macOS: $(sw_vers -productVersion) ($(uname -m)); silo: $("$SILO" --version); date $(date -u +%FT%TZ)"
vms; "$SILO" list

echo "== create source"
t0=$(date +%s); "$SILO" create $SRC --macos 2>&1 | grep -v "^Creating .*: [0-9]*s$"; rc=${PIPESTATUS[0]}
echo "create rc=$rc in $(( $(date +%s) - t0 )) s"; "$SILO" status $SRC --json | grep -E '"(state|detail|diskGiB)"'
SID=$(cid $SRC); echo "-- create runtime.log"; cut -c1-1800 "$LOGS/$SID/runtime.log"
grep -q "New Password: Permission denied" "$LOGS/$SID/runtime.log"; [ $? -ne 0 ]; check "create: 'New Password: Permission denied' gone" $? ""
check "create ends stopped (not failed)" $([ "$("$SILO" status $SRC --json | python3 -c "import json,sys;print(json.load(sys.stdin)['state'])")" = stopped ]; echo $?) "diskGiB=$("$SILO" status $SRC --json | python3 -c "import json,sys;print(json.load(sys.stdin)['diskGiB'])")"
vms; "$SILO" start $SRC; check "start source" $? ""
echo "-- source LaunchAgent dir, user"; x $SRC 'id -un; ls ~/Library/LaunchAgents'

echo "== source setup"
x $SRC 'set -e
mkdir -p ~/Library/LaunchAgents ~/bin; : > ~/interval.log
cat > ~/bin/silo-check-poller.sh <<"P"
#!/bin/bash
while true; do echo "$(hostname) $(date +%s) poll" >> ~/poll.log; sleep 5; done
P
chmod +x ~/bin/silo-check-poller.sh
echo "FAKE_BOT_TOKEN=p1-fake-not-real" > ~/.bot.env
cat > ~/Library/LaunchAgents/org.silo.check.interval.plist <<EOP
<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>org.silo.check.interval</string><key>ProgramArguments</key><array><string>/bin/bash</string><string>-c</string><string>echo "$(hostname) $(date +%s) interval" >> $HOME/interval.log</string></array><key>StartInterval</key><integer>10</integer></dict></plist>
EOP
cat > ~/Library/LaunchAgents/org.silo.check.poller.plist <<EOP
<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>org.silo.check.poller</string><key>ProgramArguments</key><array><string>/bin/bash</string><string>$HOME/bin/silo-check-poller.sh</string></array><key>KeepAlive</key><true/><key>RunAtLoad</key><true/></dict></plist>
EOP
echo "* * * * * echo \"\$(hostname) \$(date +\\%s) cron\" >> \$HOME/cron.log" | crontab -
crontab -l
U=$(id -u)
launchctl bootstrap gui/$U ~/Library/LaunchAgents/org.silo.check.interval.plist 2>&1 || launchctl load ~/Library/LaunchAgents/org.silo.check.interval.plist 2>&1
launchctl bootstrap gui/$U ~/Library/LaunchAgents/org.silo.check.poller.plist 2>&1 || launchctl load ~/Library/LaunchAgents/org.silo.check.poller.plist 2>&1
true'
say "waiting for interval, poller and cron to fire on the source (max 150 s)"
t=0; until [ "$(x $SRC 'echo $(wc -l < ~/interval.log 2>/dev/null | tr -d " "; true) $(wc -l < ~/poll.log 2>/dev/null | tr -d " "; true) $(wc -l < ~/cron.log 2>/dev/null | tr -d " "; true)' | awk '{print ($1>=1 && $2>=1 && $3>=1)}')" = 1 ] || [ $t -ge 150 ]; do sleep 5; t=$((t+5)); done
S0=$(x $SRC 'echo interval=$(wc -l < ~/interval.log 2>/dev/null | tr -d " "; true) poll=$(wc -l < ~/poll.log 2>/dev/null | tr -d " "; true) cron=$(wc -l < ~/cron.log 2>/dev/null | tr -d " "; true)'); echo "source after ${t}s: $S0"
echo "$S0" | awk '{split($1,a,"=");split($2,b,"=");split($3,c,"=");exit !(a[2]>=1&&b[2]>=1&&c[2]>=1)}'; check "source jobs fired (interval, poller, cron)" $? "$S0"
SRC_IDENT=$(ident $SRC); echo "source: $SRC_IDENT"
x $SRC 'sync; echo token:; ls ~/.bot.env'
"$SILO" stop $SRC; check "stop source (one macOS VM of ours at a time)" $? ""
until [ "$("$SILO" status $SRC --json | python3 -c "import json,sys;print(json.load(sys.stdin)['state'])")" = stopped ]; do sleep 3; done
vms

echo "== recipe + fork (no checkpoint named)"
mkdir -p "$PROJ/.silo"; printf 'quiet: [silo-check-poller.sh, org.silo.check.poller]\nscrub: [/Users/silo/.bot.env]\n' > "$PROJ/.silo/recipe.yaml"
cd "$PROJ"
t1=$(date +%s); "$SILO" fork $SRC --name $FORK 2>&1 | grep -v "^Forking .*: [0-9]*s$"; rc=${PIPESTATUS[0]}; t2=$(date +%s)
check "fork created (automatic fork point)" $rc "fork setup took $((t2-t1)) s"
[ $rc -eq 0 ] || { echo "ABORT: fork failed"; nb=$("$SILO" status $FORK --json | python3 -c "import json,sys;print(json.load(sys.stdin).get('boots'))"); [ "$nb" = 1 ]; check "one boot" $? "status boots=$nb"
echo "== summary PASS=$PASS FAIL=$FAILN"; exit 1; }
"$SILO" checkpoints $SRC; "$SILO" checkpoints $FORK
vms; "$SILO" start $FORK; rc=$?; t3=$(date +%s)
check "start fork" $rc "start to ready $((t3-t2)) s; fork+start $((t3-t1)) s (dev signal, not published)"
FID=$(cid $FORK); echo "-- fork runtime.log"; cut -c1-300 "$LOGS/$FID/runtime.log" 2>/dev/null
echo "boot-related log lines: starts=$(grep -c 'step: Starting macOS' "$LOGS/$FID/runtime.log" 2>/dev/null) shutdowns=$(grep -c 'step: Shutting down' "$LOGS/$FID/runtime.log" 2>/dev/null)"
echo "status boots: $("$SILO" status $FORK --json | python3 -c "import json,sys;print(json.load(sys.stdin).get('boots'))")"; grep -n 'boot [0-9]' "$LOGS/$FID/runtime.log"
echo "guest boots: $(x $FORK 'last reboot | grep -c reboot; sysctl -n kern.boottime')"
b0=$(x $FORK 'echo interval=$(wc -l < ~/interval.log 2>/dev/null | tr -d " "; true) poll=$(wc -l < ~/poll.log 2>/dev/null | tr -d " "; true) cron=$(wc -l < ~/cron.log 2>/dev/null | tr -d " "; true)'); say "fork baseline: $b0"
say "watching 130 s"; sleep 130
b1=$(x $FORK 'echo interval=$(wc -l < ~/interval.log 2>/dev/null | tr -d " "; true) poll=$(wc -l < ~/poll.log 2>/dev/null | tr -d " "; true) cron=$(wc -l < ~/cron.log 2>/dev/null | tr -d " "; true)'); say "fork after:    $b1"
[ "$b0" = "$b1" ]; check "no interval/poller/cron runs in fork over 130 s" $? "[$b0] -> [$b1]"
x $FORK 'echo procs=$(pgrep -f "[s]ilo-check-poller.sh" | wc -l | tr -d " "); crontab -l 2>&1 | head -2; launchctl list 2>/dev/null | grep -c org.silo.check; ls ~/Library/LaunchAgents'
FIDENT=$(ident $FORK); echo "fork:   $FIDENT"
[ "$FIDENT" != "$SRC_IDENT" ] && [ "$(echo $FIDENT | awk '{print $1}')" != "$(echo $SRC_IDENT | awk '{print $1}')" ]
check "new hostname" $? "$(echo $SRC_IDENT | awk '{print $1}') -> $(echo $FIDENT | awk '{print $1}')"
[ "$(echo $FIDENT | awk '{print $3}')" != "$(echo $SRC_IDENT | awk '{print $3}')" ]; check "new machine identifier (IOPlatformUUID)" $? ""
[ "$(echo $FIDENT | awk '{print $4}')" != "$(echo $SRC_IDENT | awk '{print $4}')" ]; check "new ssh host key" $? ""
x $FORK 'test ! -e ~/.bot.env'; check "token gone" $? ""
"$SILO" status $FORK --json > "$PROJ/s.json"; python3 - "$PROJ/s.json" <<'PY'
import json,sys
s=json.load(open(sys.argv[1])); q=s.get("quiet") or {}
print("quiet:",json.dumps(q),"secrets:",s.get("secrets"),"github:",s.get("github"),"state:",s.get("state"))
sys.exit(0 if q.get("applied") is True and not q.get("secrets") and not s.get("secrets") and not s.get("github") else 1)
PY
check "quiet record applied; no secret/github assignment" $? ""
x $FORK 'xcode-select -p; ls /Library/Developer/CommandLineTools >/dev/null && echo clt-present; ls -ld --time=birth /Library/Developer/CommandLineTools 2>/dev/null; stat -f "clt mtime %Sm" /Library/Developer/CommandLineTools'
grep -ci 'developer tools' "$LOGS/$FID/runtime.log" 2>/dev/null | sed 's/^/fork runtime.log lines mentioning developer tools: /'
nb=$("$SILO" status $FORK --json | python3 -c "import json,sys;print(json.load(sys.stdin).get('boots'))"); [ "$nb" = 1 ]; check "one boot" $? "status boots=$nb"
echo "== summary PASS=$PASS FAIL=$FAILN"
