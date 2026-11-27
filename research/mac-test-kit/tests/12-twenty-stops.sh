#!/bin/bash
# id: 12
# title: Twenty stops of a busy macOS computer
# tier: long
# time: about 1.5 hours
# needs: the installed computer of test 00, macOS 27 on the Mac, no other macOS VM running (UTM, Tart, Parallels)
# step: 2
# decides: what-stop-ends, neutral-core (a refused start taken as a fact)
# question: Asked to stop while busy, how often does a macOS 27 guest shut down by itself within 30 seconds, and does a stop ever leave one of Apple's two places for macOS computers taken?
# outcome clean-stops: At least 18 of 20 busy guests shut down within 30 s of request-stop and no
#   place leaked: what-stop-ends' "ask, wait 30 s, then end it" holds on macOS as written; the 30 s is
#   rarely reached (the facts give the slowest stop).
# outcome slow-stops: More than 2 of 20 needed the forced end after 30 s, and no place leaked: a stop
#   of main usually costs the full 30 s, so a restore waits that long; what-stop-ends should say so,
#   or Silo shuts the guest down over SSH first.
# outcome place-leaks: After a stop, starting two computers failed at least once with the limit
#   error: the leak reported on macOS 26.5 is still there on this Mac. neutral-core must take a
#   refused start as a fact and say "restart the Mac"; the facts say after which stop it began.
# outcome inconclusive: Fewer than 10 rounds completed (a start failed for another reason, or
#   another VM app held a place); read the attached rounds log.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

ROUNDS="${KIT_STOP_ROUNDS:-20}"
GRACE=30
if guest_local; then
  ROUNDS="${KIT_STOP_ROUNDS:-6}"
  # The fake ignores the stop request of every second run (forced) and leaks a place on its seventh stop.
  export FAKE_SILOVM_STATE="$KIT_WORK/fake-state" FAKE_STOP_IGNORE_EVERY=2 FAKE_LEAK_EVERY=7
  rm -rf "$FAKE_SILOVM_STATE"
  GRACE=2
  KIT_BUSY_SECONDS=1
  kit_note "Linux run: a fake silovm that ignores every second stop request and leaks a place on its seventh stop; $ROUNDS rounds"
else
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  kit_confirm "This starts a macOS computer $ROUNDS times (clones, deleted at the end), each time with two short extra starts to count free places, for about 1.5 hours. Quit UTM, Tart or Parallels first."
fi

# vz_processes: how many Virtualization XPC processes run on the Mac (one per running VM).
vz_processes() {
  if guest_local; then find "$FAKE_SILOVM_STATE" -name 'place-*' | wc -l | tr -d ' '; return; fi
  pgrep -f com.apple.Virtualization.VirtualMachine | wc -l | tr -d ' '
}
before="$(vz_processes)"
kit_measure vz_processes_before "$before" count "Virtualization processes before the test (should be 0)"
[ "$before" = 0 ] || kit_note "another VM is running on this Mac: the place counts below include it"

trap guest_cleanup EXIT
# One provisioned computer, stopped: every round starts a clone of it, so each round boots a
# computer that already has its user, key and host keys.
guest_up template
s="$(vm_stop "$GB" 60)"
kit_fact template_stop "$s"
TEMPLATE="$GB"

# probe_places: start two computers at once and see whether both start. Prints "2" when both
# started, or how many did, then a refusal's error.
probe_places() {
  local p started=0 err=""
  for p in a b; do
    guest_clone "$TEMPLATE" "$KIT_WORK/probe-$p.silovm" --new-identity >/dev/null 2>&1 || { err="clone failed"; continue; }
    vm_start "$KIT_WORK/probe-$p.silovm" --net none >/dev/null
  done
  for p in a b; do
    if guest_wait_started "$KIT_WORK/probe-$p.silovm" 30; then
      started=$((started + 1))
    else
      err="$err $(vm_event "$KIT_WORK/probe-$p.silovm" stopped | cut -c1-200)"
    fi
  done
  for p in a b; do
    "$SILOVM" ctl --bundle "$KIT_WORK/probe-$p.silovm" stop >/dev/null 2>&1
  done
  sleep 2
  for p in a b; do
    vm_running "$KIT_WORK/probe-$p.silovm" && kill -9 "$(cat "$KIT_WORK/probe-$p.silovm/run.pid")" 2>/dev/null
    rm -rf "$KIT_WORK/probe-$p.silovm"
  done
  echo "$started$err"
}

rounds_log="$KIT_WORK/rounds.tsv"
printf 'round\tstart\tssh_s\tstop_how\tstop_s\tstopped_event\tvz_after\tprobe\n' > "$rounds_log"
guest_n=0; forced_n=0; refused_start=0; leak_after=""; done_n=0; slowest=0
i=1
while [ "$i" -le "$ROUNDS" ]; do
  b="$KIT_WORK/round.silovm"
  guest_clone "$TEMPLATE" "$b" || kit_fail "clone of the template failed"
  guest_forget_hosts
  if ! guest_start "$b"; then
    ev="$(vm_event "$b" stopped)"
    printf '%s\trefused\t\t\t\t%s\t%s\t\n' "$i" "$ev" "$(vz_processes)" >> "$rounds_log"
    refused_start=$((refused_start + 1))
    kit_note "round $i: the computer did not come up: $ev"
    vm_destroy "$b"
    i=$((i + 1)); continue
  fi
  guest_busy_start >/dev/null
  sleep "${KIT_BUSY_SECONDS:-20}"
  t0="$(kit_now)"
  how="$(vm_stop "$b" "$GRACE")"
  t1="$(kit_now)"
  guest_local && guest_busy_stop
  secs="$(python3 -c "print('%.1f' % ($t1 - $t0))")"
  ev="$(vm_event "$b" stopped)"
  case "$how" in
    guest) guest_n=$((guest_n + 1)) ;;
    *) forced_n=$((forced_n + 1)) ;;
  esac
  slowest="$(python3 -c "print(max($slowest, $secs) if '$how' == 'guest' else $slowest)")"
  vm_destroy "$b"
  sleep 3
  vz="$(vz_processes)"
  probe="$(probe_places)"
  case "$probe" in
    2*) ;;
    *) [ -n "$leak_after" ] || leak_after="$i" ;;
  esac
  printf '%s\tok\t%s\t%s\t%s\t%s\t%s\t%s\n' "$i" "$GUEST_SSH_SECONDS" "$how" "$secs" "$ev" "$vz" "$probe" >> "$rounds_log"
  kit_log "round $i: stop $how in $secs s; Virtualization processes after: $vz; probe: $probe"
  done_n=$((done_n + 1))
  i=$((i + 1))
done
kit_attach "$rounds_log"

kit_measure rounds_done "$done_n" count
kit_measure guest_stops "$guest_n" count "shut down within $GRACE s of request-stop"
kit_measure forced_stops "$forced_n" count "ended after $GRACE s"
kit_measure slowest_guest_stop "$slowest" s
kit_measure refused_starts "$refused_start" count
python3 - "$rounds_log" <<'EOF' > "$KIT_WORK/stop-stats.txt"
import sys
rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1])][1:]
s = sorted(float(r[4]) for r in rows if len(r) > 4 and r[1] == "ok" and r[3] == "guest" and r[4])
print("%.1f" % s[len(s) // 2] if s else "nan")
EOF
median="$(cat "$KIT_WORK/stop-stats.txt")"
[ "$median" = nan ] || kit_measure median_guest_stop "$median" s
kit_fact first_leak_after_round "${leak_after:-none}"

text="$guest_n of $done_n rounds shut down within $GRACE s (median ${median} s, slowest $slowest s), $forced_n forced, $refused_start starts refused; first failed two-place probe after round ${leak_after:-none}."
if [ "$done_n" -lt $((ROUNDS / 2)) ]; then
  kit_outcome inconclusive "$text"
elif [ -n "$leak_after" ]; then
  kit_outcome place-leaks "$text"
elif [ "$forced_n" -gt 2 ]; then
  kit_outcome slow-stops "$text"
else
  kit_outcome clean-stops "$text"
fi
