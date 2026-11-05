#!/usr/bin/env bash
# Silo v1 phase 1 VM check, Linux half: quiet forks, against the Dev engine.
# Run: bash docs/v1/checks/phase1_linux_check.sh 2>&1 | tee docs/v1/evidence/phase-1/linux-quiet-check.txt
# Uses only the `silo` CLI (and `silo exec` inside the e2e-p1-* computers) plus read-only host commands.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
SRC=e2e-p1-src; F1=e2e-p1-fork; F2=e2e-p1-fork2; F3=e2e-p1-nosec; F4=e2e-p1-sec; F5=e2e-p1-x
PROJ=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p1-proj.XXXXXX")
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
res() { # res PASS|FAIL name detail
  echo "[$(ts)] $1  $2  -- $3"; if [ "$1" = PASS ]; then PASS=$((PASS+1)); else FAILN=$((FAILN+1)); fi; }
check() { # check name expected_cmd_status detail...
  local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then res PASS "$name" "$*"; else res FAIL "$name" "$*"; fi; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- sudo bash -c "$*"; }
cleanup() {
  say "cleanup"
  for c in $F5 $F4 $F3 $F2 $F1 $SRC; do "$SILO" stop "$c" >/dev/null 2>&1; "$SILO" drop "$c" --force >/dev/null 2>&1; done
  rm -rf "$PROJ"
  "$SILO" secrets unassign E2E_P1_TOKEN $SRC >/dev/null 2>&1
  say "silo list after cleanup:"; "$SILO" list; "$SILO" secrets list
}
trap cleanup EXIT

echo "== header"
echo "engine commit: $(git -C "$REPO" rev-parse HEAD)"
echo "msb: $(python3 -c "
import json;d=json.load(open('$REPO/app/SiloUI/runtime-inputs.json'));print(d['microsandboxVersion'],'patches:',len(d['patches']))")"
echo "macOS: $(sw_vers -productVersion) ($(uname -m))"
echo "silo --version: $("$SILO" --version)"
echo "date: $(date -u +%FT%TZ)"

echo "== silo list / status / drop main"
"$SILO" list; check "silo list" $? "exits 0"
"$SILO" status workspace-1 >/dev/null; check "silo status" $? "exits 0 for an existing computer"
out=$("$SILO" drop main 2>&1); rc=$?
echo "silo drop main -> rc=$rc: $out"
[ $rc -ne 0 ]; check "silo drop refuses main" $? "non-zero exit without --force"
"$SILO" list | grep -q '^main' && res FAIL "main untouched" "main exists after drop"

echo "== source setup"
"$SILO" create $SRC --linux --cpus 2 --memory 2 --disk 8 && "$SILO" start $SRC; check "create+start source" $? ""
x $SRC 'export DEBIAN_FRONTEND=noninteractive; command -v cron >/dev/null || (apt-get update -qq && apt-get install -y -qq cron) >/dev/null 2>&1; command -v cron'
x $SRC 'mkdir -p /root/.config/autostart
cat > /usr/local/bin/silo-check-poller.sh <<"P"
#!/bin/bash
while true; do
  code=$(curl -s -m 2 -o /dev/null -w "%{http_code}" "http://127.0.0.1:8099/poll?host=$(hostname)")
  echo "$(hostname) $(date +%s) poll $code" >> /var/log/silo-check-poll.log
  sleep 5
done
P
chmod +x /usr/local/bin/silo-check-poller.sh
cat > /root/.config/autostart/poller.desktop <<"D"
[Desktop Entry]
Type=Application
Name=poller
Exec=bash /usr/local/bin/silo-check-poller.sh
D
echo "FAKE_BOT_TOKEN=p1-fake-not-real" > /root/.bot.env
cat > /tmp/ct <<"C"
* * * * * echo "$(hostname) $(date +\%s) cron" >> /var/log/silo-check-cron.log
@reboot echo "$(hostname) $(date +\%s) reboot" >> /var/log/silo-check-reboot.log
C
crontab /tmp/ct; crontab -l
(service cron start || cron) 2>&1 | tail -1
setsid -f bash /usr/local/bin/silo-check-poller.sh </dev/null >/dev/null 2>&1
sleep 1; pgrep -af silo-check-poller'
say "waiting for cron to fire on the source (max 150 s)"
t=0; until [ "$(x $SRC 'wc -l < /var/log/silo-check-cron.log 2>/dev/null || echo 0')" -ge 1 ] || [ $t -ge 150 ]; do sleep 5; t=$((t+5)); done
n=$(x $SRC 'wc -l < /var/log/silo-check-cron.log 2>/dev/null || echo 0')
[ "$n" -ge 1 ]; check "source cron fired" $? "cron.log lines=$n after ${t}s"
SRC_ID=$(x $SRC 'cat /etc/machine-id'); SRC_HOST=$(x $SRC hostname)
x $SRC 'echo src poll lines=$(wc -l < /var/log/silo-check-poll.log); pgrep -fc "[s]ilo-check-poller.sh"; ls /root/.bot.env'

echo "== recipe + fork"
mkdir -p "$PROJ/.silo"
cat > "$PROJ/.silo/recipe.yaml" <<R
quiet: [silo-check-poller.sh]
scrub: [/root/.bot.env]
R
cd "$PROJ"
"$SILO" fork $SRC --name $F1; check "fork $F1 (recipe discovered)" $? ""
"$SILO" start $F1; check "start $F1" $? ""
snap() { x $1 'echo poll=$(wc -l < /var/log/silo-check-poll.log 2>/dev/null || echo 0) cron=$(wc -l < /var/log/silo-check-cron.log 2>/dev/null || echo 0) reboot=$(wc -l < /var/log/silo-check-reboot.log 2>/dev/null || echo 0) procs=$(pgrep -fc "[s]ilo-check-poller.sh")'; }
B=$(snap $F1); say "fork baseline after start: $B"
say "watching fork for 130 s"; sleep 130
A=$(snap $F1); say "fork after 130 s:        $A"
[ "$A" = "$B" ]; check "no poller/cron/@reboot lines written in fork" $? "before[$B] after[$A]"
procs=$(x $F1 'pgrep -fc "[s]ilo-check-poller.sh"; true' | head -1)
[ "${procs:-1}" = 0 ]; check "no poller process in fork" $? "matching processes=$procs"
x $F1 'cat /var/log/silo-check-*.log | tail -4; echo cron-procs=$(pgrep -c -x cron)'
x $F1 'crontab -l 2>&1 | head -3; ls /var/lib/silo-quiet/disabled 2>&1 | head; ls /root/.config/autostart 2>&1'
F1_ID=$(x $F1 'cat /etc/machine-id'); F1_HOST=$(x $F1 hostname)
echo "source id=$SRC_ID host=$SRC_HOST | fork id=$F1_ID host=$F1_HOST"
[ -n "$F1_ID" ] && [ "$F1_ID" != "$SRC_ID" ]; check "new machine-id" $? "$SRC_ID -> $F1_ID"
[ -n "$F1_HOST" ] && [ "$F1_HOST" != "$SRC_HOST" ]; check "new hostname" $? "$SRC_HOST -> $F1_HOST"
x $F1 'test ! -e /root/.bot.env'; check "/root/.bot.env gone" $? ""
x $SRC 'test -e /root/.bot.env'; check "source still has /root/.bot.env" $? ""
"$SILO" status $F1 --json > "$PROJ/f1-status.json"; "$SILO" checkpoints $F1 --json > "$PROJ/f1-checkpoints.json"
echo "-- fork status --json"; cat "$PROJ/f1-status.json"
python3 - "$PROJ/f1-status.json" <<'PY2'
import json,sys
s=json.load(open(sys.argv[1])); q=s.get("quiet") or {}
print("quiet record:",json.dumps(q)); print("secrets:",s.get("secrets"),"github:",s.get("github"))
ok = q.get("applied") is True and "silo-check-poller.sh" in json.dumps(q.get("stop")) and "/root/.bot.env" in json.dumps(q.get("scrub")) and not q.get("secrets") and not s.get("secrets") and not s.get("github")
sys.exit(0 if ok else 1)
PY2
check "quiet record: applied, stop+scrub as in recipe; no secrets/github assignment" $? "silo status --json"

echo "== fork of a fork"
"$SILO" fork $F1 --name $F2 && "$SILO" start $F2; check "fork+start $F2" $? ""
F2_ID=$(x $F2 'cat /etc/machine-id'); F2_HOST=$(x $F2 hostname)
echo "fork2 id=$F2_ID host=$F2_HOST"
[ -n "$F2_ID" ] && [ "$F2_ID" != "$F1_ID" ] && [ "$F2_ID" != "$SRC_ID" ]; check "fork2 machine-id differs from fork and source" $? "$F2_ID"
[ "$F2_HOST" != "$F1_HOST" ] && [ "$F2_HOST" != "$SRC_HOST" ]; check "fork2 hostname differs" $? "$F2_HOST"
x $F2 'echo fork2 procs=$(pgrep -fc "[s]ilo-check-poller.sh") poll=$(wc -l < /var/log/silo-check-poll.log)'
"$SILO" stop $F2 >/dev/null 2>&1; "$SILO" drop $F2 --force >/dev/null 2>&1

echo "== secrets opt-in"
printf 'p1-fake-secret-not-real' | "$SILO" secrets set E2E_P1_TOKEN --computer $SRC --domain example.invalid; check "secrets set + assign to source" $? ""
"$SILO" secrets list
"$SILO" fork $SRC --name $F3 && "$SILO" start $F3; check "fork without --secret" $? ""
"$SILO" status $F3 --json > "$PROJ/f3.json"
python3 - "$PROJ/f3.json" <<'PY2'
import json,sys
s=json.load(open(sys.argv[1])); print("fork (no --secret) secrets:",s.get("secrets"),"github:",s.get("github"))
sys.exit(0 if not s.get("secrets") and not s.get("github") else 1)
PY2
check "fork without --secret has no secret" $? ""
"$SILO" stop $F3 >/dev/null 2>&1; "$SILO" drop $F3 --force >/dev/null 2>&1
"$SILO" fork $SRC --name $F4 --secret E2E_P1_TOKEN && "$SILO" start $F4; check "fork with --secret E2E_P1_TOKEN" $? ""
"$SILO" status $F4 --json > "$PROJ/f4.json"
python3 - "$PROJ/f4.json" <<'PY2'
import json,sys
s=json.load(open(sys.argv[1])); print("fork (--secret) secrets:",json.dumps(s.get("secrets")),"github:",s.get("github"))
names=[x if isinstance(x,str) else x.get("name") for x in (s.get("secrets") or [])]
sys.exit(0 if names==["E2E_P1_TOKEN"] and not s.get("github") else 1)
PY2
check "fork with --secret has exactly that secret" $? ""
"$SILO" secrets list
"$SILO" stop $F4 >/dev/null 2>&1; "$SILO" drop $F4 --force >/dev/null 2>&1
out=$("$SILO" fork $SRC --name $F5 --secret E2E_P1_NO_SUCH_SECRET 2>&1); rc=$?
echo "rc=$rc: $out"
[ $rc -ne 0 ]; check "--secret for a name the source lacks fails" $? "rc=$rc"
"$SILO" list | grep -q "$F5"; [ $? -ne 0 ]; check "failed fork leaves no computer behind" $? ""
"$SILO" secrets unassign E2E_P1_TOKEN $SRC; "$SILO" secrets list

echo "== read-only GitHub"
out=$("$SILO" fork $SRC --name $F5 --secret github 2>&1); rc=$?
echo "silo fork --secret github on a source with no GitHub access -> rc=$rc: $out"
"$SILO" list | grep -q "$F5" && { "$SILO" stop $F5 >/dev/null 2>&1; "$SILO" drop $F5 --force >/dev/null 2>&1; }
say "SKIP: the engine honours SILO_GITHUB_API_BASE (github_http.rs), but a source gets a GitHub assignment only through github.connect (OAuth sign-in + GitHub App config) or github.access.set; the silo CLI has no GitHub command, and a raw engine call is out of scope for this check. So no scoped-token request is made and the read-only permissions cannot be observed. Real GitHub never contacted."

echo "== summary: PASS=$PASS FAIL=$FAILN"
