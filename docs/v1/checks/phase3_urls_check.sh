#!/usr/bin/env bash
# Silo v1 phase 3 VM check, A: one URL per fork. Run:
#   bash docs/v1/checks/phase3_urls_check.sh 2>&1 | tee docs/v1/evidence/phase-3/urls-check.txt
# Only `silo` (incl. `silo exec` for observation) and read-only host commands (curl). Env: KEEP_MAIN=1 leaves `main` for part B.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
PROJ="$REPO/docs/v1/checks/phase3-proj"
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
now() { date +%s; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- sudo bash -c "$*"; }
FORKS=()
cleanup() {
  say "cleanup"
  for f in "${FORKS[@]:-}"; do [ -n "$f" ] && { "$SILO" stop "$f" >/dev/null 2>&1; "$SILO" drop "$f" --force >/dev/null 2>&1; }; done
  if [ "${KEEP_MAIN:-0}" != 1 ]; then
    "$SILO" stop main >/dev/null 2>&1; for i in $(seq 20); do "$SILO" drop main --force >/dev/null 2>&1 && break; sleep 3; done
    (cd "$PROJ" && "$SILO" recipe rm linux-dev) 2>&1
  fi
  "$SILO" list
  echo "== summary: $PASS passed, $FAILN failed"
}
trap cleanup EXIT
echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); tracked changes under app/: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1); silo: $("$SILO" --version); host $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"
if "$SILO" status main >/dev/null 2>&1; then echo "main already exists; stopping (not created by this check)"; trap - EXIT; exit 3; fi
cd "$PROJ"
echo "== silo up (project recipe .silo/recipes/linux-dev.yaml replaces the built-in: python3 web app on :3000 as service 'web', ports [3000])"
t=$(now); "$SILO" up --json > /tmp/p3-up.json 2> /tmp/p3-up.err; rc=$?; check "silo up" $rc "$(( $(now)-t )) s"
tail -5 /tmp/p3-up.err; head -c 1500 /tmp/p3-up.json; echo
echo "guest OS: $(x main '. /etc/os-release; echo $PRETTY_NAME kernel $(uname -r); hostname')"
echo "== web app through the boot entry on main"
sleep 3; echo "in-guest: $(x main 'curl -s -m 5 http://127.0.0.1:3000/')"
"$SILO" checkpoints main
echo "main ports:"; "$SILO" ports main
echo "== fork main -n 4"
t=$(now); "$SILO" fork main -n 4 --json > /tmp/p3-fork.json 2> /tmp/p3-fork.err; rc=$?; ft=$(( $(now)-t ))
check "silo fork main -n 4" $rc "wall $ft s (dev signal only)"
tail -4 /tmp/p3-fork.err
FORKS=($(python3 -c "
import json
d=json.load(open('/tmp/p3-fork.json'))
print(' '.join(f['name'] for f in d.get('forks',[])))"))
echo "forks: ${FORKS[*]:-}"
[ "${#FORKS[@]}" = 4 ]; check "4 forks created" $? "${#FORKS[@]} forks"
python3 -c "
import json
d=json.load(open('/tmp/p3-fork.json')); print('checkpoint:', d.get('checkpoint'), '(main warm checkpoint: see list above)')
for f in d.get('forks',[]): print(f['name'], f['state'], 'urls=', f.get('urls'))"
"$SILO" list
n=0; for f in "${FORKS[@]}"; do st=$("$SILO" status "$f" --json | python3 -c "import json,sys; print(json.load(sys.stdin)['state'])"); [ "$st" = running ] && n=$((n+1)); done
[ "$n" = 4 ]; check "all 4 forks running" $? "$n running"
echo "== each fork serves :3000 inside the guest (silo exec) with its own hostname"
declare_hosts=""
for f in "${FORKS[@]}"; do b=$("$SILO" exec "$f" -- curl -s -m 5 http://127.0.0.1:3000/); echo "$f in-guest: [$b]"; declare_hosts="$declare_hosts $(echo "$b" | sed 's/^host=\([^ ]*\).*/\1/')"; done
d=$(echo $declare_hosts | tr ' ' '\n' | sort -u | wc -l | tr -d ' ')
[ "$d" = 4 ]; check "web app runs in each fork with its own hostname (in-guest)" $? "$d distinct: $declare_hosts"
echo "== URLs through the proxy (network.urls)"
PORT=$(lsof -nP -a -iTCP -sTCP:LISTEN -p "$(pgrep -f 'Silo Dev.app.*silod' | head -1)" 2>/dev/null | awk '/47383|LISTEN/ {print $9}' | sed 's/.*://' | head -1)
echo "dev proxy port (lsof of the dev silod; default 47383): ${PORT:-unknown}"
nurl=0; for f in "${FORKS[@]}"; do u=$(python3 -c "
import json
d=json.load(open('/tmp/p3-fork.json'))
for x in d['forks']:
    if x['name']=='$f': print(' '.join(x.get('urls') or []))"); [ -n "$u" ] && nurl=$((nurl+1)); echo "$f printed urls: [$u]"; done
[ "$nurl" = 4 ]; check "each fork prints its own URL (network.urls)" $? "$nurl of 4 forks have URLs"
echo "-- concurrent curl of each fork's printed URL"
rm -rf /tmp/p3-out; mkdir -p /tmp/p3-out
i=0; for f in "${FORKS[@]}"; do u=$(python3 -c "
import json
d=json.load(open('/tmp/p3-fork.json'))
for x in d['forks']:
    if x['name']=='$f': print((x.get('urls') or [''])[0])"); echo "$u" > /tmp/p3-out/$i.url; ( curl -s -m 20 -w ' http=%{http_code}' "$u" > /tmp/p3-out/$i.out ) & i=$((i+1)); done; wait
bad=0; hosts=""; i=0; for f in "${FORKS[@]}"; do echo "$f $(cat /tmp/p3-out/$i.url): $(cat /tmp/p3-out/$i.out)"; grep -q "host=$f " /tmp/p3-out/$i.out && grep -q 'http=200' /tmp/p3-out/$i.out || bad=1; i=$((i+1)); done
check "4 concurrent curls: each fork's URL answers 200 with its own hostname" $bad ""
for f in "${FORKS[@]}"; do "$SILO" ports $f; done
echo "== drop forks"
for f in "${FORKS[@]}"; do "$SILO" stop "$f" >/dev/null 2>&1; "$SILO" drop "$f" --force >/dev/null 2>&1; echo "dropped $f rc=$?"; done
sleep 3
bad=0; i=0; for f in "${FORKS[@]}"; do c=$(curl -s -m 10 -o /dev/null -w "%{http_code}" "$(cat /tmp/p3-out/$i.url)"); i=$((i+1)); echo "$f after drop: http $c"; case $c in 404|502) ;; *) bad=1;; esac; done
check "dropped forks' URLs stop answering (404/502)" $bad ""
FORKS=()
