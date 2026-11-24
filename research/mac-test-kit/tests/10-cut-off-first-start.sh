#!/bin/bash
# id: 10
# title: A first start cut off after 20 seconds
# tier: vm
# time: about 20 minutes
# needs: the base computer of test 00, macOS 27, no sudo
# decides: computer-image, step 4 of file 10 (is the never-started copy needed for retries)
# question: When a computer's first, provisioned start is forced off 20 seconds in, does starting it again with the same options still provision it?
# outcome provisioned-after-cut: The second start provisioned the computer and SSH answered. A cut-off
#   first start can be retried on the same disk; the never-started copy of computer-image is needed
#   only to make later computers.
# outcome not-provisioned: The second start ran but SSH never answered: provisioning is spent by a
#   cut-off first start. A retry must start from a fresh clone of the never-started copy (step 4),
#   so computer-image must keep it.
# outcome refused-second-start: The framework refused the provisioning options on the second start
#   (the stage and error are in the sentence). Same consequence as not-provisioned, and Silo must
#   expect that error on a retry.
# outcome inconclusive: The cut did not happen as planned (the computer stopped by itself, or never
#   started); the sentence and logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/first-start.sh"

if [ "${KIT_FAKE:-0}" = 1 ]; then
  kit_note "Linux run against linux/fake-silovm"
  CUT="${KIT_CUT_SECONDS:-2}"
else
  kit_need_mac 27
  CUT="${KIT_CUT_SECONDS:-20}"
fi
kit_need_silovm
kit_need_base_vm
fs_need_provisioning

kit_confirm "Starts one new macOS computer cloned from the base, forces it off $CUT s into its first start, and starts it again (up to $((FS_SSH_LIMIT / 60 + 1)) minutes)."

b="$(vm_fresh cut)" || kit_fail "could not clone the base"
trap 'vm_destroy "$b"' EXIT
pw="$(vm_password)"
opts="--provision-user $VM_USER --provision-password $pw --provision-full-name Silo-Kit"

# First start, cut off.
fs_forget_host_keys
# shellcheck disable=SC2086
vm_start "$b" $opts --max-seconds "$FS_MAX_SECONDS" > /dev/null
if ! fs_wait_event "$b" started 120; then
  fs_keep_log "$b" first
  kit_outcome inconclusive "the first start never reached the started event: $(fs_summary "$b"); see logs/first.*"
  exit 0
fi
sleep "$CUT"
if ! vm_running "$b"; then
  fs_keep_log "$b" first
  kit_outcome inconclusive "the computer stopped by itself within $CUT s of its first start: $(fs_summary "$b")"
  exit 0
fi
"$SILOVM" ctl --bundle "$b" stop > "$KIT_WORK/stop.json" 2>&1
i=0
while vm_running "$b" && [ "$i" -lt 30 ]; do sleep 1; i=$((i + 1)); done
vm_running "$b" && kill -9 "$(cat "$b/run.pid")" 2>/dev/null
first_how="$(vm_event "$b" stopped | python3 -c 'import json,sys; t=sys.stdin.read(); print(json.loads(t).get("how") if t.strip() else "killed")')"
kit_fact first_start_stopped "$first_how"
fs_keep_log "$b" first

# Second start, same options.
fs_first_start "$b" "$VM_USER"
kit_fact second_stop "$(vm_stop "$b" 10)"
fs_keep_log "$b" second
kit_fact second_result "$FS_RESULT"
kit_fact second_refused_stage "$FS_STAGE"
kit_fact second_error_name "$FS_NAME"

case "$FS_RESULT" in
  ssh)
    kit_measure second_start_seconds_to_ssh "$FS_SECONDS" s "after a first start cut at $CUT s"
    kit_outcome provisioned-after-cut "first start forced off at $CUT s ($first_how); the second start with the same options provisioned it, SSH after $FS_SECONDS s." ;;
  refused)
    kit_outcome refused-second-start "first start forced off at $CUT s; the second start refused provisioning at stage $FS_STAGE with $FS_NAME (code $FS_CODE)." ;;
  timeout)
    kit_outcome not-provisioned "first start forced off at $CUT s; the second start ran for $FS_SSH_LIMIT s and SSH never answered for $VM_USER." ;;
  *)
    kit_outcome inconclusive "the second start ended as $FS_RESULT (stopped: $FS_HOW); see logs/second.*" ;;
esac
