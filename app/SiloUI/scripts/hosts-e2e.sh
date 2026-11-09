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
# `fork -- <command>` through --device (phase 5). SILO_E2E_SHARE=1 also exports the host's
# computer to the controller over Connections, imports the file on the controller, imports it
# back onto the host with --device, and checks that neither engine's transfer area keeps a
# file (SILO_E2E_STATE_DIR is the channel's folder under the account's home, .silo-dev for
# the Dev engine). The computers are named e2e-hosts-* and are stopped and dropped, and the
# controller removed from the host, even when a step fails. SILO_E2E_CONTAINER=1 also makes a
# container computer on the host and drives it from the controller the same way: it is listed,
# run in, switched off and on again as to its desktop, exported to the controller, imported on both
# accounts (each account runs containers of its own), and used in the copy it was imported as.
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
# Made by the sharing steps: on the controller, and on the host from a file sent with --device.
imported="$name-imported"
sent="$name-sent"
# The container computer of the container steps, and the computers its export becomes.
ctr="$name-ctr"
ctr_imported="$name-ctr-imported"
ctr_sent="$name-ctr-sent"
state_dir="${SILO_E2E_STATE_DIR:-.silo-dev}"
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
  local uid home
  uid=$(id -u "$user")
  home=$(getent passwd "$user" | cut -d: -f6)
  # A clean environment: the runner's XDG_CONFIG_HOME and friends name its own home.
  sudo -u "$user" env -i HOME="$home" USER="$user" LOGNAME="$user" PATH="/usr/local/bin:/usr/bin:/bin" \
    XDG_RUNTIME_DIR="/run/user/$uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" "$@"
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

# What an account's engine keeps in its transfer area, one name per line.
transfer_files() {
  as "$1" bash -c 'ls -A "$HOME/$1/engine/transfer" 2>/dev/null || true' _ "$state_dir"
}

# The names among the arguments that the controller lists as its own computers.
listed_on_controller() {
  controller --json list --all | python3 -c '
import json, sys
names = {computer["name"] for computer in json.load(sys.stdin)["computers"]}
print("\n".join(sorted(name for name in sys.argv[1:] if name in names)))' "$@"
}

device=""
share_dir=""
cleanup() {
  local status=$?
  trap - EXIT
  for computer in "$imported" "$ctr_imported"; do
    if [[ "$(listed_on_controller "$computer" 2>/dev/null || true)" == "$computer" ]]; then
      echo "Dropping $computer on the controller after a failed run"
      controller drop "$computer" || true
    fi
  done
  if [[ -n "$share_dir" ]]; then as "$controller_user" rm -rf "$share_dir" || true; fi
  for computer in "$ctr_sent" "$ctr" "$sent" "$fork" "$name"; do
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

# What the account's engine printed, for a run whose logs cannot be downloaded.
engine_log() {
  [[ -n "$run_as" ]] && return 0
  local user
  if [[ $1 == host ]]; then user=$host_user; else user=$controller_user; fi
  as "$user" systemctl --user status --no-pager --lines=0 >&2 || true
  sudo journalctl --no-pager -n 80 _UID="$(id -u "$user")" >&2 || true
}

wait_for() {
  local who=$1
  local deadline=$((SECONDS + ready_timeout))
  until "$who" list >/dev/null 2>&1; do
    if ((SECONDS >= deadline)); then
      echo "The $who engine did not answer within $ready_timeout s" >&2
      "$who" list || true
      engine_log "$who"
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
if ! existing=$(listed_on_host "$name" "$fork" "$sent" "$ctr" "$ctr_sent"); then
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
# The guest image and the computer's disk need room; log it so a full disk is plain.
df -h
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

if [[ "${SILO_E2E_SHARE:-0}" == 1 ]]; then
  step "Exporting $name from the host to the controller"
  [[ -z "$(transfer_files "$host_user")" ]] || { echo "The host's transfer area is not empty before the test" >&2; exit 1; }
  if [[ -n "$(listed_on_controller "$imported")" ]]; then
    echo "A computer named $imported already exists on the controller; this test never touches it" >&2
    exit 1
  fi
  share_dir=$(as "$controller_user" mktemp -d)
  exported="$share_dir/$name.silo-backup"
  # The computer is running, so the login check can list its disk; --yes accepts what it finds.
  controller --device "$device" export "$name" --yes -o "$exported"
  as "$controller_user" test -s "$exported" || { echo "The exported file is missing or empty" >&2; exit 1; }
  size=$(as "$controller_user" bash -c 'wc -c < "$1"' _ "$exported")
  echo "Exported file: $size bytes"
  [[ -z "$(transfer_files "$host_user")" ]] || { echo "The host kept files after the export: $(transfer_files "$host_user")" >&2; exit 1; }
  [[ -z "$(transfer_files "$controller_user")" ]] || { echo "The controller's transfer area holds files after the export" >&2; exit 1; }

  step "Importing the file on the controller"
  controller import "$exported" --name "$imported"
  [[ "$(listed_on_controller "$imported")" == "$imported" ]] || { echo "$imported is not on the controller" >&2; exit 1; }

  step "Importing the file onto the host from the controller with --device"
  controller --device "$device" import "$exported" --name "$sent"
  [[ "$(listed_on_host "$sent")" == "$sent" ]] || { echo "$sent is not on the host" >&2; exit 1; }
  [[ -z "$(transfer_files "$host_user")" ]] || { echo "The host kept files after the import: $(transfer_files "$host_user")" >&2; exit 1; }
  [[ -z "$(transfer_files "$controller_user")" ]] || { echo "The controller's transfer area holds files after the import" >&2; exit 1; }

  step "Dropping the imported computers"
  controller drop "$imported"
  controller --device "$device" drop "$sent"
  as "$controller_user" rm -rf "$share_dir"
  share_dir=""
fi

if [[ "${SILO_E2E_CONTAINER:-0}" == 1 ]]; then
  step "Making a container computer on the host from the controller"
  controller --device "$device" create "$ctr" --container --cpus 1 --memory 2 --disk 2
  controller --device "$device" start "$ctr"
  kind=$(host --json list --all | python3 -c '
import json, sys
rows = {c["name"]: c["kind"] for c in json.load(sys.stdin)["computers"]}
print(rows.get(sys.argv[1], "missing"))' "$ctr")
  [[ "$kind" == container ]] || { echo "$ctr is a $kind on the host, not a container" >&2; exit 1; }
  controller list --all-devices | grep -q -- "$ctr" || { echo "$ctr is not in the controller's list of every device" >&2; exit 1; }

  step "Running commands in $ctr from the controller"
  output=$(controller --device "$device" exec "$ctr" -- sh -c 'echo over-connections > /workspace/e2e-note && cat /workspace/e2e-note && uname -s')
  echo "$output"
  [[ "$output" == $'over-connections\nLinux' ]] || { echo "unexpected output from $ctr: $output" >&2; exit 1; }
  controller --device "$device" exec "$ctr" -- sh -c 'mkdir -p ~/.claude && echo "{}" > ~/.claude/.credentials.json'

  step "Turning the desktop of $ctr off and on from the controller"
  controller --device "$device" desktop off "$ctr"
  controller --device "$device" desktop on "$ctr"

  step "Exporting $ctr from the host to the controller"
  [[ -z "$(transfer_files "$host_user")" ]] || { echo "The host's transfer area is not empty before the container export" >&2; exit 1; }
  share_dir=$(as "$controller_user" mktemp -d)
  exported="$share_dir/$ctr.silo-backup"
  # The login on the computer is named, and --yes shares it anyway; nothing is left out.
  if output=$(controller --device "$device" export "$ctr" -o "$exported" 2>&1 < /dev/null); then
    echo "an export of a computer with a login finished without --yes: $output" >&2
    exit 1
  fi
  grep -q 'Claude Code login' <<<"$output" || { echo "the export did not name the login: $output" >&2; exit 1; }
  controller --device "$device" export "$ctr" --yes -o "$exported"
  as "$controller_user" test -s "$exported" || { echo "The exported container file is missing or empty" >&2; exit 1; }
  [[ -z "$(transfer_files "$host_user")" ]] || { echo "The host kept files after the container export: $(transfer_files "$host_user")" >&2; exit 1; }
  [[ -z "$(transfer_files "$controller_user")" ]] || { echo "The controller's transfer area holds files after the container export" >&2; exit 1; }

  step "Importing the container file on the controller and onto the host"
  controller import "$exported" --name "$ctr_imported"
  controller --device "$device" import "$exported" --name "$ctr_sent"
  [[ "$(listed_on_controller "$ctr_imported")" == "$ctr_imported" ]] || { echo "$ctr_imported is not on the controller" >&2; exit 1; }
  [[ "$(listed_on_host "$ctr_sent")" == "$ctr_sent" ]] || { echo "$ctr_sent is not on the host" >&2; exit 1; }
  [[ -z "$(transfer_files "$host_user")" ]] || { echo "The host kept files after the container import" >&2; exit 1; }
  [[ -z "$(transfer_files "$controller_user")" ]] || { echo "The controller's transfer area holds files after the container import" >&2; exit 1; }

  step "Using the imported copies"
  controller start "$ctr_imported"
  controller --device "$device" start "$ctr_sent"
  for where in "controller" "controller --device $device"; do
    case "$where" in
      controller) copy="$ctr_imported" ;;
      *) copy="$ctr_sent" ;;
    esac
    output=$($where exec "$copy" -- sh -c 'cat /workspace/e2e-note && test -f ~/.claude/.credentials.json && echo login')
    [[ "$output" == $'over-connections\nlogin' ]] || { echo "unexpected output from $copy: $output" >&2; exit 1; }
  done
  kind=$(controller --json list --all | python3 -c '
import json, sys
rows = {c["name"]: c["kind"] for c in json.load(sys.stdin)["computers"]}
print(rows.get(sys.argv[1], "missing"))' "$ctr_imported")
  [[ "$kind" == container ]] || { echo "$ctr_imported is a $kind on the controller, not a container" >&2; exit 1; }

  step "Dropping the container computers"
  controller stop "$ctr_imported"
  controller --device "$device" stop "$ctr_sent"
  controller drop "$ctr_imported"
  controller --device "$device" drop "$ctr_sent"
  controller --device "$device" stop "$ctr"
  controller --device "$device" drop "$ctr"
  as "$controller_user" rm -rf "$share_dir"
  share_dir=""
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
