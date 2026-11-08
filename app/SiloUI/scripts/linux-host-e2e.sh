#!/usr/bin/env bash
# Drives a real Linux computer on a running Linux host engine through its `silo` command:
# create, start, exec, checkpoint, fork, exec in the fork, then stop and drop both. The
# computers are named e2e-hosts-* and stopped and dropped even when a step fails.
#
# With SILO_E2E_CONTAINER=1 it then drives a container computer the same way (create --container,
# start, exec, a file written and read back, listed as kind container, hardening spot checks,
# stop, drop). The host must have crun, skopeo, umoci and the other container prerequisites
# (`silo host init` names them).
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
ctr="$name-ctr"
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
  for computer in "$ctr" "$fork" "$name"; do
    if listed "$computer" 2>/dev/null | grep -qx -- "$computer"; then
      echo "Stopping and dropping $computer after a failed run"
      "$silo" stop "$computer" || true
      "$silo" drop "$computer" || true
    fi
  done
  exit "$status"
}

# Runs `silo exec` in the container computer and prints what the command printed.
in_container() { "$silo" exec "$ctr" -- "$@"; }

expect() {
  local what="$1" got="$2" wanted="$3"
  if [[ "$got" != "$wanted" ]]; then
    echo "$what: got '$got', wanted '$wanted'" >&2
    exit 1
  fi
  echo "$what: $got"
}

# What to read when the container cannot reach a public host: its resolver and routes, a direct
# query to the gateway, and the gateway's and holder's own logs on the host.
container_diagnostics() {
  echo "== Diagnostics for $ctr" >&2
  in_container sh -c 'cat /etc/resolv.conf; cat /proc/net/route; cat /proc/net/dev; getent hosts example.com || echo "getent: no answer"' >&2 || true
  in_container python3 -c '
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.settimeout(5)
query = b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x03com\x00\x00\x01\x00\x01"
try:
    s.sendto(query, ("172.30.0.5", 53))
    print("dns answer bytes:", len(s.recvfrom(512)[0]))
except OSError as error:
    print("dns query failed:", error)' >&2 || true
  find "$HOME/.silo-dev" -path '*containers/computers/*/state/*.log' 2>/dev/null | while read -r log; do
    echo "-- $log" >&2
    tail -n 40 "$log" >&2 || true
  done
}

container_checks() {
  step "Creating container computer $ctr"
  "$silo" create "$ctr" --container --cpus 1 --memory 2 --disk 2

  step "Starting $ctr"
  "$silo" start "$ctr"

  step "Running a command in $ctr"
  output=$(in_container uname -a)
  echo "$output"
  [[ "$output" == Linux* ]] || { echo "uname -a in $ctr did not print a Linux kernel" >&2; exit 1; }

  step "Writing and reading a file in $ctr"
  expect "file" "$(in_container sh -c 'echo container-ok > /workspace/e2e-note && cat /workspace/e2e-note')" container-ok

  step "Listing $ctr"
  kind=$("$silo" --json list --all | python3 -c '
import json, sys
rows = {c["name"]: c["kind"] for c in json.load(sys.stdin)["computers"]}
print(rows.get(sys.argv[1], "missing"))' "$ctr")
  expect "kind" "$kind" container

  step "Checking the hardening baseline in $ctr"
  expect "capabilities" "$(in_container sh -c "grep '^CapEff' /proc/self/status | tr -d '[:space:]'")" CapEff:0000000000000000
  expect "no_new_privs" "$(in_container sh -c "grep '^NoNewPrivs' /proc/self/status | tr -d '[:space:]'")" NoNewPrivs:1
  expect "seccomp" "$(in_container sh -c "grep '^Seccomp:' /proc/self/status | tr -d '[:space:]'")" Seccomp:2
  # io_uring_setup is call 425 on x86_64 and aarch64; the deny-list answers EPERM (1).
  expect "io_uring" "$(in_container python3 -c 'import ctypes; c = ctypes.CDLL(None, use_errno=True); print(c.syscall(425, 8, 0), ctypes.get_errno())')" "-1 1"
  expect "nested user namespace" "$(in_container sh -c 'unshare -U true >/dev/null 2>&1; echo $?' | sed 's/^[1-9][0-9]*$/refused/')" refused
  expect "interfaces" "$(in_container sh -c "tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d ' ' | sort | tr '\\n' ' '")" "lo tap0 "

  step "Checking the network of $ctr"
  # A public host answers, so the refusals below mean something.
  code=000
  for _ in 1 2 3; do
    code=$(in_container curl -sS -o /dev/null --max-time 20 -w '%{http_code}' https://example.com || true)
    [[ "$code" == 200 ]] && break
    sleep 3
  done
  if [[ "$code" != 200 ]]; then container_diagnostics; fi
  expect "public host" "$code" 200
  for target in 169.254.169.254 10.0.0.1 172.30.0.5 127.0.0.2; do
    result=$(in_container python3 -c '
import socket, sys
try:
    socket.create_connection((sys.argv[1], 80), timeout=8).close()
    print("connected")
except OSError:
    print("refused")' "$target")
    expect "private address $target" "$result" refused
  done

  step "Stopping and dropping $ctr"
  "$silo" stop "$ctr"
  "$silo" drop "$ctr"
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
if ! existing=$(listed "$name" "$fork" "$ctr"); then
  echo "Could not list the engine's computers; this test never runs without knowing them" >&2
  exit 1
fi
if [[ -n "$existing" ]]; then
  echo "A computer named $name, $fork or $ctr already exists; this test never touches it" >&2
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
if [[ "${SILO_E2E_CONTAINER:-0}" == 1 ]]; then
  container_checks
  left=$(listed "$ctr")
  if [[ -n "$left" ]]; then
    echo "Still listed after drop: $left" >&2
    exit 1
  fi
fi
trap - EXIT
step "Passed"
