#!/usr/bin/env bash
# Drives a real Linux computer on a running Linux host engine through its `silo` command:
# create, start, exec, checkpoint, fork, exec in the fork, then stop and drop both. The
# computers are named e2e-hosts-* and stopped and dropped even when a step fails.
#
# Usage: linux-host-e2e.sh ENGINE_DIR
# ENGINE_DIR is an unpacked engine archive (silo-engine/), whose engine is already serving.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 ENGINE_DIR" >&2
  exit 2
fi
silo="$1/silo"
name="${SILO_E2E_COMPUTER:-e2e-hosts-ci}"
fork="$name-fork"
# Seconds to wait for the engine socket to answer.
ready_timeout="${SILO_E2E_READY_TIMEOUT:-120}"
# Seconds between attempts to create the computer when the VM image download fails.
retry_wait="${SILO_E2E_RETRY_WAIT:-30}"

case "$name" in
  e2e-hosts-*) ;;
  *) echo "Test computers are named e2e-hosts-*, not $name" >&2; exit 2 ;;
esac

step() { printf '\n== %s\n' "$*"; }

listed() {
  "$silo" --json list --all | python3 -c '
import json, sys
names = {computer["name"] for computer in json.load(sys.stdin)["computers"]}
print("\n".join(sorted(name for name in sys.argv[1:] if name in names)))' "$@"
}

cleanup() {
  local status=$?
  trap - EXIT
  rm -f "${stderr_file:-}"
  for computer in "$fork" "$name"; do
    if listed "$computer" 2>/dev/null | grep -qx -- "$computer"; then
      echo "Stopping and dropping $computer after a failed run"
      "$silo" stop "$computer" || true
      "$silo" drop "$computer" || true
    fi
  done
  exit "$status"
}

step "Waiting for the engine"
deadline=$((SECONDS + ready_timeout))
until "$silo" list >/dev/null 2>&1; do
  if ((SECONDS >= deadline)); then
    echo "The engine did not answer within $ready_timeout s" >&2
    "$silo" list || true
    exit 1
  fi
  sleep 2
done
# The check must fail when the list does: an empty answer would let the test take over a
# computer it did not make.
if ! existing=$(listed "$name" "$fork"); then
  echo "Could not list the engine's computers; this test never runs without knowing them" >&2
  exit 1
fi
if [[ -n "$existing" ]]; then
  echo "A computer named $name or $fork already exists; this test never touches it" >&2
  exit 1
fi
stderr_file="$(mktemp)"
trap cleanup EXIT

step "Creating $name"
# The VM image comes from a GitHub release, whose download endpoint sometimes answers 504 for a
# while: a download failure is retried, anything else fails at once.
for attempt in 1 2 3; do
  if "$silo" create "$name" --linux --cpus 1 --memory 2 --disk 2 2> "$stderr_file"; then
    break
  fi
  cat "$stderr_file" >&2
  if ((attempt == 3)) || ! grep -q "could not download the VM image" "$stderr_file"; then
    exit 1
  fi
  echo "The VM image download failed (attempt $attempt of 3); trying again in ${retry_wait} s" >&2
  if listed "$name" | grep -qx -- "$name"; then "$silo" drop "$name" || true; fi
  sleep "$retry_wait"
done

step "Starting $name"
"$silo" start "$name"

step "Running a command in $name"
output=$("$silo" exec "$name" -- uname -a)
echo "$output"
[[ "$output" == Linux* ]] || { echo "uname -a in $name did not print a Linux kernel" >&2; exit 1; }

step "Saving a checkpoint of $name"
"$silo" checkpoint "$name" --name e2e-hosts-ci

step "Forking $name as $fork"
"$silo" fork "$name" --name "$fork"
state=$("$silo" --json status "$fork" | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')
if [[ "$state" != running ]]; then
  "$silo" start "$fork"
fi

step "Running a command in $fork"
output=$("$silo" exec "$fork" -- uname -a)
echo "$output"
[[ "$output" == Linux* ]] || { echo "uname -a in $fork did not print a Linux kernel" >&2; exit 1; }

# Stopped first, so the run checks Stop too and Drop never has to end a running guest.
step "Stopping $fork and $name"
"$silo" stop "$fork"
"$silo" stop "$name"

step "Dropping $fork and $name"
"$silo" drop "$fork"
"$silo" drop "$name"
left=$(listed "$name" "$fork")
if [[ -n "$left" ]]; then
  echo "Still listed after drop: $left" >&2
  exit 1
fi
trap - EXIT
step "Passed"
