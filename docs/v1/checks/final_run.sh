#!/usr/bin/env bash
# Silo v1 final run (slice 6b) against the release-profile Dev build. Run:
#   bash docs/v1/checks/final_run.sh [sections] 2>&1 | tee docs/v1/evidence/final/final-run.txt
# sections: any of header demo quiet cu promote cold warm4 claw macos (default: all, in this order).
# Computers: main (made only if absent; never dropped: left stopped for the owner to drop) and e2e-final-*;
# nothing else is touched.
# Never more than 2 macOS VMs. Engine: the Dev silod from the release bundle (see final_lib.sh).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/final_lib.sh"
for f in demo cu measure claw macos; do [ -f "$CHECKS/final_$f.sh" ] && . "$CHECKS/final_$f.sh"; done
SECTIONS=${*:-header demo quiet cu promote cold warm4 claw macos}
cleanup() {
  say "cleanup (e2e-final-*; main is stopped, not dropped)"
  for c in $(final_names); do drop_one "$c"; done
  if exists main && [ -z "$KEEP_MAIN" ]; then "$SILO" stop main >/dev/null 2>&1
    echo "OWNER: main is left $(state main) (made by this run: $MADE_MAIN); please drop it: silo drop main --force"; fi
  "$SILO" list; echo "macOS VMs running: $(nvms)"
  echo "== summary ($SECTIONS): $PASS passed, $FAILN failed, $BLOCKED blocked"
}
KEEP_MAIN=${KEEP_MAIN:-}
trap cleanup EXIT
for c in $(final_names); do say "dropping leftover $c"; drop_one "$c"; done
for s in $SECTIONS; do
  case $s in
    header) echo "== header ($(date -u +%FT%TZ))"; header_facts;;
    demo) demo_linux;; quiet) quiet_linux;; cu) cu_linux;; promote) promote_linux;;
    cold) measure_cold;; warm4) measure_warm4;; claw) claw_linux;; macos) macos_fork;;
  esac
done
