#!/usr/bin/env bash
# Fork measurements ("before"), phase F1 of docs/forks-ci/PLAN.md. Runs on a Mac host and on a Linux KVM host.
#
#   SILO=/path/to/silo [MACHINE=name] [SECTIONS="cold warm1 warm4 ten macos"] bash docs/forks-ci/checks/measure_forks.sh
#
# Environment:
#   SILO            the silo CLI of the engine to measure (default on a Mac: ~/.local/bin/silo-dev; required on Linux).
#                   Its engine must already be running. Never a production CLI on a developer Mac.
#   MACHINE         label of the evidence directory (default: hostname)
#   OUT             evidence directory (default: docs/forks-ci/evidence/f1-before/$MACHINE)
#   SECTIONS        measurements to run, in this order (default: cold warm1 warm4 ten, plus macos on a Mac when
#                   SILO_MACOS_SOURCE is set). cold = (a), warm1 = (c), warm4 = (b) and (d), ten = (f), macos = (e).
#   SILO_MACOS_SOURCE  name of a macOS computer to fork for (e); Mac only.
#   SILO_PROJECT    project folder holding .silo/recipes/demo.yaml (default: docs/v1/checks/final-proj)
#   WAIT_COLD       seconds to wait before each cold run (default 70 > warm_max_age 60)
#   RUNS            runs per measurement (default 3)
#   TEN             how many forks "at once" (default 10)
#
# Computers: `main` (made by `silo up --recipe demo` if absent; dropped at the end only if this run made it)
# and e2e-forks-*; nothing else is touched.
#
# Clock: monotonic wall time of the `silo fork` process, from spawn to exit. It exits after every fork is started
#   and `true` has run in it. The CLI polls jobs every 300 ms, so each value can read up to 0.3 s late; the engine
#   does not expose job timestamps to the CLI.
# RAM: 20 s after the fork is ready, macOS `footprint` / Linux sum of Pss over the computer's msb process tree
#   (measure_footprint.py).
set -uo pipefail
CHECKS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$CHECKS/../../.." && pwd)
OS=$(uname -s)
if [ -z "${SILO:-}" ]; then
  if [ "$OS" = Darwin ]; then SILO="$HOME/.local/bin/silo-dev"; else echo "SILO must name the silo CLI on Linux" >&2; exit 2; fi
fi
MACHINE=${MACHINE:-$(hostname -s)}
OUT=${OUT:-$REPO/docs/forks-ci/evidence/f1-before/$MACHINE}
RAW=$OUT/raw
PROJ=${SILO_PROJECT:-$REPO/docs/v1/checks/final-proj}
WAIT_COLD=${WAIT_COLD:-70}
RUNS=${RUNS:-3}
TEN=${TEN:-10}
MACOS_SOURCE=${SILO_MACOS_SOURCE:-}
DEFAULT_SECTIONS="cold warm1 warm4 ten"
[ "$OS" = Darwin ] && [ -n "$MACOS_SOURCE" ] && DEFAULT_SECTIONS="$DEFAULT_SECTIONS macos"
SECTIONS=${SECTIONS:-$DEFAULT_SECTIONS}
mkdir -p "$RAW"
MADE_MAIN=0

ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
now() { python3 -c 'import time;print(f"{time.monotonic():.3f}")'; }
elapsed() { python3 -c "print(f'{$(now) - $1:.2f}')"; }
stats() { python3 "$CHECKS/measure_stats.py" "$@"; }
fp() { python3 "$CHECKS/measure_footprint.py" "$1"; }
jfield() { python3 -c "import json,sys;d=json.load(sys.stdin)
for k in sys.argv[1].split('.'): d=d[k]
print(d)" "$1" 2>/dev/null; }
exists() { "$SILO" status "$1" >/dev/null 2>&1; }
state() { "$SILO" status "$1" --json 2>/dev/null | jfield state; }
nvms() { "$SILO" list | awk '$2=="running" && $3=="macos"' | wc -l | tr -d ' '; }
# drop_one NAME: stop and drop an e2e-forks-* computer (never anything else here).
drop_one() { local c=$1; case $c in e2e-forks-*) ;; *) echo "refusing to drop $c"; return 1;; esac
  exists "$c" || return 0; "$SILO" stop "$c" >/dev/null 2>&1
  for _ in $(seq 20); do "$SILO" drop "$c" --force >/dev/null 2>&1 && return 0; exists "$c" || return 0; sleep 3; done; }
forks_names() { "$SILO" list --json | python3 -c "
import json,sys
d=json.load(sys.stdin); d=d if isinstance(d,list) else d.get('computers',d)
print(' '.join(c['name'] for c in d if c['name'].startswith('e2e-forks-')))"; }
# timed_fork LABEL OUTFILE ARGS...: runs `silo fork ARGS` in the project, appends "label seconds" to OUTFILE.
timed_fork() { local label=$1 out=$2; shift 2; local t0 before after; before=$(fork_points); t0=$(now)
  (cd "$PROJ" && "$SILO" fork "$@" > "$RAW/fork-$label.out" 2> "$RAW/fork-$label.err"); local rc=$?
  local s; s=$(elapsed "$t0"); echo "$label $s" >> "$out"
  after=$(fork_points)
  if [ "$after" -gt "$before" ]; then echo "  $label: captured a Fork point of main (no usable warm checkpoint)" > "$RAW/fork-$label.note"
  else echo "  $label: no new checkpoint (warm checkpoint used)" > "$RAW/fork-$label.note"; fi
  echo "$label rc=$rc ${s}s"; return $rc; }
up_main() { (cd "$PROJ" && "$SILO" up --recipe demo > "$RAW/up-$1.out" 2>&1); }
# The fork captures a "Fork point" checkpoint of main when no warm checkpoint is usable, and prints nothing
# about it; so count main's "Fork point" checkpoints around each fork.
fork_points() { "$SILO" checkpoints main 2>/dev/null | grep -c 'Fork point'; }
# timed_fork notes the result in $RAW/fork-LABEL.note and echoes it.
note_checkpoint() { cat "$RAW/fork-$1.note"; }

cleanup() {
  say "cleanup"
  for c in $(forks_names); do drop_one "$c"; done
  if [ "$MADE_MAIN" = 1 ] && exists main; then "$SILO" stop main >/dev/null 2>&1
    for _ in $(seq 20); do "$SILO" drop main --force >/dev/null 2>&1 && break; exists main || break; sleep 3; done
    exists main && echo "main (made by this run) could not be dropped" || echo "main (made by this run) dropped"; fi
  "$SILO" list
}
trap cleanup EXIT

header() {
  echo "== header ($(date -u +%FT%TZ))"
  echo "commit of the scripts: $(git -C "$REPO" rev-parse HEAD 2>/dev/null)"
  python3 "$CHECKS/measure_facts.py"
  echo "silo CLI: $SILO ($("$SILO" --version 2>&1 | head -1); channel $("$SILO" --print-channel-identifier 2>/dev/null))"
  echo "silo CLI resolves to: $(python3 -c 'import os,sys;print(os.path.realpath(sys.argv[1]))' "$SILO")"
  echo "computers: $("$SILO" list | tr -s ' ' | tr '\n' ';')"
  echo "sections: $SECTIONS; runs $RUNS; ten = $TEN; wait before a cold run ${WAIT_COLD}s"
  echo "clock: wall clock (monotonic) of the silo process; the CLI polls jobs every 300 ms"
}

ensure_main() {
  if exists main; then say "main exists ($(state main)); not dropped by this run"
  else say "main absent: creating it with silo up --recipe demo"; MADE_MAIN=1; up_main first; fi
  up_main ensure; tail -n 3 "$RAW/up-ensure.out"
  echo "main: $("$SILO" list | grep '^main ')"
}

measure_cold() { # (a)
  : > "$RAW/cold.txt"; : > "$RAW/cold-fp.txt"
  for i in $(seq "$RUNS"); do
    say "cold run $i: waiting ${WAIT_COLD}s so no warm checkpoint is usable"; sleep "$WAIT_COLD"
    timed_fork cold$i "$RAW/cold.txt" main --name e2e-forks-cold$i -- true; note_checkpoint cold$i
    if [ "$i" = 1 ]; then sleep 20; echo "-- RAM of cold fork 1 (20 s after ready)"; fp e2e-forks-cold1 | tee -a "$RAW/cold-fp.txt"; fi
    drop_one e2e-forks-cold$i
  done
  stats "(a) Linux fork of a running idle main, cold, to running+ready" "$RAW/cold.txt"
}

measure_warm1() { # (c)
  : > "$RAW/warm1.txt"
  for i in $(seq "$RUNS"); do
    up_main warm1-$i; sleep 3
    timed_fork warm1-$i "$RAW/warm1.txt" main --name e2e-forks-w1x$i -- true; note_checkpoint warm1-$i
    drop_one e2e-forks-w1x$i
  done
  stats "(c) one fork right after silo up (warm checkpoint), to running+ready" "$RAW/warm1.txt"
}

measure_warm4() { # (b) and (d)
  : > "$RAW/warm4.txt"; : > "$RAW/ram-per-fork.txt"; : > "$RAW/warm4-fp.txt"
  for i in $(seq "$RUNS"); do
    up_main warm4-$i; sleep 3
    timed_fork warm4-$i "$RAW/warm4.txt" main -n 4 --name e2e-forks-w4x$i -- true; note_checkpoint warm4-$i
    # Footprint of the four forks of this run, 20 s after ready (every run, so (d) has N=3 runs of 4 forks).
    sleep 20
    for k in 1 2 3 4; do
      echo "-- RAM of e2e-forks-w4x$i-$k"; fp e2e-forks-w4x$i-$k | tee "$RAW/fp-w4x$i-$k.txt" | tee -a "$RAW/warm4-fp.txt"
      echo "w4x$i-$k $(grep '^TOTAL' "$RAW/fp-w4x$i-$k.txt" | awk '{print $2}')" >> "$RAW/ram-per-fork.txt"
    done
    for k in 1 2 3 4; do drop_one e2e-forks-w4x$i-$k; done
  done
  stats "(b) silo fork main -n 4 right after silo up (warm checkpoint), to 4 running+ready" "$RAW/warm4.txt"
  stats "(d) RAM per fork, 20 s after ready, over the forks of ${RUNS} runs of -n 4 (MiB)" "$RAW/ram-per-fork.txt" MiB
}

measure_ten() { # (f)
  local per avail need
  per=$(sort -n "$RAW/ram-per-fork.txt" 2>/dev/null | awk '{v[NR]=$2} END{if(NR) print v[int((NR+1)/2)]; else print 0}')
  avail=$(python3 "$CHECKS/measure_facts.py" avail-mib)
  # Needs the per-fork median of (d) times TEN, plus 25 % and 2 GiB left for the host.
  need=$(python3 -c "print(int($per * $TEN * 1.25 + 2048))")
  echo "(f) $TEN forks at once: per-fork RAM median ${per} MiB from (d); available ${avail} MiB; needed ${need} MiB (per fork x $TEN x 1.25 + 2 GiB)"
  if [ "$per" = 0 ]; then echo "(f) NOT MEASURED: no per-fork RAM from (d) to size it (section warm4 did not run)"; return; fi
  if [ "$avail" -lt "$need" ]; then echo "(f) NOT MEASURED: available memory ${avail} MiB is below the ${need} MiB the $TEN forks need"; return; fi
  : > "$RAW/ten.txt"; : > "$RAW/ten-fp.txt"
  up_main ten; sleep 3
  timed_fork ten "$RAW/ten.txt" main -n "$TEN" --name e2e-forks-ten -- true; note_checkpoint ten
  grep -qi 'ram budget\|fork_ram_budget\|memory' "$RAW/fork-ten.err" && sed -n '1,5p' "$RAW/fork-ten.err"
  sleep 20
  local total=0 up=0
  for k in $(seq "$TEN"); do
    exists e2e-forks-ten-$k || continue
    up=$((up+1)); fp e2e-forks-ten-$k > "$RAW/fp-ten-$k.txt"
    t=$(grep '^TOTAL' "$RAW/fp-ten-$k.txt" | awk '{print $2}'); echo "ten-$k $t" >> "$RAW/ten-fp.txt"
  done
  total=$(awk '{s+=$2} END{printf "%.1f", s}' "$RAW/ten-fp.txt")
  echo "(f) $up of $TEN forks exist; total RAM of them 20 s after ready: ${total} MiB (per fork $(awk '{s+=$2;n++} END{if(n) printf "%.1f", s/n}' "$RAW/ten-fp.txt") MiB)"
  stats "(f) -n $TEN from the warm checkpoint, to $TEN running+ready (one run)" "$RAW/ten.txt"
  for k in $(seq "$TEN"); do drop_one e2e-forks-ten-$k; done
}

measure_macos() { # (e), Mac only
  if [ "$OS" != Darwin ]; then echo "(e) NOT MEASURED: macOS forks run on a Mac only"; return; fi
  if [ -z "$MACOS_SOURCE" ]; then echo "(e) NOT MEASURED: SILO_MACOS_SOURCE not set"; return; fi
  if ! exists "$MACOS_SOURCE"; then echo "(e) NOT MEASURED: macOS computer $MACOS_SOURCE does not exist"; return; fi
  : > "$RAW/macos.txt"
  for i in $(seq "$RUNS"); do
    if [ "$(nvms)" -ge 2 ]; then echo "(e) NOT MEASURED (run $i): 2 macOS VMs already run"; break; fi
    local t0 s rc; t0=$(now)
    "$SILO" fork "$MACOS_SOURCE" --name e2e-forks-mac$i --gui --gui-timeout 300 --json > "$RAW/macos-fork$i.json" 2> "$RAW/macos-fork$i.err"; rc=$?
    s=$(elapsed "$t0"); echo "macos$i rc=$rc ${s}s"; [ "$rc" = 0 ] && echo "macos$i $s" >> "$RAW/macos.txt"
    drop_one e2e-forks-mac$i
  done
  [ -s "$RAW/macos.txt" ] && stats "(e) macOS fork to ready (fork --gui returns ready)" "$RAW/macos.txt"
}

for c in $(forks_names); do say "dropping leftover $c"; drop_one "$c"; done
header
ensure_main
for s in $SECTIONS; do
  echo; echo "== $s"
  case $s in
    cold) measure_cold;; warm1) measure_warm1;; warm4) measure_warm4;; ten) measure_ten;; macos) measure_macos;;
    *) echo "unknown section $s";;
  esac
done
