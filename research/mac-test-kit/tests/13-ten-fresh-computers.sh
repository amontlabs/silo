#!/bin/bash
# id: 13
# title: Ten fresh computers, and one with a new identity
# tier: long
# time: about an hour (eleven first starts in sequence)
# needs: the base computer of test 00, macOS 27, no sudo
# decides: computer-image, first-run-time-macos, step 6 of file 10 (the time limit before a retry)
# question: How long does a fresh clone of the installed computer take from its first start to SSH, how often does it fail, and does a clone with a new machine identifier and auxiliary storage start and provision?
# outcome reliable-identity-ok: All ten provisioned, and the clone with a new identity did too. The
#   sentence gives the retry time limit (1.5 times the slowest, rounded up to 30 s) for step 6 and
#   the time for first-run-time-macos; computer-image can make each later computer from the
#   installed system with its own identity.
# outcome reliable-identity-fails: All ten provisioned, but the clone with a new machine identifier
#   and new auxiliary storage did not start or provision. computer-image must keep the installed
#   auxiliary storage (made by VZMacOSInstaller) for each later computer, or install again.
# outcome unreliable: At least one of the ten failed (count, reasons and identity result in the
#   sentence). Step 6's retry from the never-started copy is needed, with the limit given.
# outcome all-failed: None of the ten gave SSH: look at test 01 and the logs first.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/first-start.sh"

if [ "${KIT_FAKE:-0}" = 1 ]; then
  kit_note "Linux run against linux/fake-silovm"
else
  kit_need_mac 27
fi
kit_need_silovm
kit_need_base_vm
fs_need_provisioning

N="${KIT_FRESH_COUNT:-10}"
kit_confirm "Starts $((N + 1)) new macOS computers one after the other, each cloned from the base and deleted after (up to $((FS_SSH_LIMIT / 60 + 1)) minutes each, about an hour in all)."

b=""
trap '[ -n "$b" ] && vm_destroy "$b"' EXIT

times="" ok=0 failed=0 reasons=""
i=1
while [ "$i" -le "$N" ]; do
  label="fresh-$(printf '%02d' "$i")"
  b="$(vm_fresh "$label")" || kit_fail "could not clone the base"
  fs_first_start "$b" "$VM_USER"
  vm_stop "$b" 10 > /dev/null
  fs_keep_log "$b" "$label"
  if [ "$FS_RESULT" = ssh ]; then
    ok=$((ok + 1)); times="$times $FS_SECONDS"
    kit_measure "seconds_to_ssh_$i" "$FS_SECONDS" s "fresh clone $i"
  else
    failed=$((failed + 1)); reasons="$reasons $i:$FS_RESULT${FS_NAME#-}"
    kit_fact "fresh_${i}_failure" "$FS_RESULT stage=$FS_STAGE name=$FS_NAME stopped=$FS_HOW"
  fi
  vm_destroy "$b"; b=""
  i=$((i + 1))
done

stats=""
if [ "$ok" -gt 0 ]; then
  # shellcheck disable=SC2086
  stats="$(python3 - $times <<'EOF'
import math, statistics, sys
t = sorted(float(x) for x in sys.argv[1:])
p90 = t[min(len(t) - 1, int(math.ceil(0.9 * len(t))) - 1)]
limit = int(math.ceil(1.5 * t[-1] / 30.0) * 30)
print("%.1f %.1f %.1f %.1f %d" % (t[0], statistics.median(t), p90, t[-1], limit))
EOF
)"
  # shellcheck disable=SC2086
  set -- $stats
  kit_measure seconds_to_ssh_min "$1" s
  kit_measure seconds_to_ssh_median "$2" s
  kit_measure seconds_to_ssh_p90 "$3" s
  kit_measure seconds_to_ssh_max "$4" s
  kit_measure retry_limit_suggested "$5" s "1.5 times the slowest, rounded up to 30 s"
  summary="median $2 s, slowest $4 s, suggested limit $5 s"
fi
kit_measure fresh_failures "$failed" count "of $N"

# One clone with a new machine identifier, new auxiliary storage and a new MAC address.
b="$(vm_fresh new-identity --new-identity)" || kit_fail "could not clone the base with --new-identity"
fs_first_start "$b" "$VM_USER"
vm_stop "$b" 10 > /dev/null
fs_keep_log "$b" new-identity
id_result="$FS_RESULT"
kit_fact new_identity_result "$FS_RESULT stage=$FS_STAGE name=$FS_NAME stopped=$FS_HOW"
[ "$FS_RESULT" = ssh ] && kit_measure new_identity_seconds_to_ssh "$FS_SECONDS" s
vm_destroy "$b"; b=""

if [ "$ok" -eq 0 ]; then
  kit_outcome all-failed "none of $N fresh computers gave SSH within $FS_SSH_LIMIT s (${reasons# }); new identity: $id_result."
elif [ "$failed" -gt 0 ]; then
  kit_outcome unreliable "$failed of $N fresh computers failed (${reasons# }); the others: $summary; new identity: $id_result."
elif [ "$id_result" = ssh ]; then
  kit_outcome reliable-identity-ok "all $N fresh computers gave SSH: $summary; the clone with a new identity provisioned too."
else
  kit_outcome reliable-identity-fails "all $N fresh computers gave SSH: $summary; the clone with a new identity ended as $id_result."
fi
