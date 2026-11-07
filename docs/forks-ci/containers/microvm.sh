#!/usr/bin/env bash
# The microVM side of phase C1 on the same runner: the same developer workload as boot/workload.sh in forks of a
# Silo Linux computer, the cold start of a new computer, and (docs/forks-ci/checks/measure_forks.sh) the fork times.
#
#   SILO=/path/to/silo OUT=dir bash docs/forks-ci/containers/microvm.sh
#
# The engine must already be running (the workflow starts it). Computers it makes are named e2e-c1-*; `main` is
# made by `silo up --recipe demo` and dropped at the end only if this run made it.
set -uo pipefail
C1=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$C1/../../.." && pwd)
SILO=${SILO:?SILO must name the silo CLI}
OUT=${OUT:-$REPO/docs/forks-ci/evidence/c1-containers/microvm}
RUNS=${RUNS:-3}
PROJ=$REPO/docs/v1/checks/final-proj
RAW=$OUT/raw
mkdir -p "$RAW"
: > "$OUT/summary.txt"
now() { date +%s.%N; }
sub() { awk -v a="$1" -v b="$2" 'BEGIN{printf "%.2f", b-a}'; }
say() { echo "[$(date +%H:%M:%S)] $*"; }
summ() { python3 "$REPO/docs/forks-ci/checks/measure_stats.py" "$@" | tee -a "$OUT/summary.txt"; }
exists() { "$SILO" status "$1" >/dev/null 2>&1; }
drop_one() {
  case $1 in e2e-c1-*) ;; *) echo "refusing to drop $1"; return 1;; esac
  exists "$1" || return 0
  "$SILO" stop "$1" >/dev/null 2>&1
  for _ in $(seq 20); do "$SILO" drop "$1" --force >/dev/null 2>&1 && return 0; exists "$1" || return 0; sleep 3; done
}
MADE_MAIN=0
cleanup() {
  for n in $("$SILO" list 2>/dev/null | awk '$1 ~ /^e2e-c1-/{print $1}'); do drop_one "$n"; done
  if [ "$MADE_MAIN" = 1 ] && exists main; then
    "$SILO" stop main >/dev/null 2>&1
    for _ in $(seq 20); do "$SILO" drop main --force >/dev/null 2>&1 && break; exists main || break; sleep 3; done
  fi
}
trap cleanup EXIT

{
  echo "date: $(date -u +%FT%TZ)"
  python3 "$REPO/docs/forks-ci/checks/measure_facts.py"
  echo "silo: $SILO ($("$SILO" --version 2>&1 | head -1))"
  echo "scripts commit: $(git -C "$REPO" rev-parse HEAD 2>/dev/null)"
} | tee "$RAW/facts.txt"

if ! exists main; then MADE_MAIN=1; say "making main with silo up --recipe demo"; (cd "$PROJ" && "$SILO" up --recipe demo > "$RAW/up-main.txt" 2>&1); tail -n 3 "$RAW/up-main.txt"; fi
"$SILO" list

# Cold start of a new computer: create, then start to a command that runs.
: > "$RAW/create.txt"; : > "$RAW/start.txt"; : > "$RAW/exec.txt"; : > "$RAW/start-ready.txt"; : > "$RAW/newvm-ram.txt"
for i in $(seq "$RUNS"); do
  n=e2e-c1-new$i
  t0=$(now); "$SILO" create "$n" --linux --cpus 2 --memory 4 --disk 8 > "$RAW/create-$i.out" 2>&1; t1=$(now)
  "$SILO" start "$n" > "$RAW/start-$i.out" 2>&1; t2=$(now)
  "$SILO" exec "$n" -- true > "$RAW/exec-$i.out" 2>&1; t3=$(now)
  echo "new$i $(sub "$t0" "$t1")" >> "$RAW/create.txt"
  echo "new$i $(sub "$t1" "$t2")" >> "$RAW/start.txt"
  echo "new$i $(sub "$t2" "$t3")" >> "$RAW/exec.txt"
  echo "new$i $(sub "$t1" "$t3")" >> "$RAW/start-ready.txt"
  sleep 20
  echo "new$i $(python3 "$REPO/docs/forks-ci/checks/measure_footprint.py" "$n" | tee "$RAW/ram-new$i.txt" | awk '$1=="TOTAL"{print $2}')" >> "$RAW/newvm-ram.txt"
  drop_one "$n"
done
summ "microVM: create a new computer" "$RAW/create.txt"
summ "microVM: start of a new computer (silo start)" "$RAW/start.txt"
summ "microVM: first silo exec after start" "$RAW/exec.txt"
summ "microVM: start + first exec = cold start to a ready shell" "$RAW/start-ready.txt"
summ "microVM: PSS of a new computer 20 s after ready (MiB)" "$RAW/newvm-ram.txt" MiB

# The developer workload, fresh fork of main each run.
B64=$(base64 < "$C1/boot/workload.sh" | tr -d '\n')
: > "$RAW/work-total.txt"
for i in $(seq "$RUNS"); do
  n=e2e-c1-w$i
  (cd "$PROJ" && "$SILO" fork main --name "$n" -- true > "$RAW/fork-w$i.out" 2>&1)
  t0=$(now)
  "$SILO" exec "$n" --timeout 2400 -- bash -c "echo $B64 | base64 -d > /tmp/workload.sh; if [ \$(id -u) = 0 ]; then HOME=/root bash /tmp/workload.sh; else sudo -n -H bash /tmp/workload.sh; fi" > "$RAW/work-$i.txt" 2> "$RAW/work-$i.err"
  echo "work$i $(sub "$t0" "$(now)")" >> "$RAW/work-total.txt"
  grep -E '^(PHASE|PHASE_TAIL|WORKLOAD_DONE)' "$RAW/work-$i.txt" | cut -c1-300
  drop_one "$n"
done
for phase in git_clone_express apt_update apt_install npm_install rustup_install cargo_install_hexyl git_clone_zlib make_zlib; do
  grep -h "^PHASE $phase " "$RAW"/work-[0-9].txt 2>/dev/null | awk '$4==0{print $2, $3}' > "$RAW/work-phase-$phase.txt"
  [ -s "$RAW/work-phase-$phase.txt" ] && summ "microVM workload: $phase" "$RAW/work-phase-$phase.txt"
done
summ "microVM workload: whole run" "$RAW/work-total.txt"

# The F1 fork measurements again, on this run's engine.
say "fork measurements (docs/forks-ci/checks/measure_forks.sh)"
SILO=$SILO MACHINE=github-ubuntu-24.04-kvm OUT=$OUT/f1-rerun bash "$REPO/docs/forks-ci/checks/measure_forks.sh" 2>&1 | tee "$OUT/measure-forks.log"
