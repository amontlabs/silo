#!/usr/bin/env bash
# Two devices on one Linux machine: a host account prepared with `silo host init` and a
# controller account with its own engine that adds the host by address over the local SSH
# server, then makes, starts, uses and drops a computer on the host with `silo --device`.
# Nothing asks a question: the controller's key goes to the host with --authorize, and the
# host's SSH key is pinned by its fingerprint with --host-key.
#
# Usage: hosts-e2e.sh ENGINE_DIR HOST_USER CONTROLLER_USER
# ENGINE_DIR is an unpacked engine archive both accounts may run (root-owned, not writable by
# others). Both accounts linger, so their systemd user managers run; the machine's SSH server
# listens on SILO_E2E_SSH_PORT (22). SILO_E2E_REMOTE_EXEC=1 also runs `exec` and
# `fork -- <command>` through --device (phase 5). The computers are named e2e-hosts-* and are
# stopped and dropped, and the controller removed from the host, even when a step fails.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 ENGINE_DIR HOST_USER CONTROLLER_USER" >&2
  exit 2
fi
engine="$1"
host_user="$2"
controller_user="$3"
name="${SILO_E2E_COMPUTER:-e2e-hosts-remote}"
fork="$name-fork"
port="${SILO_E2E_SSH_PORT:-22}"
host_key_file="${SILO_E2E_HOST_KEY_FILE:-/etc/ssh/ssh_host_ed25519_key.pub}"
ready_timeout="${SILO_E2E_READY_TIMEOUT:-120}"
# Runs a command as an account: `RUN_AS USER COMMAND...`. Tests replace it.
run_as="${SILO_E2E_RUN_AS:-}"

case "$name" in
  e2e-hosts-*) ;;
  *) echo "Test computers are named e2e-hosts-*, not $name" >&2; exit 2 ;;
esac

step() { printf '\n== %s\n' "$*"; }

# As an account, with its user manager's environment, so `systemctl --user` reaches it.
as() {
  local user=$1
  shift
  if [[ -n "$run_as" ]]; then
    "$run_as" "$user" "$@"
    return
  fi
  local uid
  uid=$(id -u "$user")
  sudo -u "$user" -H env XDG_RUNTIME_DIR="/run/user/$uid" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" "$@"
}
host() { as "$host_user" "$engine/silo" "$@"; }
controller() { as "$controller_user" "$engine/silo" "$@"; }

# The names among the arguments that the host lists as its own computers.
listed_on_host() {
  host --json list --all | python3 -c '
import json, sys
names = {computer["name"] for computer in json.load(sys.stdin)["computers"]}
print("\n".join(sorted(name for name in sys.argv[1:] if name in names)))' "$@"
}

device=""
cleanup() {
  local status=$?
  trap - EXIT
  for computer in "$fork" "$name"; do
    if [[ "$(listed_on_host "$computer" 2>/dev/null || true)" == "$computer" ]]; then
      echo "Stopping and dropping $computer on the host after a failed run"
      host stop "$computer" || true
      host drop "$computer" || true
    fi
  done
  if [[ -n "$device" ]]; then
    echo "Removing the host from the controller after a failed run"
    controller devices remove "$device" || true
  fi
  exit "$status"
}

wait_for() {
  local who=$1
  local deadline=$((SECONDS + ready_timeout))
  until "$who" list >/dev/null 2>&1; do
    if ((SECONDS >= deadline)); then
      echo "The $who engine did not answer within $ready_timeout s" >&2
      "$who" list || true
      exit 1
    fi
    sleep 2
  done
}

step "Starting the controller's engine"
as "$controller_user" "$engine/silod" --service register
wait_for controller

step "Reading the controller's Silo key"
key_dir=$(mktemp -d)
chmod 755 "$key_dir"
controller --json devices key > "$key_dir/key.json"
python3 - "$key_dir/key.json" "$key_dir/controller.pub" <<'PY'
import json, sys
key = json.load(open(sys.argv[1]))
assert key['publicKey'].startswith('ssh-ed25519 '), key
assert key['fingerprint'].startswith('SHA256:'), key
open(sys.argv[2], 'w').write(key['publicKey'] + '\n')
PY
chmod 644 "$key_dir/controller.pub"

step "Preparing the host with silo host init"
host --json host init --authorize "$key_dir/controller.pub" > "$key_dir/init.json"
cat "$key_dir/init.json"
python3 - "$key_dir/init.json" "$key_dir/key.json" <<'PY'
import json, sys
init, key = json.load(open(sys.argv[1])), json.load(open(sys.argv[2]))
assert init['vm']['ok'] is True, init['vm']
assert init['service']['status'] == 'enabled', init['service']
assert init['service']['linger'] == 'yes', init['service']
assert init['connections']['enabled'] is True, init['connections']
assert init['authorized'][0]['fingerprint'] == key['fingerprint'], init['authorized']
assert [row['fingerprint'] for row in init['controllers']] == [key['fingerprint']], init['controllers']
PY
wait_for host
# Running it again changes nothing.
host --json host init --authorize "$key_dir/controller.pub" > "$key_dir/again.json"
python3 -c 'import json, sys; again = json.load(open(sys.argv[1])); assert again["authorized"][0]["added"] is False, again; assert len(again["controllers"]) == 1, again' "$key_dir/again.json"
host host status

# The check must fail when the list does: an empty answer would let the test take over a
# computer it did not make.
if ! existing=$(listed_on_host "$name" "$fork"); then
  echo "Could not list the host's computers; this test never runs without knowing them" >&2
  exit 1
fi
if [[ -n "$existing" ]]; then
  echo "A computer named $name or $fork already exists on the host; this test never touches it" >&2
  exit 1
fi
trap cleanup EXIT

step "Adding the host from the controller by address"
fingerprint=$(ssh-keygen -lf "$host_key_file" | awk '{print $2}')
controller --json devices add "$host_user@127.0.0.1:$port" --host-key "$fingerprint" < /dev/null > "$key_dir/added.json"
cat "$key_dir/added.json"
device=$(python3 -c '
import json, sys
added = json.load(open(sys.argv[1]))
assert added["hostKey"]["fingerprint"] == sys.argv[2], added
assert added["repaired"] == [], added
print(added["device"]["id"])' "$key_dir/added.json" "$fingerprint")
controller list --all-devices

step "Creating and starting $name on the host"
controller --device "$device" create "$name" --linux --cpus 1 --memory 2 --disk 2
controller --device "$device" start "$name"
[[ "$(listed_on_host "$name")" == "$name" ]] || { echo "$name is not on the host" >&2; exit 1; }

if [[ "${SILO_E2E_REMOTE_EXEC:-0}" == 1 ]]; then
  step "Running a command in $name from the controller"
  output=$(controller --device "$device" exec "$name" -- uname -a)
  echo "$output"
  [[ "$output" == Linux* ]] || { echo "uname -a in $name did not print a Linux kernel" >&2; exit 1; }

  step "Forking $name with a command from the controller"
  output=$(controller --device "$device" fork "$name" --name "$fork" -- uname -a)
  echo "$output"
  [[ "$output" == *Linux* ]] || { echo "the fork's command did not print a Linux kernel" >&2; exit 1; }
fi

step "Stopping and dropping from the controller"
for computer in "$fork" "$name"; do
  if [[ -n "$(listed_on_host "$computer")" ]]; then
    controller --device "$device" stop "$computer"
    controller --device "$device" drop "$computer"
  fi
done
left=$(listed_on_host "$name" "$fork")
if [[ -n "$left" ]]; then
  echo "Still on the host after drop: $left" >&2
  exit 1
fi

step "Removing the host from the controller"
controller devices remove "$device"
device=""
host --json host status > "$key_dir/status.json"
python3 -c 'import json, sys; status = json.load(open(sys.argv[1])); assert status["controllers"] == [], status' "$key_dir/status.json"
trap - EXIT
step "Passed"
