#!/usr/bin/env bash
# Silo v1 phase 1 VM check, Linux half: quiet forks against the Dev engine. Run:
#   bash docs/v1/checks/phase1_linux_check.sh 2>&1 | tee docs/v1/evidence/phase-1/linux-quiet-check.txt
# Only `silo` (incl. `silo exec` in e2e-p1-* computers) plus read-only host commands.
# The source's jobs are boot-persistent: XDG autostart entries (run by the Silo desktop boot hook) start the
# poller and cron; the root crontab has a per-minute job and an @reboot job.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
SRC=e2e-p1-src; CTL=e2e-p1-ctl; F1=e2e-p1-fork; F2=e2e-p1-fork2; F5=e2e-p1-x
PROJ=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p1-proj.XXXXXX")
PASS=0; FAILN=0; BLOCKED=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
blocked() { echo "[$(ts)] BLOCKED  $1  -- $2"; BLOCKED=$((BLOCKED+1)); }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- sudo bash -c "$*"; }
cleanup() {
  say "cleanup"
  for c in $F5 $F2 $F1 $CTL $SRC; do "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in $F5 $F2 $F1 $CTL $SRC; do for i in $(seq 30); do "$SILO" drop "$c" --force >/dev/null 2>&1 && break; "$SILO" status "$c" >/dev/null 2>&1 || break; sleep 3; done; done
  rm -rf "$PROJ"; say "silo list after cleanup:"; "$SILO" list
}
trap cleanup EXIT
LOGS='/var/log/silo-check-cron.log /var/log/silo-check-reboot.log /var/tmp/silo-check-poll.log'
counts() { x $1 'c() { cat $1 2>/dev/null | wc -l; }; echo cron=$(c /var/log/silo-check-cron.log) reboot=$(c /var/log/silo-check-reboot.log) poll=$(c /var/tmp/silo-check-poll.log) poller_procs=$(pgrep -f "[s]ilo-check-poller.sh" | wc -l) crond_procs=$(pgrep -x cron | wc -l)'; }
showlogs() { x $1 'for f in '"$LOGS"'; do [ -f $f ] && tail -n 3 $f | sed "s|^|  $(basename $f): |"; done; true'; }
# lines written by a host (first field of each log line)
hostlines() { x $1 'cat '"$LOGS"' 2>/dev/null | awk -v h='"$2"' "\$1==h" | wc -l'; }

echo "== header"
echo "engine/CLI commit: $(git -C "$REPO" rev-parse HEAD); tracked changes under app/ (git status --porcelain app): [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1); patches in runtime-inputs.json: $(python3 -c "import json;print(len(json.load(open('$REPO/app/SiloUI/runtime-inputs.json'))['patches']))")"
echo "host macOS: $(sw_vers -productVersion) ($(uname -m)); silo: $("$SILO" --version); date $(date -u +%FT%TZ)"

echo "== silo list / status / drop main"
"$SILO" list; check "silo list" $? ""
"$SILO" status workspace-1 >/dev/null; check "silo status" $? ""
out=$("$SILO" drop main 2>&1); rc=$?; echo "silo drop main -> rc=$rc: $out"; [ $rc -ne 0 ]; check "silo drop refuses main" $? ""

echo "== source: boot-persistent jobs"
"$SILO" create $SRC --linux --cpus 2 --memory 2 --disk 8 >/dev/null 2>&1 && "$SILO" start $SRC >/dev/null 2>&1; check "create+start source" $? ""
echo "guest OS: $(x $SRC '. /etc/os-release; echo $PRETTY_NAME; uname -r')" | tr '\n' ' '; echo
x $SRC 'echo desktop session processes: $(pgrep -f "xfce4-session|xfce4-panel|xfsettingsd" | wc -l)'
check "silo-desktop autostart is on (Xfce session running)" $([ "$(x $SRC 'pgrep -f xfce4-session | wc -l')" -ge 1 ]; echo $?) "XDG autostart runs under it"
x $SRC 'export DEBIAN_FRONTEND=noninteractive; command -v cron >/dev/null || (apt-get update -qq && apt-get install -y -qq cron) >/dev/null 2>&1; command -v cron
cat > /usr/local/bin/silo-check-poller.sh <<"P"
#!/bin/bash
while true; do
  code=$(curl -s -m 2 -o /dev/null -w "%{http_code}" "http://127.0.0.1:8099/poll?host=$(hostname)")
  echo "$(hostname) $(date +%s) poll $code" >> /var/tmp/silo-check-poll.log
  sleep 5
done
P
chmod +x /usr/local/bin/silo-check-poller.sh
install -d -o silo -g silo /home/silo/.config/autostart
cat > /home/silo/.config/autostart/poller.desktop <<"D"
[Desktop Entry]
Type=Application
Name=poller
Exec=bash /usr/local/bin/silo-check-poller.sh
D
chown silo:silo /home/silo/.config/autostart/poller.desktop
cat > /etc/xdg/autostart/silo-check-cron.desktop <<"D"
[Desktop Entry]
Type=Application
Name=silo-check-cron
Exec=sudo -n /usr/sbin/service cron start
D
echo "FAKE_BOT_TOKEN=p1-fake-not-real" > /root/.bot.env
cat > /tmp/ct <<"C"
* * * * * echo "$(hostname) $(date +\%s) cron" >> /var/log/silo-check-cron.log
@reboot echo "$(hostname) $(date +\%s) reboot" >> /var/log/silo-check-reboot.log
C
crontab /tmp/ct; crontab -l
service cron stop >/dev/null 2>&1; true'
echo "no hand-started job: restarting the source so everything starts only through boot"
"$SILO" stop $SRC >/dev/null 2>&1; "$SILO" start $SRC >/dev/null 2>&1; check "restart source (silo stop/start)" $? ""
say "waiting for poller, cron and @reboot after the boot (max 180 s)"
t=0; until x $SRC 'test $(cat /var/log/silo-check-cron.log 2>/dev/null | wc -l) -ge 1 -a $(cat /var/log/silo-check-reboot.log 2>/dev/null | wc -l) -ge 1 -a $(cat /var/tmp/silo-check-poll.log 2>/dev/null | wc -l) -ge 1' >/dev/null 2>&1 || [ $t -ge 180 ]; do sleep 5; t=$((t+5)); done
SC=$(counts $SRC); echo "source after boot (${t}s): $SC"; showlogs $SRC
echo "$SC" | grep -qE 'cron=[1-9]'; check "source: cron job ran after boot" $? "$SC"
echo "$SC" | grep -qE 'reboot=[1-9]'; check "source: @reboot job ran after boot" $? ""
echo "$SC" | grep -qE 'poll=[1-9]'; check "source: poller ran after boot (autostart)" $? ""
SRC_ID=$(x $SRC 'cat /etc/machine-id'); SRC_HOST=$(x $SRC hostname)

mkdir -p "$PROJ/.silo" "$PROJ/none"
printf 'quiet: [silo-check-poller.sh]\nscrub: [/root/.bot.env]\n' > "$PROJ/.silo/recipe.yaml"
printf 'quiet: []\nscrub: []\n' > "$PROJ/empty-recipe.yaml"

watch_fork() { # name host
  local f=$1
  "$SILO" start $f >/dev/null 2>&1; check "start $f" $? ""
  say "$f boot settle 45 s, then baseline"; sleep 45
  local b0 b1 h; h=$(x $f hostname); b0=$(counts $f); say "$f ($h) baseline: $b0"
  say "watching $f 130 s"; sleep 130; b1=$(counts $f); say "$f after: $b1"; showlogs $f
  echo "$f: lines written by host '$h': $(hostlines $f $h); by source host '$SRC_HOST' (inherited): $(hostlines $f $SRC_HOST)"
  FHOST=$h; FB0="$b0"; FB1="$b1"
}

echo "== CONTROL fork (empty recipe: no quiet/scrub list)"
(cd "$PROJ/none" && "$SILO" fork $SRC --name $CTL --recipe-file "$PROJ/empty-recipe.yaml" >/dev/null 2>&1); check "control fork created" $? ""
watch_fork $CTL
echo "CONTROL report: baseline[$FB0] after[$FB1]"
echo "control status quiet: $("$SILO" status $CTL --json | python3 -c "import json,sys;print(json.dumps(json.load(sys.stdin).get('quiet')))")"
echo "control: crontab=$(x $CTL 'crontab -l 2>&1 | head -1') xdg=$(x $CTL 'ls /etc/xdg/autostart | grep silo-check; ls /home/silo/.config/autostart 2>&1' | tr '\n' ' ')"
echo "control: token present=$(x $CTL 'test -e /root/.bot.env && echo yes || echo no')"
"$SILO" stop $CTL >/dev/null 2>&1; "$SILO" drop $CTL --force >/dev/null 2>&1

echo "== QUIET fork (recipe: quiet [silo-check-poller.sh], scrub [/root/.bot.env])"
(cd "$PROJ" && "$SILO" fork $SRC --name $F1 >/dev/null 2>&1); check "fork $F1 (recipe discovered)" $? ""
watch_fork $F1
n=$(hostlines $F1 "$FHOST"); [ "$n" = 0 ]; check "no cron/@reboot/poller line written by the fork host" $? "fork-host lines=$n"
[ "$FB0" = "$FB1" ]; check "no log growth in 130 s" $? "[$FB0] -> [$FB1]"
echo "$FB1" | grep -q 'poller_procs=0'; check "no poller process" $? ""
echo "$FB1" | grep -q 'crond_procs=0'; check "no cron daemon process" $? "$(echo $FB1 | grep -o 'crond_procs=[0-9]*')"
echo "-- diagnostics in the quiet fork: who runs the poller, what autostart/cron state remains"
x $F1 'ps -eo pid,ppid,user,etime,args | grep "[s]ilo-check\|[x]fce4-session\| [c]ron" | cut -c1-150; echo xdg-system:; ls /etc/xdg/autostart | grep silo-check; echo xdg-silo:; ls -la /home/silo/.config/autostart 2>&1; echo quiet-disabled:; find /var/lib/silo-quiet -maxdepth 3 2>/dev/null | head -20; crontab -l 2>&1 | head -2'
F1_ID=$(x $F1 'cat /etc/machine-id')
[ "$F1_ID" != "$SRC_ID" ]; check "new machine-id" $? "$SRC_ID -> $F1_ID"
[ "$FHOST" != "$SRC_HOST" ]; check "new hostname" $? "$SRC_HOST -> $FHOST"
x $F1 'test ! -e /root/.bot.env'; check "/root/.bot.env gone in fork" $? ""
x $SRC 'test -e /root/.bot.env'; check "source still has /root/.bot.env" $? ""
"$SILO" status $F1 --json > "$PROJ/s.json"; cat "$PROJ/s.json"
python3 - "$PROJ/s.json" <<'PY'
import json,sys
s=json.load(open(sys.argv[1])); q=s.get("quiet") or {}
sys.exit(0 if q.get("applied") is True and "silo-check-poller.sh" in json.dumps(q.get("stop")) and "/root/.bot.env" in json.dumps(q.get("scrub")) and not q.get("secrets") and not s.get("secrets") and not s.get("github") else 1)
PY
check "quiet record applied, stop/scrub as recipe, no assignments (the source has none either: weak)" $? ""

echo "== fork of a fork"
"$SILO" fork $F1 --name $F2 >/dev/null 2>&1 && "$SILO" start $F2 >/dev/null 2>&1; check "fork+start $F2" $? ""
F2_ID=$(x $F2 'cat /etc/machine-id'); F2_HOST=$(x $F2 hostname)
[ "$F2_ID" != "$F1_ID" ] && [ "$F2_ID" != "$SRC_ID" ]; check "fork2 machine-id differs from fork and source" $? "$F2_ID"
[ "$F2_HOST" != "$FHOST" ] && [ "$F2_HOST" != "$SRC_HOST" ]; check "fork2 hostname differs" $? "$F2_HOST"
sleep 30; n=$(hostlines $F2 "$F2_HOST"); echo "fork2 (forked from the quiet fork WITHOUT a recipe, so the poller is not named): host wrote $n log lines (informational)"
"$SILO" stop $F2 >/dev/null 2>&1; "$SILO" drop $F2 --force >/dev/null 2>&1

echo "== secrets"
out=$(printf 'p1-fake-secret-not-real' | "$SILO" secrets set E2E_P1_TOKEN --computer $SRC --domain example.invalid 2>&1); rc=$?; echo "secrets set rc=$rc: $out"
if echo "$out" | grep -q credential_access_denied; then
  blocked "positive --secret case (fork has exactly E2E_P1_TOKEN)" "owner's Keychain approval for the engine (credential_access_denied)"
  blocked "non-trivial 'no assignments by default'" "same: the source cannot be given a secret"
else check "secrets set" $rc ""; fi
out=$("$SILO" fork $SRC --name $F5 --secret E2E_P1_NO_SUCH_SECRET 2>&1); rc=$?; echo "rc=$rc: $out"
[ $rc -ne 0 ]; check "--secret for a name the source lacks fails" $? "rc=$rc"
"$SILO" list | grep -q "$F5"; [ $? -ne 0 ]; check "failed fork leaves no computer" $? ""
out=$("$SILO" fork $SRC --name $F5 --secret github 2>&1); echo "--secret github on a source with no GitHub access -> $out"
blocked "read-only GitHub against a mock" "SILO_GITHUB_API_BASE exists but the source needs a GitHub App installation (github.connect / github.access.set); the CLI has no GitHub command"

echo "== summary: PASS=$PASS FAIL=$FAILN BLOCKED=$BLOCKED"
