# Helpers for tests that start a macOS computer. Sourced after lib/common.sh.
# Every computer is a clone of the installed, never started base (tests/00-install-base.sh), made
# in the test's own work folder, and deleted at the end of the test unless KIT_KEEP_VMS=1.
#
# SSH never reads or writes ~/.ssh: the kit has its own key under ~/SiloKit/ssh, keeps no known
# hosts (every clone gets new host keys at the same address),
# and every ssh call passes -F /dev/null.

KIT_SSH_DIR="${KIT_WORK_ROOT:-$HOME/SiloKit}/ssh"

# The password every provisioned user of this kit gets. Generated once, kept in ~/SiloKit/ssh.
vm_password() {
  mkdir -p "$KIT_SSH_DIR"; chmod 700 "$KIT_SSH_DIR"
  if [ ! -s "$KIT_SSH_DIR/password" ]; then
    (umask 077; python3 -c 'import secrets; print(secrets.token_urlsafe(18))' > "$KIT_SSH_DIR/password")
  fi
  cat "$KIT_SSH_DIR/password"
}

# The user name tests provision when the name itself is not what is tested.
VM_USER="${KIT_VM_USER:-silokit}"

_vm_key() {
  mkdir -p "$KIT_SSH_DIR"; chmod 700 "$KIT_SSH_DIR"
  [ -f "$KIT_SSH_DIR/id_ed25519" ] || ssh-keygen -q -t ed25519 -N '' -C silo-kit -f "$KIT_SSH_DIR/id_ed25519"
  echo "$KIT_SSH_DIR/id_ed25519"
}

_vm_askpass() {
  local f="$KIT_SSH_DIR/askpass.sh"
  if [ ! -x "$f" ]; then
    printf '#!/bin/sh\ncat "%s"\n' "$KIT_SSH_DIR/password" > "$f"; chmod 700 "$f"
  fi
  echo "$f"
}

_vm_ssh_opts() {
  echo "-F /dev/null -o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no -o LogLevel=ERROR -o ConnectTimeout=5 -o ServerAliveInterval=5 -o ServerAliveCountMax=3"
}

# vm_fresh NAME [--new-identity]: clone the base into the test's work folder; prints the bundle.
vm_fresh() {
  kit_need_silovm; kit_need_base_vm
  local b="$KIT_WORK/$1.silovm"; shift
  rm -rf "$b"
  "$SILOVM" clone --from "$KIT_BASE_VM" --to "$b" "$@" >&2 || kit_fail "silovm clone failed"
  echo "$b"
}

# vm_start BUNDLE [silovm run options...]: start in the background. Events go to BUNDLE/run.jsonl,
# stderr to BUNDLE/run.log. Prints the pid of silovm run.
vm_start() {
  local b="$1"; shift
  : > "$b/run.jsonl"
  "$SILOVM" run --bundle "$b" "$@" > "$b/run.jsonl" 2> "$b/run.log" &
  echo $! > "$b/run.pid"
  echo $!
}

# vm_provision_args [USER]: the provisioning options with the kit's password.
vm_provision_args() {
  echo "--provision-user ${1:-$VM_USER} --provision-password $(vm_password) --provision-full-name Silo-Kit"
}

# vm_running BUNDLE: true while silovm run is alive.
vm_running() { [ -f "$1/run.pid" ] && kill -0 "$(cat "$1/run.pid")" 2>/dev/null; }

# vm_event BUNDLE NAME: print the last event NAME of the run, as JSON, or nothing.
vm_event() {
  python3 - "$1/run.jsonl" "$2" <<'EOF'
import json, sys
last = None
try:
    for line in open(sys.argv[1]):
        try: e = json.loads(line)
        except ValueError: continue
        if e.get("event") == sys.argv[2]: last = e
except OSError: pass
if last is not None: print(json.dumps(last))
EOF
}

# vm_ip BUNDLE [WAIT]: the computer's address on Apple's NAT.
vm_ip() { "$SILOVM" ip --bundle "$1" --wait "${2:-120}"; }

# vm_ssh_pw BUNDLE USER cmd...: one command over SSH with the password (before the key is set).
vm_ssh_pw() {
  local b="$1" u="$2"; shift 2
  local ip; ip="$(vm_ip "$b" 5)" || return 255
  vm_password >/dev/null
  # shellcheck disable=SC2046
  SSH_ASKPASS="$(_vm_askpass)" SSH_ASKPASS_REQUIRE=force DISPLAY=none \
    ssh $(_vm_ssh_opts) -o PreferredAuthentications=password,keyboard-interactive -o PubkeyAuthentication=no \
    "$u@$ip" "$@" < /dev/null
}

# vm_ssh BUNDLE USER cmd...: one command over SSH with the kit's key. Its stdin is the caller's
# when that is a pipe or a file (a copy, a password); never the terminal, where ssh would swallow
# what the person types for the kit's next question.
vm_ssh() {
  local b="$1" u="$2"; shift 2
  local ip; ip="$(vm_ip "$b" 5)" || return 255
  if [ -t 0 ]; then
    # shellcheck disable=SC2046
    ssh $(_vm_ssh_opts) -o BatchMode=yes -i "$(_vm_key)" "$u@$ip" "$@" < /dev/null
  else
    # shellcheck disable=SC2046
    ssh $(_vm_ssh_opts) -o BatchMode=yes -i "$(_vm_key)" "$u@$ip" "$@"
  fi
}

# vm_wait_ssh BUNDLE USER TIMEOUT: wait until SSH with the password answers. Prints the seconds
# waited, counted from the "started" event. Returns 1 on timeout or when the run stopped.
vm_wait_ssh() {
  local b="$1" u="$2" limit="$3" t0 now
  t0="$(kit_now)"
  while :; do
    vm_running "$b" || return 1
    if vm_ssh_pw "$b" "$u" true 2>/dev/null; then break; fi
    now="$(kit_now)"
    python3 -c "import sys; sys.exit(0 if $now - $t0 < $limit else 1)" || return 1
    sleep 3
  done
  python3 - "$b/run.jsonl" "$(kit_now)" <<'EOF'
import json, sys
t = None
for line in open(sys.argv[1]):
    try: e = json.loads(line)
    except ValueError: continue
    if e.get("event") == "started": t = e.get("t")
print("%.1f" % (float(sys.argv[2]) - t) if t else "nan")
EOF
}

# vm_setup_key BUNDLE USER: put the kit's key in the user's authorized_keys, with the password.
vm_setup_key() {
  local pub; pub="$(cat "$(_vm_key).pub")"
  vm_ssh_pw "$1" "$2" "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '$pub' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys"
}

# vm_ready BUNDLE [USER] [TIMEOUT]: wait for SSH, then set the key. Prints the seconds to SSH.
vm_ready() {
  local s
  s="$(vm_wait_ssh "$1" "${2:-$VM_USER}" "${3:-900}")" || return 1
  vm_setup_key "$1" "${2:-$VM_USER}" >/dev/null || return 1
  echo "$s"
}

# vm_sudo BUNDLE USER cmd: run cmd as root inside the computer, with the user's password on stdin.
# This is sudo inside the guest, never on the Mac.
vm_sudo() {
  local b="$1" u="$2"; shift 2
  vm_password | vm_ssh "$b" "$u" "sudo -S -p '' sh -c $(printf '%q' "$*")"
}

# vm_stop BUNDLE [GRACE]: ask the guest to stop, wait GRACE seconds (30), then force it.
# Prints how it stopped: guest, forced, or already.
vm_stop() {
  local b="$1" grace="${2:-30}" i=0
  vm_running "$b" || { echo already; return 0; }
  "$SILOVM" ctl --bundle "$b" request-stop >/dev/null 2>&1
  while vm_running "$b" && [ "$i" -lt "$grace" ]; do sleep 1; i=$((i + 1)); done
  if vm_running "$b"; then
    "$SILOVM" ctl --bundle "$b" stop >/dev/null 2>&1
    sleep 2
    vm_running "$b" && kill -9 "$(cat "$b/run.pid")" 2>/dev/null
    echo forced
  else
    echo guest
  fi
}

# vm_destroy BUNDLE: stop it if needed and delete it, unless KIT_KEEP_VMS=1.
vm_destroy() {
  vm_stop "$1" 5 >/dev/null
  if [ "${KIT_KEEP_VMS:-0}" = 1 ]; then kit_note "kept $1"; else rm -rf "$1"; fi
}
