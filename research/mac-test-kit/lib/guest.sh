# shellcheck shell=bash disable=SC2034  # its variables are read by the tests that source it
# Helpers for the tests that work inside a provisioned macOS computer (02 09 11 12 14 16 21 22).
# Sourced after lib/common.sh and lib/vm.sh. Written for macOS /bin/bash 3.2.
#
# One computer at a time is "the current one": GB holds its bundle. guest_sh and guest_sudo run a
# script in it, as the provisioned user or as root inside it (never on the Mac).
#
# On Linux (KIT_FAKE=1, or not a Mac) there is no computer: guest_sh and guest_sudo run the script
# here, in a shell whose HOME is a folder of the test's work folder, so a test's logic runs as its
# Linux analogue. Commands that only exist on macOS are the test's to avoid in that case.

GB=""
GUEST_BUNDLES=""
GUEST_HOME="${KIT_WORK:-/tmp}/guest-home"

guest_local() { [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; }

# On Linux, the tests that start computers use a small fake of silovm (no VM, no network).
if guest_local; then
  SILOVM="$KIT_ROOT/linux/fake-silovm-guest"
  KIT_BASE_VM="${KIT_WORK:-/tmp}/fake-base.silovm"
  mkdir -p "$KIT_BASE_VM" "$GUEST_HOME" 2>/dev/null
  [ -f "$KIT_BASE_VM/config.json" ] || echo '{"mac":"fa:ke:00:00:00:01","cpus":2,"memory_gb":4,"disk_format":"raw"}' > "$KIT_BASE_VM/config.json"
  [ -f "$KIT_BASE_VM/disk.img" ] || : > "$KIT_BASE_VM/disk.img"
fi

# Every fresh computer is a clone of the same base: the same MAC address, so the same address on
# Apple's NAT, and new SSH host keys. OpenSSH then sees a changed host key and, with
# StrictHostKeyChecking=no, turns password authentication and port forwarding off, which
# vm_wait_ssh needs. So the kit's known_hosts is emptied before each new computer.
guest_forget_hosts() { mkdir -p "$KIT_SSH_DIR" && : > "$KIT_SSH_DIR/known_hosts"; }

# guest_cleanup: stop and delete every computer guest_up or guest_track registered. For traps.
guest_cleanup() {
  local b
  for b in $GUEST_BUNDLES; do
    [ -d "$b" ] && vm_destroy "$b"
  done
  GUEST_BUNDLES=""
}

guest_track() { GUEST_BUNDLES="$GUEST_BUNDLES $1"; }

# _guest_fail_with_logs BUNDLE "reason": keep the run's logs, then fail the test.
_guest_fail_with_logs() {
  [ -f "$1/run.log" ] && kit_attach "$1/run.log"
  [ -f "$1/run.jsonl" ] && kit_attach "$1/run.jsonl"
  kit_fail "$2"
}

# guest_wait_started BUNDLE SECONDS: wait for the "started" event. 0 when it came, 1 when the run
# ended first or the time ran out.
guest_wait_started() {
  local b="$1" n=0
  while [ "$n" -lt "$2" ]; do
    [ -n "$(vm_event "$b" started)" ] && return 0
    vm_running "$b" || { [ -n "$(vm_event "$b" started)" ] && return 0; return 1; }
    sleep 1; n=$((n + 1))
  done
  return 1
}

# guest_up NAME [silovm run options]: a fresh clone of the base, first start with provisioning,
# wait for SSH and set the kit's key. Sets GB and GUEST_SSH_SECONDS. Fails the test on a refusal.
guest_up() {
  local name="$1"; shift
  kit_need_silovm; kit_need_base_vm
  GB="$KIT_WORK/$name.silovm"
  rm -rf "$GB"
  "$SILOVM" clone --from "$KIT_BASE_VM" --to "$GB" >&2 || kit_fail "silovm clone failed"
  guest_track "$GB"
  guest_forget_hosts
  # shellcheck disable=SC2046
  vm_start "$GB" $(vm_provision_args) "$@" >/dev/null
  if guest_local; then
    guest_wait_started "$GB" 20 || _guest_fail_with_logs "$GB" "the fake computer did not start"
    GUEST_SSH_SECONDS=0
    return 0
  fi
  kit_log "waiting for the computer to finish its first start and answer SSH (up to 15 minutes)"
  GUEST_SSH_SECONDS="$(vm_ready "$GB" "$VM_USER" 900)" ||
    _guest_fail_with_logs "$GB" "the provisioned computer did not answer SSH within 15 minutes"
}

# guest_start BUNDLE [silovm run options]: start an already provisioned computer, wait for SSH with
# the kit's key. Sets GB and GUEST_SSH_SECONDS. Returns 1 when it does not come up in 10 minutes.
guest_start() {
  local b="$1"; shift
  GB="$b"
  vm_start "$b" "$@" >/dev/null
  if guest_local; then
    guest_wait_started "$b" 20 || return 1
    GUEST_SSH_SECONDS=0
    return 0
  fi
  GUEST_SSH_SECONDS="$(vm_wait_ssh "$b" "$VM_USER" 600)" || return 1
}

# guest_clone FROM TO [--new-identity]: clone a stopped bundle (a checkpoint, or a fork with
# --new-identity) and register it for cleanup.
guest_clone() {
  local from="$1" to="$2"; shift 2
  rm -rf "$to"
  "$SILOVM" clone --from "$from" --to "$to" "$@" >&2 || return 1
  rm -f "$to/run.pid" "$to/ctl.sock"
  guest_track "$to"
}

# The guest's PATH for the tools the kit installs (zsh reads no profile for a command over SSH).
GUEST_PATH='export PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH";'

# guest_sh "script": run a POSIX sh script in the current computer as the provisioned user. The
# script's stdin is /dev/null, so it never reads the terminal. Exit status is the script's.
guest_sh() {
  if guest_local; then
    HOME="$GUEST_HOME" /bin/sh -c "$GUEST_PATH $1" < /dev/null
  else
    vm_ssh "$GB" "$VM_USER" "/bin/sh -c $(printf '%q' "$GUEST_PATH $1")" < /dev/null
  fi
}

# guest_sudo "script": run a POSIX sh script as root inside the current computer.
guest_sudo() {
  if guest_local; then
    HOME="$GUEST_HOME" /bin/sh -c "$1" < /dev/null
  else
    vm_sudo "$GB" "$VM_USER" "$1"
  fi
}

# guest_put LOCAL REMOTE: copy one file into the computer (REMOTE relative to the user's home).
guest_put() {
  if guest_local; then
    (cd "$GUEST_HOME" && mkdir -p "$(dirname "$2")" && cat > "$2") < "$1"
  else
    vm_ssh "$GB" "$VM_USER" "mkdir -p \"\$(dirname '$2')\" && cat > '$2'" < "$1"
  fi
}

# guest_get REMOTE LOCAL: copy one file out of the computer.
guest_get() {
  if guest_local; then
    (cd "$GUEST_HOME" && cat "$1") > "$2"
  else
    vm_ssh "$GB" "$VM_USER" "cat '$1'" < /dev/null > "$2"
  fi
}

# GUEST_LIMIT: a shell function for guest scripts, "limit SECONDS cmd...": runs cmd, ends it after
# SECONDS (macOS has no timeout command). Status 124 when it was ended. Prepend it to a script.
GUEST_LIMIT='limit() { _n="$1"; shift; "$@" & _p=$!; ( sleep "$_n"; touch "$HOME/.silokit-limit-$_p"; kill -TERM "$_p" 2>/dev/null || rm -f "$HOME/.silokit-limit-$_p"; sleep 3; kill -KILL "$_p" 2>/dev/null ) >/dev/null 2>&1 & _w=$!; wait "$_p"; _r=$?; kill "$_w" 2>/dev/null; if [ -f "$HOME/.silokit-limit-$_p" ]; then rm -f "$HOME/.silokit-limit-$_p"; _r=124; fi; return "$_r"; };'

# guest_install_claude: Claude Code's documented native installer (code.claude.com/docs/en/setup),
# into ~/.local/bin. Prints the version. Auto-update stays as installed: it is what a person gets.
guest_install_claude() {
  guest_sh 'command -v claude >/dev/null 2>&1 || curl -fsSL https://claude.ai/install.sh | bash >/dev/null 2>&1; claude --version 2>/dev/null | head -1'
}

# guest_install_codex: Codex's documented standalone installer (github.com/openai/codex README),
# into ~/.local/bin, with its prompts off. Prints the version.
guest_install_codex() {
  guest_sh 'command -v codex >/dev/null 2>&1 || curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_NON_INTERACTIVE=1 sh >/dev/null 2>&1; codex --version 2>/dev/null | head -1'
}

# guest_busy_start: one CPU loop per processor and a write-and-sync loop on the disk, detached, so
# the computer is busy when it is asked to stop. guest_busy_stop ends them.
guest_busy_start() {
  guest_sh 'n=$(sysctl -n hw.ncpu 2>/dev/null || nproc); i=0
while [ "$i" -lt "$n" ]; do nohup yes > /dev/null 2>&1 < /dev/null & i=$((i + 1)); done
nohup /bin/sh -c "while :; do dd if=/dev/zero of=\$HOME/silokit-busy.bin bs=1048576 count=256 2>/dev/null; sync; rm -f \$HOME/silokit-busy.bin; done" > /dev/null 2>&1 < /dev/null &
echo "$n"'
}
guest_busy_stop() { guest_sh 'pkill -x yes; pkill -f silokit-busy; rm -f "$HOME/silokit-busy.bin"; true'; }

# guest_boot_time: the computer's boot time in seconds since 1970 (kern.boottime).
guest_boot_time() {
  if guest_local; then python3 -c 'import time; print(int(time.time() - float(open("/proc/uptime").read().split()[0])))'; return; fi
  guest_sh 'sysctl -n kern.boottime' 2>/dev/null | sed -n 's/^{ sec = \([0-9]*\),.*/\1/p'
}

# guest_reboot: restart the current computer from inside (sudo shutdown -r now) and wait until it
# answers SSH with a new boot time. Prints how it came back: "in-place" when silovm kept running
# through the restart, "vm-stopped" when the restart ended the run and the kit started it again.
# Returns 1 when it does not come back within 10 minutes.
guest_reboot() {
  local before after t0 mode=in-place
  # Never on Linux: guest_sudo runs here there, and this would restart this machine.
  if guest_local; then echo skipped; return 0; fi
  before="$(guest_boot_time)"
  guest_sudo 'shutdown -r now' >/dev/null 2>&1
  t0="$(kit_now)"
  sleep 10
  while python3 -c "import sys,time; sys.exit(0 if time.time() - $t0 < 600 else 1)"; do
    if ! vm_running "$GB" && [ "$mode" = in-place ]; then
      mode=vm-stopped
      kit_note "the restart ended silovm run ($(vm_event "$GB" stopped)); starting the computer again"
      vm_start "$GB" >/dev/null
    fi
    after="$(guest_boot_time 2>/dev/null)"
    if [ -n "$after" ] && [ "$after" != "$before" ]; then echo "$mode"; return 0; fi
    sleep 5
  done
  echo "$mode"
  return 1
}

# guest_ssh_command [extra ssh options]: the command line a person pastes into Terminal on the Mac
# to reach the current computer as the provisioned user, with the kit's key. Uses no ~/.ssh.
guest_ssh_command() {
  local ip; ip="$(vm_ip "$GB" 30)" || return 1
  echo "ssh -F /dev/null -o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no -o LogLevel=ERROR -i $(_vm_key) -t $* $VM_USER@$ip"
}

# guest_make_test_ca DIR: a test certificate authority and one server certificate for the
# providers' host names, made with openssl on this machine (LibreSSL on a Mac). Writes ca.crt,
# leaf.crt, leaf.key and chain.crt into DIR. Only for tests inside a computer; nothing is trusted
# on the Mac.
guest_make_test_ca() {
  mkdir -p "$1" || return 1
  (
    cd "$1" || exit 1
    cat > ca.cnf <<'EOF'
[req]
distinguished_name = dn
prompt = no
[dn]
CN = Silo Kit Test CA
[v3_ca]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
EOF
    cat > leaf.cnf <<'EOF'
[req]
distinguished_name = dn
prompt = no
[dn]
CN = api.anthropic.com
[v3_leaf]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:api.anthropic.com, DNS:claude.ai, DNS:platform.claude.com, DNS:statsig.anthropic.com, DNS:api.openai.com, DNS:chatgpt.com, DNS:auth.openai.com, DNS:ab.chatgpt.com
authorityKeyIdentifier = keyid
EOF
    openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.crt -days 30 -config ca.cnf -extensions v3_ca 2>/dev/null &&
      openssl req -new -newkey rsa:2048 -nodes -keyout leaf.key -out leaf.csr -config leaf.cnf 2>/dev/null &&
      openssl x509 -req -in leaf.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out leaf.crt -days 30 -extfile leaf.cnf -extensions v3_leaf 2>/dev/null &&
      cat leaf.crt ca.crt > chain.crt
  )
}

# The host names guest_make_test_ca's certificate covers, for /etc/hosts lines.
GUEST_TLS_HOSTS="api.anthropic.com claude.ai platform.claude.com statsig.anthropic.com api.openai.com chatgpt.com auth.openai.com ab.chatgpt.com"

# guest_tls_verdict LOGFILE: read an "openssl s_server -state" log and print what clients did:
# accepted (a client finished the handshake, so it trusted the certificate), refused (a client
# sent a fatal alert), both, or none (no client connected).
guest_tls_verdict() {
  local ok=0 no=0
  grep -qi 'read finished' "$1" && ok=1
  grep -qi 'alert read:fatal' "$1" && no=1
  case "$ok$no" in
    10) echo accepted ;;
    01) echo refused ;;
    11) echo both ;;
    *) echo none ;;
  esac
}

# guest_redact: drop anything that looks like a token or an e-mail address from text kept in the
# results (a status line, an error). Reads stdin.
guest_redact() {
  sed -E 's/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+/<email>/g; s/[A-Za-z0-9_+=-]{32,}/<long-string>/g'
}
