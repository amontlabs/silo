#!/bin/bash
# id: 25
# title: How many booted simulators fit in one computer
# tier: long
# time: about 40 minutes
# needs: the Xcode computer of test 23, macOS 27 on the Mac; KIT_SIMS sets the most simulators booted (8)
# decides: simulators-for-parallel-agents (the maximum booted at once), sizing-target (memory of main for an iOS developer)
# question: In a computer with its user on the screen, how much memory does each booted simulator with an app running take, and how many boot before the computer starts to swap or a boot fails?
# outcome measured: Simulators were booted one at a time until the computer swapped, a boot failed
#   or the limit of the run was reached. The memory each one added and the number that fit are in
#   the measures; the maximum of simulators-for-parallel-agents is set from them.
# outcome first-boot-failed: Not even one simulator booted; see test 24 first.
# outcome inconclusive: Something else went wrong; the sentence and the attached logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

ios_need_mac_or_fake "the simulator only runs on macOS"
ios_need_xcode_vm
MAX="${KIT_SIMS:-8}"
ios_fake || kit_confirm "This starts a clone of the Xcode computer, logs its user in on its screen and boots up to $MAX simulators in it, one at a time (about 40 minutes)."
trap guest_cleanup EXIT
ios_up sims
build="$(ios_build_app 2>&1)"
echo "$build" | grep -q built || { kit_outcome inconclusive "the test app did not build: $(echo "$build" | tail -1 | cut -c1-200)"; exit 0; }
kit_fact computer_memory_gb "$(guest_sh 'sysctl -n hw.memsize' | awk '{printf "%.1f", $1 / 1073741824}')"
kit_fact computer_cpus "$(guest_sh 'sysctl -n hw.ncpu')"

# Simulators need the user on the screen if test 24 says so; logging in first makes this test
# answer either way.
console="$(ios_console_user)"
if [ "$console" = none ]; then
  console="$(ios_screen_login)" || { kit_outcome inconclusive "the computer did not come back after the restart for automatic login"; exit 0; }
fi
kit_fact screen_user "$console"

ios_wait 60   # let the start settle
# shellcheck disable=SC2046
set -- $(ios_memory_used_mb)
base_used="${1:-}"; base_swap="${2:-0}"
[ -n "$base_used" ] || { kit_outcome inconclusive "could not read the computer's memory (vm_stat)"; exit 0; }
kit_measure used_mb_with_none "$base_used" MB "active + wired + compressed, no simulator booted"

log="$KIT_WORK/sims.tsv"
printf 'n\tused_mb\tswap_mb\tboot_s\n' > "$log"
n=0; prev="$base_used"; why="the run's limit of $MAX"
while [ "$n" -lt "$MAX" ]; do
  udid="$(ios_new_device "kit-$n")"
  [ -n "$udid" ] || { why="simctl create failed"; break; }
  t0="$(kit_now)"
  boot="$(guest_sh "$GUEST_LIMIT limit 400 xcrun simctl bootstatus '$udid' -b 2>&1; echo rc=\$?")"
  t1="$(kit_now)"
  echo "$boot" | grep -q 'rc=0' || { why="boot $((n + 1)) failed: $(echo "$boot" | grep -v rc= | tail -1 | cut -c1-120)"; break; }
  guest_sh "xcrun simctl install '$udid' \$HOME/silokit/KitApp.app && xcrun simctl launch '$udid' dev.silo.kitapp" >/dev/null 2>&1
  n=$((n + 1))
  ios_wait 45
  # shellcheck disable=SC2046
  set -- $(ios_memory_used_mb)
  used="${1:-$prev}"; swap="${2:-0}"
  printf '%s\t%s\t%s\t%s\n' "$n" "$used" "$swap" "$(python3 -c "print(round($t1 - $t0, 1))")" >> "$log"
  kit_log "$n booted: used $used MB (+$((used - prev))), swap $swap MB"
  prev="$used"
  if python3 -c "import sys; sys.exit(0 if $swap - $base_swap > 256 else 1)"; then why="the computer started to swap after $n"; break; fi
done
kit_attach "$log"
guest_sh 'xcrun simctl shutdown all' >/dev/null 2>&1

if [ "$n" = 0 ]; then kit_outcome first-boot-failed "$why"; exit 0; fi
per="$(python3 -c "print(round(($prev - $base_used) / $n / 1024, 2))")"
kit_measure gb_per_booted_simulator "$per" GB "average over $n, app running, after 45 s"
kit_measure simulators_booted "$n" count "$why"
kit_outcome measured "$n simulators booted ($why); each added about $per GB on average. Table in sims.tsv."
