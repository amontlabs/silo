# Helpers for final_run.sh (sourced). Release-profile Dev build; computers: main (if absent) and e2e-final-* only.
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
# The CLI is the `silo` bundled in the release-profile Dev app (its channel comes from the bundle identifier):
# a plain release CLI is the production channel (it would use ~/.silo), so it is not used.
SILO="$REPO/app/SiloUI/target/release/bundle/macos/Silo Dev.app/Contents/MacOS/silo"
BUNDLE="$REPO/app/SiloUI/target/release/bundle/macos/Silo Dev.app"
EVID="$REPO/docs/v1/evidence/final"; RAW="$EVID/raw"
PROJ="$REPO/docs/v1/checks/final-proj"
CHECKS="$REPO/docs/v1/checks"
mkdir -p "$RAW"
PASS=0; FAILN=0; BLOCKED=0; MADE_MAIN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
blocked() { echo "[$(ts)] BLOCKED  $1  -- $2"; BLOCKED=$((BLOCKED+1)); }
now() { python3 -c 'import time;print(f"{time.monotonic():.3f}")'; }
elapsed() { python3 -c "print(f'{$(now) - $1:.2f}')"; }
# x C CMD...: run as root in C (shell text); xs: as the working account
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- sudo bash -c "$*" 2>&1 | grep -v '^step [0-9]*$'; return "${PIPESTATUS[0]}"; }
xs() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- bash -c "$*" 2>&1 | grep -v '^step [0-9]*$'; return "${PIPESTATUS[0]}"; }
jfield() { python3 -c "import json,sys;d=json.load(sys.stdin)
for k in sys.argv[1].split('.'): d=d[k]
print(d)" "$1" 2>/dev/null; }
state() { "$SILO" status "$1" --json 2>/dev/null | jfield state; }
exists() { "$SILO" status "$1" >/dev/null 2>&1; }
nvms() { "$SILO" list | awk '$2=="running" && $3=="macos"' | wc -l | tr -d ' '; }
# drop_one NAME: stop then drop an e2e-final-* computer. Never main: agents may not drop it, the owner does.
drop_one() { local c=$1; case $c in e2e-final-*) ;; *) echo "refusing to drop $c"; return 1;; esac
  exists "$c" || return 0; "$SILO" stop "$c" >/dev/null 2>&1
  for i in $(seq 20); do "$SILO" drop "$c" --force >/dev/null 2>&1 && return 0; exists "$c" || return 0; sleep 3; done; }
final_names() { "$SILO" list --json | python3 -c "
import json,sys
d=json.load(sys.stdin); d=d if isinstance(d,list) else d.get('computers',d)
print(' '.join(c['name'] for c in d if c['name'].startswith('e2e-final-')))"; }
engine_pid() { pgrep -f "release/bundle/macos/Silo Dev.app.*silod --agent" | head -1; }
# timed_fork LABEL OUTFILE ARGS...: runs `silo fork ARGS`, appends "label seconds" (CLI process wall clock, monotonic) to OUTFILE
timed_fork() { local label=$1 out=$2; shift 2; local t0; t0=$(now)
  (cd "$PROJ" && "$SILO" fork "$@" > "$RAW/fork-$label.out" 2> "$RAW/fork-$label.err"); local rc=$?
  local s; s=$(elapsed "$t0"); echo "$label $s" >> "$out"; echo "$label rc=$rc ${s}s: $(grep -i 'checkpoint' "$RAW/fork-$label.err" | head -1)"; return $rc; }
