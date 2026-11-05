#!/usr/bin/env bash
# Silo v1 phase 3 VM check, B: Make this main (silo fold --promote) and silo undo, Linux then macOS. Run:
#   bash docs/v1/checks/phase3_promote_check.sh 2>&1 | tee docs/v1/evidence/phase-3/promote-check.txt
# Only `silo` (incl. `silo exec` for fixtures and read-only observation) and read-only host commands (curl, ls).
# Needs a computer called `main`: uses the one part A left (created by the check), else `silo up` makes it; it is dropped at the end.
# Env: SKIP_MAC=1 skips the macOS half. Computers: main (ours, created by this check family), e2e-p3-* only.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
PROJ="$REPO/docs/v1/checks/phase3-proj"
FORK=e2e-p3-pr; SECRET=E2E_P3_SECRET; MSRC=e2e-p3-macsrc; MFORK=e2e-p3-macpr
PASS=0; FAILN=0; BLOCKED=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
blocked() { echo "[$(ts)] BLOCKED  $1  -- $2"; BLOCKED=$((BLOCKED+1)); }
now() { date +%s; }
# urlwait URL PATTERN: retry up to 45 s, print last answer
urlwait() { local i o; for i in $(seq 15); do o=$(curl -s -m 8 "$1"); echo "$o" | grep -q "$2" && { echo "$o"; return 0; }; sleep 3; done; echo "$o"; return 1; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- sudo bash -c "$*"; }
xm() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- bash -c "$*"; }
nvms() { pgrep -f Virtualization.VirtualMachine | wc -l | tr -d ' '; }
# every computer as name:id:state, sorted
snap() { "$SILO" list --json | python3 -c "
import json,sys
d=json.load(sys.stdin); d=d if isinstance(d,list) else d.get('computers',d)
print('\n'.join(sorted('%s:%s:%s'%(c['name'],c['id'][:8],c['state']) for c in d)))"; }
idof() { "$SILO" status "$1" --json 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])" 2>/dev/null; }
archive() { "$SILO" list --json | python3 -c "
import json,sys
d=json.load(sys.stdin); d=d if isinstance(d,list) else d.get('computers',d)
print(' '.join(c['name'] for c in d if c['name'].startswith('main-before-')))"; }
created_main=0
cleanup() {
  say "cleanup"
  for c in $FORK $MFORK $MSRC $(archive) main; do "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in $FORK $MFORK $MSRC $(archive) main; do for i in $(seq 20); do "$SILO" drop "$c" --force >/dev/null 2>&1 && break; "$SILO" status "$c" >/dev/null 2>&1 || break; sleep 3; done; done
  "$SILO" secrets list 2>&1 | grep -q "$SECRET" && echo "note: secret $SECRET still listed (no silo command removes a secret)"
  (cd "$PROJ" && "$SILO" recipe rm linux-dev) 2>&1
  say "silo list after cleanup:"; "$SILO" list; echo "macOS VMs running: $(nvms)"
  echo "== summary: $PASS passed, $FAILN failed ($BLOCKED blocked, not counted)"
}
trap cleanup EXIT
echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); tracked changes under app/: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1); silo: $("$SILO" --version); host macOS $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"
echo "macOS VMs running at start: $(nvms)"; "$SILO" list

echo "== Linux: main with a web app, a crontab line, a secret"
cd "$PROJ"
if ! "$SILO" status main >/dev/null 2>&1; then say "main absent: silo up"; "$SILO" up >/dev/null 2>&1; check "silo up (main created for part B)" $? ""; fi
echo "guest OS: $(x main '. /etc/os-release; echo $PRETTY_NAME kernel $(uname -r)')"
MAINID=$(idof main); echo "main id $MAINID"
x main 'echo main-v1 > /root/p3-marker; echo "* * * * * root echo p3cron >> /var/log/p3cron.log" > /etc/cron.d/p3-job; (crontab -l 2>/dev/null; echo "*/5 * * * * echo p3root >> /var/log/p3root.log") | crontab -'
echo "main crontab: $(x main 'crontab -l | tail -1; ls /etc/cron.d')"
if printf %s "throwaway-p3-value" | "$SILO" secrets set $SECRET --computer main --domain example.invalid > /tmp/p3-secret.out 2>&1; then
  check "silo secrets set $SECRET --computer main" 0 ""; HAVE_SECRET=1
else
  HAVE_SECRET=0; cat /tmp/p3-secret.out; if grep -q credential_access_denied /tmp/p3-secret.out; then blocked "secret assignment on main" "credential_access_denied: $(head -c 200 /tmp/p3-secret.out)"; else check "silo secrets set $SECRET --computer main" 1 "$(head -c 300 /tmp/p3-secret.out)"; fi
fi
"$SILO" secrets list
SSHDIR="$HOME/.silo-dev/ssh"
echo "SSH entries on this Dev home before: $(ls "$SSHDIR" 2>&1 | tr '\n' ' ')"
BEFORE=$(snap); echo "$BEFORE"

echo "== fork main into $FORK"
t=$(now); "$SILO" fork main --name $FORK > /tmp/p3-pfork.out 2>&1; check "silo fork main --name $FORK" $? "wall $(( $(now)-t )) s (Linux)"
"$SILO" start $FORK >/dev/null 2>&1; check "start $FORK" $? ""
FORKID=$(idof $FORK); echo "fork id $FORKID"
echo "fork status: $("$SILO" status $FORK | grep quiet)"
echo "fork cron parked: etc/cron.d=[$(x $FORK 'ls /etc/cron.d | tr "\n" " "')] root crontab=[$(x $FORK 'crontab -l 2>&1 | tail -1')]"
x $FORK 'echo fork-v2 > /root/p3-marker; sed -i "s/content=v1/content=v2/" /opt/p3web/server.py; pkill -x python3; sleep 1; (setsid nohup python3 /opt/p3web/server.py >/dev/null 2>&1 &); sleep 2; curl -s -m 5 http://127.0.0.1:3000/'
echo "fork web: $(x $FORK 'curl -s -m 5 http://127.0.0.1:3000/')"
echo "main web (v1): $(x main 'curl -s -m 5 http://127.0.0.1:3000/')"
"$SILO" ports main; URLMAIN=$("$SILO" ports main --json | python3 -c "import json,sys,re; print(re.findall(r'http://[^\\s\"]+', sys.stdin.read())[0])"); echo "main URL: $URLMAIN"; PORTSBEFORE=$("$SILO" ports main | sed "s/[0-9]*\\.[0-9]*s//" | tr -s " ")
echo "main URL before promote: $(curl -s -m 8 -w ' http=%{http_code}' $URLMAIN | head -c 200)"

echo "== silo fold $FORK --promote"
t=$(now); "$SILO" fold $FORK --promote --json > /tmp/p3-promote.json 2> /tmp/p3-promote.err; rc=$?; check "silo fold $FORK --promote exits 0" $rc "wall $(( $(now)-t )) s"
tail -6 /tmp/p3-promote.err; head -c 1500 /tmp/p3-promote.json; echo
AFTER=$(snap); echo "$AFTER"; AR=$(archive); echo "archive: $AR"
[ "$(idof main)" = "$FORKID" ]; check "the fork is now named main (id $FORKID)" $? "main id $(idof main)"
[ -n "$AR" ] && [ "$(idof $AR)" = "$MAINID" ]; check "old main archived as main-before-* with its id" $? "$AR"
echo "-- content on the new main"
"$SILO" start main >/dev/null 2>&1; check "start new main" $? ""
sleep 4
echo "marker: $(x main 'cat /root/p3-marker')  web: $(x main 'curl -s -m 5 http://127.0.0.1:3000/')"
[ "$(x main 'cat /root/p3-marker')" = fork-v2 ] && x main 'curl -s -m 5 http://127.0.0.1:3000/' | grep -q content=v2; check "new main serves the fork's content (file + web app output, in-guest)" $? ""
"$SILO" ports main; echo "main URL after promote (waits up to 45 s): $(urlwait $URLMAIN content=v2)"
urlwait $URLMAIN content=v2 >/dev/null; check "main's URL serves the fork's content" $? "http://3000.main.silo.localhost:47383/"
echo "-- secrets"
"$SILO" secrets list; "$SILO" status main | grep -i "secrets assigned"
if [ "$HAVE_SECRET" = 1 ]; then "$SILO" secrets list | grep "$SECRET" | grep -q main; check "secret $SECRET stays assigned to main (new main)" $? ""; else blocked "secrets carried" "no secret could be assigned"; fi
echo "-- quiet-parked jobs"
echo "new main: etc/cron.d=[$(x main 'ls /etc/cron.d | tr "\n" " "')] root crontab=[$(x main 'crontab -l 2>&1 | tail -1')]"
x main 'test -f /etc/cron.d/p3-job && crontab -l | grep -q p3root'; check "cron.d line and root crontab line restored on the new main" $? ""
echo "-- SSH alias"
echo "ssh main.silo-dev: $(ssh -o BatchMode=yes -o ConnectTimeout=15 main.silo-dev 'sudo cat /root/p3-marker; hostname' 2>&1 | tr '\n' ' ')"
[ "$(ssh -o BatchMode=yes -o ConnectTimeout=15 main.silo-dev 'sudo cat /root/p3-marker' 2>/dev/null)" = fork-v2 ]; check "ssh main.silo-dev reaches the new main (marker fork-v2)" $? ""
echo "== silo undo main"
t=$(now); "$SILO" undo main --json > /tmp/p3-undo.json 2> /tmp/p3-undo.err; rc=$?; check "silo undo main exits 0" $rc "wall $(( $(now)-t )) s"; tail -4 /tmp/p3-undo.err; head -c 800 /tmp/p3-undo.json; echo
AFTERU=$(snap); echo "$AFTERU"
[ "$(idof main)" = "$MAINID" ] && [ "$(idof $FORK)" = "$FORKID" ] && [ -z "$(archive)" ]; check "names back: main=$MAINID, $FORK=$FORKID, no main-before-*" $? "archive now: [$(archive)]"
"$SILO" start main >/dev/null 2>&1; sleep 4
echo "marker: $(x main 'cat /root/p3-marker')  web: $(x main 'curl -s -m 5 http://127.0.0.1:3000/')  cron: $(x main 'ls /etc/cron.d | tr "\n" " "; crontab -l | tail -1')"
[ "$(x main 'cat /root/p3-marker')" = main-v1 ] && x main 'curl -s -m 5 http://127.0.0.1:3000/' | grep -q content=v1; check "content back on main (file + web app)" $? ""
"$SILO" ports main; urlwait $URLMAIN content=v1 >/dev/null; check "main's URL serves the original content again" $? ""
x main 'test -f /etc/cron.d/p3-job && crontab -l | grep -q p3root'; check "main's cron lines intact after undo" $? ""
[ "$(ssh -o BatchMode=yes -o ConnectTimeout=15 main.silo-dev 'sudo cat /root/p3-marker' 2>/dev/null)" = main-v1 ]; check "ssh main.silo-dev reaches the original main again" $? ""
[ "$("$SILO" ports main | sed "s/[0-9]*\\.[0-9]*s//" | tr -s " ")" = "$PORTSBEFORE" ]; check "main's ports back as before" $? ""
[ "$HAVE_SECRET" = 1 ] && { "$SILO" secrets list | grep "$SECRET" | grep -q main; check "secret still on main after undo" $? ""; }
echo "fork after undo: $(x $FORK 'cat /root/p3-marker' 2>&1 | head -1)"

if [ "${SKIP_MAC:-0}" != 1 ]; then
echo "== macOS: source $MSRC, fork $MFORK"
echo "macOS VMs running: $(nvms)"
[ "$(nvms)" -le 0 ] || { echo "macOS VMs of someone else are running: skipping macOS"; SKIP_MAC=1; }
fi
if [ "${SKIP_MAC:-0}" != 1 ]; then
t=$(now); "$SILO" create $MSRC --macos 2>&1 | grep -v "^Creating .*: [0-9]*s$"; check "create $MSRC (macOS)" ${PIPESTATUS[0]} "$(( $(now)-t )) s"
"$SILO" start $MSRC >/dev/null 2>&1; check "start $MSRC" $? ""
echo "guest: $(xm $MSRC 'sw_vers | tr "\n" " "')"
xm $MSRC 'echo mac-main-v1 > ~/p3-marker'
MSID=$(idof $MSRC)
t=$(now); "$SILO" fork $MSRC --name $MFORK > /tmp/p3-mfork.out 2>&1; mrc=$?; check "silo fork $MSRC --name $MFORK" $mrc "wall $(( $(now)-t )) s (macOS fork of a running source, dev signal only)"
tail -3 /tmp/p3-mfork.out
MFID=$(idof $MFORK)
"$SILO" start $MFORK >/dev/null 2>&1; check "start $MFORK" $? "macOS VMs: $(nvms)"
xm $MFORK 'echo mac-fork-v2 > ~/p3-marker'; echo "fork marker: $(xm $MFORK 'cat ~/p3-marker')"
echo "macOS VMs running: $(nvms)"
MB=$(snap); echo "$MB"
t=$(now); "$SILO" fold $MFORK --promote --json > /tmp/p3-mpromote.json 2> /tmp/p3-mpromote.err; rc=$?; check "silo fold $MFORK --promote (macOS)" $rc "wall $(( $(now)-t )) s"
tail -5 /tmp/p3-mpromote.err; head -c 1200 /tmp/p3-mpromote.json; echo
snap; AR=$(archive); echo "archive: [$AR]"
"$SILO" list --json | python3 -c "
import json,sys
d=json.load(sys.stdin); d=d if isinstance(d,list) else d.get('computers',d)
print([ (c['name'],c['id'][:8]) for c in d if c['id'] in ('$MSID','$MFID')])"
echo "source name expected after promote: $MSRC now id $(idof $MSRC) (fork id ${MFID:0:8}, source id ${MSID:0:8})"
[ "$(idof $MSRC)" = "$MFID" ]; check "macOS: the fork now carries the source's name" $? ""
echo "macOS VMs running: $(nvms)"
"$SILO" start $MSRC >/dev/null 2>&1; echo "marker on promoted: $(xm $MSRC 'cat ~/p3-marker')"
[ "$(xm $MSRC 'cat ~/p3-marker')" = mac-fork-v2 ]; check "macOS: promoted computer has the fork's content" $? ""
"$SILO" stop $MSRC >/dev/null 2>&1
t=$(now); "$SILO" undo $MSRC --json > /tmp/p3-mundo.json 2> /tmp/p3-mundo.err; rc=$?; check "silo undo $MSRC (macOS)" $rc "wall $(( $(now)-t )) s"; tail -4 /tmp/p3-mundo.err; head -c 600 /tmp/p3-mundo.json; echo
snap
[ "$(idof $MSRC)" = "$MSID" ] && [ "$(idof $MFORK)" = "$MFID" ]; check "macOS: names back after undo" $? ""
"$SILO" start $MSRC >/dev/null 2>&1; echo "marker after undo: $(xm $MSRC 'cat ~/p3-marker')"
[ "$(xm $MSRC 'cat ~/p3-marker')" = mac-main-v1 ]; check "macOS: content back after undo" $? ""
fi
