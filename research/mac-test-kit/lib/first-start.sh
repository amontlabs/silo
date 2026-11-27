# Helpers for the tests of a computer's first, provisioned start (01, 10, 13). Sourced after
# lib/common.sh and lib/vm.sh.

# How long a test waits for SSH after a start before it calls the start failed, and how long a
# computer may run at most. Short on Linux, where linux/fake-silovm stands in.
if [ "${KIT_FAKE:-0}" = 1 ]; then
  FS_SSH_LIMIT="${KIT_SSH_LIMIT:-20}"
  FS_MAX_SECONDS="${KIT_MAX_SECONDS:-120}"
else
  FS_SSH_LIMIT="${KIT_SSH_LIMIT:-900}"
  FS_MAX_SECONDS="${KIT_MAX_SECONDS:-2400}"
fi

# fs_need_provisioning: skip unless this Mac and this silovm can provision a computer.
fs_need_provisioning() {
  local hc
  hc="$("$SILOVM" host-check)" || kit_fail "silovm host-check failed"
  kit_fact host_check "$hc"
  if [ "$(printf '%s' "$hc" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("provisioning_api"))')" != True ]; then
    kit_skip "silovm cannot provision a computer here (host-check: $hc): it needs macOS 27 on the Mac and a silovm built with the macOS 27 SDK (./kit build)"
  fi
}

# Every fresh computer makes new SSH host keys, and the clones share the base's MAC address, so
# they get the same address on Apple's NAT. With a known_hosts file that holds the previous
# computer's key, OpenSSH turns password authentication off ("to avoid man-in-the-middle
# attacks") even with StrictHostKeyChecking=no, and vm_ssh_pw would never answer. So the kit's
# own known_hosts (under ~/SiloKit/ssh, never ~/.ssh) is emptied before each new computer.
fs_forget_host_keys() { rm -f "$KIT_SSH_DIR/known_hosts"; }

# fs_summary BUNDLE: one line from the run's events:
#   REFUSED_STAGE REFUSED_NAME REFUSED_CODE STARTED STOPPED_HOW
# with "-" for what did not happen. STARTED is yes or no.
fs_summary() {
  python3 - "$1/run.jsonl" <<'EOF'
import json, sys
stage = name = code = how = "-"
started = "no"
try:
    lines = open(sys.argv[1]).read().splitlines()
except OSError:
    lines = []
for line in lines:
    try: e = json.loads(line)
    except ValueError: continue
    ev = e.get("event")
    if ev == "provisioning" and e.get("ok") is False and stage == "-":
        stage, name, code = e.get("stage") or "-", e.get("name") or "-", str(e.get("code", "-"))
    elif ev == "started":
        started = "yes"
    elif ev == "stopped":
        how = e.get("how") or "-"
print(stage, name, code, started, how)
EOF
}

# fs_wait_event BUNDLE EVENT SECONDS: wait until the run printed EVENT or stopped.
fs_wait_event() {
  local b="$1" ev="$2" i=0
  while [ "$i" -lt "$3" ]; do
    [ -n "$(vm_event "$b" "$ev")" ] && return 0
    vm_running "$b" || return 1
    sleep 1; i=$((i + 1))
  done
  return 1
}

# fs_first_start BUNDLE USER [silovm run options...]: start the computer provisioned for USER and
# wait for SSH with the kit's password, or for the run to end. Sets:
#   FS_RESULT   ssh | refused | stopped | timeout
#   FS_SECONDS  seconds from the "started" event to SSH answering (ssh only)
#   FS_STAGE    validate | set | start, where provisioning was refused, or "-"
#   FS_NAME     the VZError.Code name of the refusal, or "-"
#   FS_CODE     its code, or "-"
#   FS_HOW      how the run stopped, or "-" while it runs
# The computer is left running when SSH answered or on a timeout; vm_destroy stops it.
# shellcheck disable=SC2034  # the FS_ variables are read by the tests
fs_first_start() {
  local b="$1" u="$2" s sum
  shift 2
  fs_forget_host_keys
  vm_start "$b" --provision-user "$u" --provision-password "$(vm_password)" --provision-full-name "Silo Kit" \
    --max-seconds "$FS_MAX_SECONDS" "$@" > /dev/null
  FS_RESULT=timeout FS_SECONDS="" FS_STAGE="-" FS_NAME="-" FS_CODE="-" FS_HOW="-"
  fs_wait_event "$b" started 120 || true
  if s="$(vm_wait_ssh "$b" "$u" "$FS_SSH_LIMIT")"; then
    FS_RESULT=ssh FS_SECONDS="$s"
  fi
  sum="$(fs_summary "$b")"
  # shellcheck disable=SC2086
  set -- $sum
  FS_STAGE="$1" FS_NAME="$2" FS_CODE="$3" FS_HOW="$5"
  if [ "$FS_RESULT" != ssh ]; then
    if [ "$FS_STAGE" != "-" ]; then FS_RESULT=refused
    elif ! vm_running "$b"; then FS_RESULT=stopped
    fi
  fi
  return 0
}

# fs_keep_log BUNDLE NAME: keep the run's events and log under a name of the test's choosing.
fs_keep_log() {
  local d="$KIT_WORK/logs"
  mkdir -p "$d"
  cp "$1/run.jsonl" "$d/$2.jsonl" 2>/dev/null && kit_attach "$d/$2.jsonl"
  cp "$1/run.log" "$d/$2.log" 2>/dev/null && kit_attach "$d/$2.log"
  return 0
}
