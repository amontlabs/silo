#!/usr/bin/env bash
# Silo v1 final run (slice 6b) against the release-profile Dev build. Run:
#   bash docs/v1/checks/final_run.sh [sections] 2>&1 | tee docs/v1/evidence/final/final-run.txt
# sections: any of header demo quiet cu promote cold warm4 claw macos (default: all, in this order).
# Computers: main (only if absent; dropped at the end if we made it) and e2e-final-*; nothing else is touched.
# Never more than 2 macOS VMs. Engine: the Dev silod from the release bundle (see final_lib.sh).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/final_lib.sh"
for f in demo cu measure claw macos; do [ -f "$CHECKS/final_$f.sh" ] && . "$CHECKS/final_$f.sh"; done
SECTIONS=${*:-header demo quiet cu promote cold warm4 claw macos}
cleanup() {
  say "cleanup (e2e-final-* and main if this run made it)"
  for c in $(final_names); do drop_one "$c"; done
  [ "$MADE_MAIN" = 1 ] && [ -z "$KEEP_MAIN" ] && drop_one main
  "$SILO" list; echo "macOS VMs running: $(nvms)"
  echo "== summary ($SECTIONS): $PASS passed, $FAILN failed, $BLOCKED blocked"
}
KEEP_MAIN=${KEEP_MAIN:-}
trap cleanup EXIT
for s in $SECTIONS; do
  case $s in
    header) echo "== header ($(date -u +%FT%TZ))"; header_facts;;
    demo) demo_linux;; quiet) quiet_linux;; cu) cu_linux;; promote) promote_linux;;
    cold) measure_cold;; warm4) measure_warm4;; claw) claw_linux;; macos) macos_fork;;
  esac
done
