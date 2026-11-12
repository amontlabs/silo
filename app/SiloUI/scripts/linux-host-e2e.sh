#!/usr/bin/env bash
# Drives a real Linux computer on a running Linux host engine through its `silo` command:
# create, start, exec, checkpoint, fork, exec in the fork, then stop and drop both. Between the
# checkpoint and the fork it checks that no export holds memory: the checkpoint, saved while the
# computer ran, is refused; the running computer's export and the export of a fork of that
# checkpoint are opened and hold disks only. The
# computers are named e2e-hosts-* and stopped and dropped even when a step fails.
#
# With SILO_E2E_CONTAINER=1 it then drives a container computer the same way (create --container,
# start, exec, a file written and read back, listed as kind container, hardening spot checks,
# stop, drop), then a second container with the desktop on: the desktop service, the viewer port
# (over SSH as the app's viewer reaches it, and through the URL proxy), Cua Driver listing windows
# and saving a screenshot, Chrome with --no-sandbox, `fork --gui`, `silo desktop off`; then a
# container created with `--no-desktop`; and a container exported to a file and imported under a
# new name. The host must have crun, skopeo, umoci and the other
# container prerequisites (`silo host init` names them).
#
# Usage: linux-host-e2e.sh ENGINE_DIR
# ENGINE_DIR is an unpacked engine archive (silo-engine/), whose engine is already serving.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 ENGINE_DIR" >&2
  exit 2
fi
engine_dir="$1"
silo="$1/silo"
name="${SILO_E2E_COMPUTER:-e2e-hosts-ci}"
fork="$name-fork"
# The fork of a checkpoint that was saved while the computer ran.
ckpt_fork="$name-ckpt"
ctr="$name-ctr"
# A second container that keeps the secret when it is taken away from the first (a secret always
# has a computer).
ctr2="$name-ctr2"
# A container whose stated size is small, so it is over it.
ctr3="$name-ctr3"
# The container that gets the desktop checks, and the forks made from it.
ctr4="$name-ctr4"
# A container created with its desktop off.
ctr5="$name-ctr5"
# A container that is exported, and the computer its export is imported as.
ctr6="$name-ctr6"
ctr7="$name-ctr7"
# Where the desktop checks leave their evidence (a screenshot of the desktop and of Chrome); the
# workflow uploads it. A run outside CI keeps it in a temporary folder.
artifacts="${SILO_E2E_ARTIFACTS:-}"
# The file that fills the host's disk in the guard check; the cleanup removes it.
fill_file=""
# Where the VM exports are written; the cleanup removes it.
export_dir=""
# The engine's systemd unit, which the restart check ends the way a crash does.
engine_unit="${SILO_E2E_UNIT:-org.silo.dev.engine.service}"
# Seconds to wait for the engine socket to answer.
ready_timeout="${SILO_E2E_READY_TIMEOUT:-120}"
# Seconds between attempts to create the computer when the VM image download fails.
retry_wait="${SILO_E2E_RETRY_WAIT:-30}"

case "$name" in
  e2e-hosts-*) ;;
  *) echo "Test computers are named e2e-hosts-*, not $name" >&2; exit 2 ;;
esac

# The engine's files: the socket folder and the data folder the runtime home is under.
engine_roots() { printf '%s\n' "$HOME/.silo-dev" "$HOME/.local/share/org.silo.dev"; }
engine_find() { local root; while read -r root; do [[ -d "$root" ]] && find "$root" "$@" 2>/dev/null; done < <(engine_roots); true; }

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
  [[ -n "$fill_file" ]] && rm -f "$fill_file"
  rm -f "${stderr_file:-}"
  remove_editor_key
  [[ -n "$export_dir" ]] && rm -rf "$export_dir"
  for computer in "$ctr7" "$ctr6" "$ctr4-gui" "$ctr4-plain" "$ctr4" "$ctr5" "$ctr3" "$ctr2" "$ctr" "$ctr-k" "$ctr-f-1" "$ctr-f-2" "$ctr-f-3" "$ctr-f-4" "$ckpt_fork" "$fork" "$name"; do
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
# The same with a timeout in seconds.
in_container_for() { local seconds="$1"; shift; "$silo" exec "$ctr" --timeout "$seconds" -- "$@"; }

# Runs a command as root in the container, the way Silo runs its own root commands: through the
# engine's helper and `crun exec`, with the seven capabilities and the init shim in front. `silo
# exec` runs as the working account, which cannot become root (no_new_privs), so the commands that
# need root, such as a package install, use this. It sets SILO_RUNTIME as the engine does.
root_in_container() {
  local state process status=0
  state=$(engine_find -path "*containers/computers/$ctr/state" -type d | head -n 1)
  [[ -n "$state" ]] || { echo "no state folder found for $ctr" >&2; return 1; }
  process=$(mktemp)
  python3 - "$process" /silo/init --own-group -- "$@" <<'PY'
import json, sys
caps = ["CAP_CHOWN", "CAP_DAC_OVERRIDE", "CAP_FOWNER", "CAP_FSETID", "CAP_KILL", "CAP_SETGID", "CAP_SETUID"]
json.dump({
    "terminal": False,
    "user": {"uid": 0, "gid": 0},
    "args": sys.argv[2:],
    "env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root", "SILO_RUNTIME=container", "DEBIAN_FRONTEND=noninteractive"],
    "cwd": "/",
    "noNewPrivileges": True,
    "capabilities": {k: caps for k in ("effective", "bounding", "inheritable", "ambient", "permitted")},
}, open(sys.argv[1], "w"))
PY
  # In a scope of the user manager, as the engine is: joining the container's cgroup needs a common
  # ancestor the account may write, which a shell in a login session does not share.
  systemd-run --user --scope --quiet -- "$engine_dir/silo-ctr" enter --state "$state" -- crun --root "$state/crun" --cgroup-manager=cgroupfs exec --process "$process" "$ctr" || status=$?
  rm -f "$process"
  return "$status"
}

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
  in_container sh -c 'id; ls -ln / /etc/resolv.conf /etc/hosts; grep -E "resolv|hosts" /proc/self/mountinfo; cat /etc/resolv.conf; cat /proc/net/route; cat /proc/net/dev; getent hosts example.com || echo "getent: no answer"' >&2 || true
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
  engine_find -path '*containers/computers/*/state/resolv.conf' -exec stat -c '%a %U %n' {} \; >&2 || true
  sudo dmesg 2>/dev/null | grep -iE 'apparmor|landlock|denied' | tail -n 15 >&2 || true
  engine_find -path '*containers/computers/*/state/*.log' | while read -r log; do
    echo "-- $log" >&2
    tail -n 40 "$log" >&2 || true
  done
}

# The id of the checkpoint of $1 called $2.
checkpoint_id() {
  "$silo" --json checkpoints "$1" | python3 -c '
import json, sys
found = [c["id"] for c in json.load(sys.stdin)["checkpoints"] if c["name"] == sys.argv[1]]
print(found[0] if found else "")' "$2"
}

# The scope the checkpoint $2 of $1 was saved with.
checkpoint_scope() {
  "$silo" --json checkpoints "$1" | python3 -c '
import json, sys
found = [c.get("scope", "?") for c in json.load(sys.stdin)["checkpoints"] if c["name"] == sys.argv[1]]
print(found[0] if found else "missing")' "$2"
}

# Fails unless the export file $1 holds disks only. The file is a header, a manifest and one
# MicroSandbox snapshot archive (a zstd tar) per computer. MicroSandbox keeps a capture's memory,
# execution and device state under `checkpoints/` in that archive, and names the capture's scope
# in `snapshots/<id>/snapshot.json`: neither may say anything but a disk capture.
export_holds_no_memory() {
  python3 - "$1" <<'PY'
import json, subprocess, sys, tarfile, threading

with open(sys.argv[1], "rb") as file:
    if not file.read(16).startswith(b"SILO-BACKUP"):
        sys.exit("not a Silo export file")
    file.read(4)
    manifest = json.loads(file.read(int.from_bytes(file.read(8), "big")))
    if not manifest["computers"]:
        sys.exit("the export holds no computer")
    for computer in manifest["computers"]:
        name = computer["name"]
        zstd = subprocess.Popen(["zstd", "-dc"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)

        def feed(left=computer["payloadSize"]):
            while left:
                chunk = file.read(min(left, 1 << 20))
                if not chunk:
                    break
                zstd.stdin.write(chunk)
                left -= len(chunk)
            zstd.stdin.close()

        feeder = threading.Thread(target=feed, daemon=True)
        feeder.start()
        entries = snapshots = 0
        with tarfile.open(fileobj=zstd.stdout, mode="r|") as archive:
            for member in archive:
                path = member.name.removeprefix("./")
                entries += 1
                if path.split("/")[0] == "checkpoints":
                    sys.exit(f"{name}: the export holds {path}")
                if path.startswith("snapshots/") and path.endswith("/snapshot.json"):
                    descriptor = json.load(archive.extractfile(member))
                    scope = descriptor.get("scope")
                    kind = descriptor.get("state", {}).get("kind")
                    if scope != "file" or kind != "file":
                        sys.exit(f"{name}: {path} is a {scope}/{kind} capture, not a disk capture")
                    snapshots += 1
        feeder.join()
        if zstd.wait() != 0:
            sys.exit(f"{name}: the snapshot archive could not be read")
        if snapshots == 0:
            sys.exit(f"{name}: the export names no snapshot")
        print(f"{name}: {entries} entries, {snapshots} disk capture(s), nothing under checkpoints/")
PY
}

# No export of a VM computer holds memory. The computer $name is running and has the checkpoint
# e2e-hosts-ci, saved while it ran.
vm_exports_hold_no_memory() {
  local id output
  command -v zstd > /dev/null || { echo "zstd is needed to open an export file" >&2; exit 1; }
  export_dir=$(mktemp -d)

  step "Exporting the checkpoint of $name that was saved while it ran is refused"
  expect "checkpoint scope" "$(checkpoint_scope "$name" e2e-hosts-ci)" full
  id=$(checkpoint_id "$name" e2e-hosts-ci)
  if output=$("$silo" export "$name" --checkpoint "$id" -o "$export_dir/checkpoint.silo-backup" --yes < /dev/null 2>&1); then
    echo "a checkpoint that holds memory was exported: $output" >&2
    exit 1
  fi
  echo "$output"
  grep -q "holds the computer's memory" <<<"$output" || { echo "the refusal did not give its reason" >&2; exit 1; }
  grep -q "Fork the checkpoint and export the fork" <<<"$output" || { echo "the refusal did not say what to do instead" >&2; exit 1; }
  [[ -z "$(ls -A "$export_dir")" ]] || { echo "a refused export left a file: $(ls -A "$export_dir")" >&2; exit 1; }

  step "Exporting $name while it runs"
  "$silo" export "$name" -o "$export_dir/running.silo-backup" --yes < /dev/null
  echo "export file: $(wc -c < "$export_dir/running.silo-backup") bytes"
  export_holds_no_memory "$export_dir/running.silo-backup"

  step "Forking that checkpoint as $ckpt_fork and exporting the fork"
  "$silo" fork "$name" --name "$ckpt_fork" --checkpoint "$id"
  if [[ "$("$silo" --json status "$ckpt_fork" | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')" != running ]]; then
    "$silo" start "$ckpt_fork"
  fi
  "$silo" export "$ckpt_fork" -o "$export_dir/fork.silo-backup" --yes < /dev/null
  export_holds_no_memory "$export_dir/fork.silo-backup"
  "$silo" stop "$ckpt_fork"
  "$silo" drop "$ckpt_fork"
  rm -rf "$export_dir"
  export_dir=""
}

# Whether $2 exists in the computer $1 ("yes" or "no").
has_file() { "$silo" exec "$1" -- sh -c "test -e '$2' && echo yes || echo no"; }

# Checkpoints, forks, diff, keep, undo and a quiet fork of a running container, then an engine
# restart (a crash: the unit is killed, systemd starts it again) and, after them, the host disk guard.
container_checkpoint_features() {
  step "Checkpointing the running $ctr"
  in_container sh -c 'echo base > /workspace/ct4-base'
  root_in_container sh -c 'echo "* * * * * root touch /tmp/ct4-cron-ran" > /etc/cron.d/ct4-cron'
  "$silo" checkpoint "$ctr" --name ct4-base
  base=$(checkpoint_id "$ctr" ct4-base)
  [[ -n "$base" ]] || { echo "no checkpoint ct4-base listed" >&2; exit 1; }
  expect "checkpoint scope" "$(checkpoint_scope "$ctr" ct4-base)" disk
  in_container sh -c 'echo later > /workspace/ct4-after'

  step "Forking $ctr four times from the checkpoint"
  started=$(date +%s.%N)
  "$silo" fork "$ctr" --name "$ctr-f" -n 4 --checkpoint "$base"
  finished=$(date +%s.%N)
  python3 -c 'import sys; print("fork -n 4 from a checkpoint of a running container: %.1f s" % (float(sys.argv[2]) - float(sys.argv[1])))' "$started" "$finished"
  for i in 1 2 3 4; do
    fork_i="$ctr-f-$i"
    expect "$fork_i sees the checkpoint" "$("$silo" exec "$fork_i" -- cat /workspace/ct4-base)" base
    expect "$fork_i lacks later writes" "$(has_file "$fork_i" /workspace/ct4-after)" no
    expect "$fork_i has no cron job of the parent" "$(has_file "$fork_i" /etc/cron.d/ct4-cron)" no
    "$silo" exec "$fork_i" -- sh -c "echo fork-$i > /workspace/only-$i; echo fork-$i > /home/silo/only-$i"
  done
  expect "the parent has its cron job" "$(has_file "$ctr" /etc/cron.d/ct4-cron)" yes
  for i in 1 2 3 4; do
    for j in 1 2 3 4; do
      [[ "$i" == "$j" ]] && continue
      expect "fork $i does not see fork $j" "$(has_file "$ctr-f-$i" "/workspace/only-$j")" no
    done
  done
  expect "the parent does not see a fork's file" "$(has_file "$ctr" /workspace/only-1)" no

  step "Diff in a fork"
  diff_text=$("$silo" diff "$ctr-f-1")
  echo "$diff_text"
  grep -q "/home/silo/only-1" <<<"$diff_text" || { echo "the diff of $ctr-f-1 does not list its change" >&2; exit 1; }
  for i in 1 2 3 4; do
    "$silo" stop "$ctr-f-$i"
    "$silo" drop "$ctr-f-$i"
  done

  step "Keeping a fork into $ctr and undoing it"
  "$silo" fork "$ctr" --name "$ctr-k"
  "$silo" start "$ctr-k"
  "$silo" exec "$ctr-k" -- sh -c 'echo kept > /home/silo/ct4-kept'
  "$silo" fold "$ctr-k"
  expect "kept file in the parent" "$(has_file "$ctr" /home/silo/ct4-kept)" yes
  "$silo" undo "$ctr"
  state=$("$silo" --json status "$ctr" | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')
  [[ "$state" == running ]] || "$silo" start "$ctr"
  expect "kept file after undo" "$(has_file "$ctr" /home/silo/ct4-kept)" no
  expect "earlier state after undo" "$(has_file "$ctr" /workspace/ct4-after)" yes
  "$silo" stop "$ctr-k"
  "$silo" drop "$ctr-k"

  step "Restarting the engine under the running $ctr"
  systemctl --user kill --kill-whom=main --signal=SIGKILL "$engine_unit"
  came_back=no
  for _ in $(seq 1 60); do
    sleep 3
    if [[ "$("$silo" --json status "$ctr" 2>/dev/null | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])' 2>/dev/null)" == running ]]; then
      came_back=yes
      break
    fi
  done
  expect "container after an engine restart" "$came_back" yes
  expect "file after an engine restart" "$(in_container cat /workspace/ct4-base)" base
  code=000
  for _ in 1 2 3 4 5; do
    code=$(in_container curl -sS -o /dev/null --max-time 20 -w '%{http_code}' https://example.com || true)
    [[ "$code" == 200 ]] && break
    sleep 3
  done
  expect "public host after an engine restart" "$code" 200
}

# The host's disk guard: a container computer has no size bound, but a host short of disk space
# refuses to start one and stops the running ones that are over their stated size. The host is
# filled past the margin with a fallocate'd file on the engine's file system.
container_disk_guard() {
  step "Filling the host past the disk margin with $ctr3 over its size"
  "$silo" create "$ctr3" --container --cpus 1 --memory 1 --disk 1
  # A container's size is its root disk and workspace together (81 GiB here); the record of this
  # stopped test computer is made small so the runner's disk is enough to pass it.
  record=$(engine_find -path "*containers/computers/$ctr3/computer.json" | head -n 1)
  python3 - "$record" <<'PY'
import json, sys
record = json.load(open(sys.argv[1]))
record["rootDiskMib"], record["workspaceMib"] = 300, 100
json.dump(record, open(sys.argv[1], "w"))
PY
  "$silo" start "$ctr3"
  # 600 MiB, over the 400 MiB it is stated to have. Nothing stops it while the host has room.
  "$silo" exec "$ctr3" -- dd if=/dev/zero of=/home/silo/fill bs=1M count=600 conv=fsync status=none
  local home margin_mib=${SILO_CONTAINER_DISK_MARGIN_MIB:-5120} free
  home=$(dirname "$(engine_find -path "*containers/computers/$ctr3/computer.json" | head -n 1)")
  free=$(df --output=avail -B1 "$home" | tail -n 1 | tr -d ' ')
  # Leave half the margin free.
  fill_file="$(dirname "$home")/silo-e2e-fill"
  fallocate -l "$((free - margin_mib * 1024 * 1024 / 2))" "$fill_file"
  # The guard looks every 30 seconds (every minute while low).
  stopped=no
  for i in $(seq 1 30); do
    state=$("$silo" --json status "$ctr3" | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')
    if [[ "$state" != running ]]; then stopped=yes; break; fi
    sleep 10
  done
  expect "the oversized computer is stopped while the host is low on disk" "$stopped" yes
  refused=$("$silo" start "$ctr3" 2>&1 || true)
  echo "start while low: $refused"
  grep -q "nearly full" <<<"$refused" || { echo "the start was not refused for the disk: $refused" >&2; exit 1; }
  forked=$("$silo" checkpoint "$ctr" --name ct4-low 2>&1 || true)
  grep -q "nearly full" <<<"$forked" || { echo "the checkpoint was not refused for the disk: $forked" >&2; exit 1; }
  expect "the container that stayed within its size keeps running" "$("$silo" --json status "$ctr" | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')" running
  rm -f "$fill_file"
  "$silo" start "$ctr3"
  expect "the computer starts again once the file is removed" "$("$silo" --json status "$ctr3" | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')" running
  "$silo" stop "$ctr3"
  "$silo" drop "$ctr3"
}

container_checks() {
  step "Creating container computer $ctr"
  if ! "$silo" create "$ctr" --container --cpus 1 --memory 2 --disk 2; then
    echo "== The engine's log after the failed create" >&2
    journalctl --user --unit "$engine_unit" --no-pager -n 80 >&2 || true
    exit 1
  fi

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

  step "Ending a timed-out command in $ctr together with what it started"
  # The command starts a background process and outlives its timeout; neither may remain.
  if in_container_for 3 sh -c 'sleep 300 & echo $! > /tmp/e2e-leftover; wait' 2>/dev/null; then
    echo "a command that outlived its timeout reported success" >&2
    exit 1
  fi
  sleep 2
  expect "timed-out command leftovers" "$(in_container sh -c 'kill -0 "$(cat /tmp/e2e-leftover)" 2>/dev/null && echo alive || echo gone')" gone

  step "Checking Silo's own root commands in $ctr"
  # `silo start` ran the account setup as root through a waited exec (a timeout, the seven
  # capabilities, ambient and inheritable): the account it creates and the record it writes prove
  # the root exec worked, and a command as that account sees them.
  expect "root account setup" "$(in_container sh -c 'echo "$(id -un) $(test -s /var/lib/silo/working-account.json && echo recorded)"')" "silo recorded"
  # No exec leaves its directory (pid file, process JSON with the environment) behind.
  leftovers=1
  for _ in $(seq 1 20); do
    leftovers=$(engine_find -path "*containers/computers/$ctr/state/exec/*" | wc -l)
    [[ "$leftovers" == 0 ]] && break
    sleep 0.5
  done
  expect "exec files left behind" "$leftovers" 0

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

  container_network_features
  container_access_features
  container_checkpoint_features
  container_disk_guard

  step "Stopping and dropping $ctr"
  "$silo" stop "$ctr"
  "$silo" drop "$ctr"
}

# A probe run in the container as the working account: starts Cua Driver the way an agent does
# (the MCP launcher), lists its windows and saves a screenshot of the desktop to $1.
cua_probe_source() {
  base64 -w0 <<'PY'
import json, select, subprocess, sys, time

out = sys.argv[1]
proc = subprocess.Popen(['/usr/local/libexec/silo-cua-driver-mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE, text=True, bufsize=1)

def send(message):
    proc.stdin.write(json.dumps(message) + '\n')
    proc.stdin.flush()

def receive(wanted, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        ready, _, _ = select.select([proc.stdout], [], [], 0.5)
        if not ready:
            if proc.poll() is not None:
                return None
            continue
        line = proc.stdout.readline()
        if not line:
            return None
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get('id') == wanted:
            return message
    return None

def call(identifier, name, arguments):
    send({'jsonrpc': '2.0', 'id': identifier, 'method': 'tools/call', 'params': {'name': name, 'arguments': arguments}})
    reply = receive(identifier, 90)
    result = (reply or {}).get('result', {})
    text = ' '.join(c.get('text', '') for c in result.get('content', []) if c.get('type') == 'text')
    return result.get('isError'), text, reply

send({'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'e2e', 'version': '0'}}})
if not receive(1, 60):
    print('CUA initialize: no answer', proc.stderr.read(2000) if proc.poll() is not None else '')
    sys.exit(1)
send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
error, text, reply = call(2, 'list_windows', {})
print('WINDOWS', error, text[:1500].replace('\n', ' | '))
error, text, reply = call(3, 'get_desktop_state', {'screenshot_out_file': out})
print('STATE', error, text[:300].replace('\n', ' | '))
proc.terminate()
PY
}

# Saves a screenshot of the desktop of $ctr to the evidence folder as $2: the probe above writes
# it in the container, the file is copied out through the working account.
desktop_screenshot() {
  local file="$1" shot="/tmp/e2e-$1" out
  out=$(in_container sh -c "echo $(cua_probe_source) | base64 -d > /tmp/e2e-cua-probe.py && python3 /tmp/e2e-cua-probe.py $shot")
  echo "$out"
  grep -q '^WINDOWS' <<<"$out" || { echo "Cua Driver did not answer" >&2; exit 1; }
  windows=$(grep '^WINDOWS' <<<"$out")
  # A command's output is limited, and a screenshot of a busy page can be larger than the limit
  # once base64 has made it a third bigger: copy it out in pieces.
  : > "$artifacts/$file"
  local piece=0 chunk size
  while :; do
    chunk=$(in_container sh -c "dd if=$shot bs=98304 skip=$piece count=1 2>/dev/null | base64 -w0")
    [[ -n "$chunk" ]] || break
    printf %s "$chunk" | base64 -d >> "$artifacts/$file"
    piece=$((piece + 1))
  done
  size=$(wc -c < "$artifacts/$file")
  [[ "$(head -c 4 "$artifacts/$file" | tail -c 3)" == PNG && "$size" -gt 5000 ]] || { echo "$file is not a screenshot ($size bytes)" >&2; exit 1; }
  echo "screenshot $file: $size bytes"
}

# Whether the desktop's streamer answers on the container's loopback. The viewer's port 6901
# answers 401 without its password and 200 with it (see expect_viewer).
desktop_answers() {
  local code
  code=$(in_container curl -sS -o /dev/null --max-time 5 -w '%{http_code}' http://127.0.0.1:6901/ 2>/dev/null || true)
  [[ "$code" == 401 ]]
}

# The viewer's password, as the desktop service keeps it for root.
viewer_password=""
read_viewer_password() {
  viewer_password=$(root_in_container sh -c 'cat /var/lib/silo-desktop/connection.json' | python3 -c 'import json, sys; print(json.load(sys.stdin)["password"])')
  [[ -n "$viewer_password" ]] || { echo "the desktop service has no viewer password" >&2; exit 1; }
}

# expect_viewer LABEL URL: the viewer at URL (reached from this host) refuses a request without its
# password and serves one with it.
expect_viewer() {
  local label="$1" url="$2"
  expect "$label without the password" "$(curl -sS -o /dev/null --max-time 20 -w '%{http_code}' "$url" || true)" 401
  expect "$label with the password" "$(curl -sS -o /dev/null --max-time 20 -w '%{http_code}' -u "silo:$viewer_password" "$url" || true)" 200
}

wait_for_desktop() {
  local _
  for _ in $(seq 1 90); do
    desktop_answers && return 0
    sleep 2
  done
  echo "The desktop of $ctr did not answer; its service says:" >&2
  root_in_container /usr/local/bin/silo-desktop status >&2 || true
  root_in_container sh -c 'tail -n 60 /var/log/silo-desktop.log' >&2 || true
  return 1
}

# A container computer exported to a file and imported under a new name: its files come back
# (including a login the export warned about, which it never filters), its settings stay, it runs,
# its network is blocked while its published ports answer, and an import on a device that cannot
# run containers is refused in plain words.
container_export_import() {
  local ctr="$ctr6" copy="$ctr7" file output kind
  file=$(mktemp -d)/container.silo-backup
  step "Creating and starting $ctr, a container computer with files to export"
  "$silo" create "$ctr" --container --cpus 1 --memory 2 --disk 2
  "$silo" start "$ctr"
  in_container sh -c 'echo exported-ok > /workspace/e2e-export-note && mkdir -p ~/.claude && echo "{}" > ~/.claude/.credentials.json && ln -s e2e-export-note /workspace/e2e-link && mkdir -p /workspace/e2e-dir && echo nested > /workspace/e2e-dir/inner'

  step "Exporting $ctr: the login warning is the same as a virtual machine's and nothing is filtered"
  if output=$("$silo" export "$ctr" -o "$file" < /dev/null 2>&1); then
    echo "an export of a computer with a login finished without --yes: $output" >&2
    exit 1
  fi
  grep -q 'Claude Code login' <<<"$output" || { echo "the export did not name the login: $output" >&2; exit 1; }
  [[ ! -e "$file" ]] || { echo "a refused export left a file" >&2; exit 1; }
  "$silo" export "$ctr" -o "$file" --yes
  [[ -s "$file" ]] || { echo "the export file is missing or empty" >&2; exit 1; }
  echo "export file: $(wc -c < "$file") bytes"

  step "Importing the file as $copy"
  "$silo" import "$file" --name "$copy"
  kind=$("$silo" --json list --all | python3 -c '
import json, sys
rows = {c["name"]: c["kind"] for c in json.load(sys.stdin)["computers"]}
print(rows.get(sys.argv[1], "missing"))' "$copy")
  expect "kind of the import" "$kind" container
  "$silo" start "$copy"
  step "Running commands in $copy"
  expect "file" "$("$silo" exec "$copy" -- cat /workspace/e2e-export-note)" exported-ok
  expect "link" "$("$silo" exec "$copy" -- readlink /workspace/e2e-link)" e2e-export-note
  expect "nested file" "$("$silo" exec "$copy" -- cat /workspace/e2e-dir/inner)" nested
  expect "the login travels" "$("$silo" exec "$copy" -- sh -c 'test -f ~/.claude/.credentials.json && echo there')" there
  expect "kernel" "$("$silo" exec "$copy" -- uname -s)" Linux
  "$silo" exec "$copy" -- sh -c 'echo changed > /workspace/e2e-export-note'

  step "The import's network is blocked; a port it publishes answers"
  local code state
  code=$("$silo" exec "$copy" -- curl -sS -o /dev/null --max-time 10 -w '%{http_code}' https://example.com 2>/dev/null || true)
  expect "a public host from the import" "${code:-000}" 000
  "$silo" exec "$copy" -- sh -c '(nohup python3 -m http.server 6903 --bind 0.0.0.0 >/dev/null 2>&1 &)'
  "$silo" publish "$copy" 6903
  state=""
  for _ in $(seq 1 40); do
    state=$("$silo" --json ports "$copy" | python3 -c '
import json, sys
port = next((p for p in json.load(sys.stdin)["ports"] if p["port"] == 6903), {})
print(port.get("state") or "-")')
    [[ "$state" == reachable ]] && break
    sleep 0.5
  done
  expect "a port the import publishes" "$state" reachable
  expect "the source is unchanged" "$("$silo" exec "$ctr" -- cat /workspace/e2e-export-note)" exported-ok

  step "Refusing an import under a name that exists, and a damaged file"
  if output=$("$silo" import "$file" --name "$copy" 2>&1); then
    echo "an import under an existing name finished" >&2
    exit 1
  fi
  echo "refused: $output"
  head -c "$(($(wc -c < "$file") / 2))" "$file" > "$file.cut"
  if output=$("$silo" import "$file.cut" --name "$ctr-cut" 2>&1); then
    echo "a truncated file was imported" >&2
    exit 1
  fi
  echo "refused: $output"
  expect "no computer from the refused imports" "$(listed "$ctr-cut")" ""

  step "Stopping and dropping $ctr and $copy"
  "$silo" stop "$copy"
  "$silo" stop "$ctr"
  "$silo" drop "$copy"
  "$silo" drop "$ctr"
  rm -rf "$(dirname "$file")"
  # What the import kept of the file (its checkpoint) goes with the computer.
  leftovers=1
  for _ in $(seq 1 30); do
    leftovers=$(engine_find -path "*containers/snapshots/silo-import-*" -name '*.json' | wc -l)
    [[ "$leftovers" == 0 ]] && break
    sleep 1
  done
  expect "import checkpoints left behind" "$leftovers" 0
}

# A container created with its desktop off. `--no-desktop` sends the request the app's form sends
# (`computers.create` with `configuration.desktop = {startWithComputer: false}`); a recipe cannot
# make a container, so this is the creation-time setting for one.
container_without_desktop() {
  local ctr="$ctr5" state
  step "Creating and starting $ctr, a container computer with its desktop off at creation"
  "$silo" create "$ctr" --container --no-desktop --cpus 1 --memory 2 --disk 4
  expect "desktop setting" "$("$silo" --json status "$ctr" | python3 -c 'import json, sys; print(json.load(sys.stdin)["desktop"])')" False
  "$silo" start "$ctr"
  # The engine's boot command waits for the working account (made by this first start) and runs
  # the desktop boot hook, which starts nothing for a computer whose desktop is off.
  sleep 45
  desktop_answers && { echo "a desktop answers in a computer created with it off" >&2; exit 1; }
  for process in Xvfb xfce4-session selkies; do
    expect "process $process" "$(in_container sh -c "pgrep -f '[${process:0:1}]${process:1}' >/dev/null && echo up || echo down")" down
  done
  state=$(root_in_container /usr/local/bin/silo-desktop status | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')
  expect "desktop service state" "$state" stopped
  step "Stopping and dropping $ctr"
  "$silo" stop "$ctr"
  "$silo" drop "$ctr"
}

# The desktop, computer use and Chrome in a container computer, through the same commands a virtual
# machine's desktop uses. `silo desktop off`, a restart and a fork check the setting after creation.
container_desktop_features() {
  # Every helper above reads $ctr: for these checks it is the desktop container.
  local ctr="$ctr4" host state code url host_port forwarder reply
  [[ -n "$artifacts" ]] || artifacts=$(mktemp -d)
  mkdir -p "$artifacts"

  step "Creating and starting $ctr, a container computer with the desktop on"
  "$silo" create "$ctr" --container --cpus 2 --memory 4 --disk 4
  "$silo" start "$ctr"

  step "Checking that the desktop of $ctr started with it"
  wait_for_desktop
  read_viewer_password
  expect "the viewer inside the computer with its password" "$(in_container curl -sS -o /dev/null --max-time 5 -w '%{http_code}' -u "silo:$viewer_password" http://127.0.0.1:6901/)" 200
  state=$(root_in_container /usr/local/bin/silo-desktop status | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')
  expect "desktop service state" "$state" running
  for process in Xvfb xfce4-session selkies; do
    expect "process $process" "$(in_container sh -c "pgrep -f '[${process:0:1}]${process:1}' >/dev/null && echo up || echo down")" up
  done
  # /run is memory, as a virtual machine's is: what the desktop keeps there (the display's
  # authority cookie, the session's environment) is not in the computer's files.
  expect "/run is a tmpfs" "$(in_container sh -c 'grep -q " /run tmpfs " /proc/mounts && echo yes || echo no')" yes
  expect "the session's environment file" "$(in_container sh -c 'stat -c %a:%U /run/silo-desktop/user/session.env')" 600:silo
  # Every process in the computer can read another's arguments, so the viewer's password is not one.
  expect "the streamer's arguments carry no password" "$(in_container sh -c 'tr "\0" " " < /proc/$(pgrep -f "[s]elkies.*--addr=127" | head -n 1)/cmdline | grep -c basic-auth-password || true')" 0

  step "Reaching the viewer port over SSH, as the app's viewer does"
  host=$("$silo" --json ssh-setup "$ctr" | python3 -c 'import json, sys; print(json.load(sys.stdin)["host"])')
  ssh -N -o BatchMode=yes -o ConnectTimeout=30 -o ExitOnForwardFailure=yes -L 29901:127.0.0.1:6901 "$host" &
  forwarder=$!
  code=000
  for _ in $(seq 1 20); do
    code=$(curl -sS -o /dev/null --max-time 5 -w '%{http_code}' http://127.0.0.1:29901/ 2>/dev/null || true)
    [[ "$code" == 401 ]] && break
    sleep 1
  done
  expect_viewer "the viewer over ssh -L" http://127.0.0.1:29901/
  kill "$forwarder" 2>/dev/null || true
  wait "$forwarder" 2>/dev/null || true

  step "Reaching the viewer port through the URL proxy"
  # The streamer listens on the container's loopback only, as in a virtual machine; a published
  # port reaches the guest's own address, so a forwarder in the container hands 6902 on to it.
  in_container sh -c 'cat > /tmp/e2e-forward.py <<EOF
import socket, threading
def pump(a, b):
    try:
        while (data := a.recv(65536)):
            b.sendall(data)
    except OSError:
        pass
    finally:
        for sock in (a, b):
            try: sock.shutdown(socket.SHUT_RDWR)
            except OSError: pass
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("0.0.0.0", 6902))
server.listen(8)
while True:
    client, _ = server.accept()
    target = socket.create_connection(("127.0.0.1", 6901))
    threading.Thread(target=pump, args=(client, target), daemon=True).start()
    threading.Thread(target=pump, args=(target, client), daemon=True).start()
EOF
(nohup python3 /tmp/e2e-forward.py >/dev/null 2>&1 &)'
  "$silo" publish "$ctr" 6902 --http
  url="" host_port="" state=""
  for _ in $(seq 1 40); do
    read -r state host_port url < <("$silo" --json ports "$ctr" | python3 -c '
import json, sys
port = next((p for p in json.load(sys.stdin)["ports"] if p["port"] == 6902), {})
print(port.get("state") or "-", port.get("hostPort") or "-", port.get("url") or "-")')
    [[ "$state" == reachable && "$url" != - ]] && break
    sleep 0.5
  done
  expect "viewer port state" "$state" reachable
  expect_viewer "the viewer through the URL proxy" "$url"
  expect_viewer "the viewer on the loopback port" "http://127.0.0.1:$host_port/"

  step "Listing windows with Cua Driver and saving a screenshot of $ctr"
  local windows=""
  desktop_screenshot desktop-screenshot.png
  grep -qiE 'panel|desktop' <<<"$windows" || { echo "Cua Driver listed no desktop window: $windows" >&2; exit 1; }

  step "Starting Chrome in $ctr (with --no-sandbox) and loading a page through the gateway"
  if [[ "$(uname -m)" != x86_64 ]]; then
    echo "Google Chrome has no Linux arm64 build: skipped on $(uname -m)"
  else
    root_in_container apt-get update -qq
    for attempt in 1 2 3; do
      root_in_container sh -c 'cd /tmp && curl -fsSL --max-time 300 -o chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb && apt-get -o APT::Sandbox::User=root install -y -qq ./chrome.deb' && break
      ((attempt == 3)) && { echo "Google Chrome did not install" >&2; exit 1; }
      sleep 10
    done
    # Its own sandbox needs a nested user namespace, which the baseline denies: it must fail
    # as it is, and run through the launcher Silo's computer-use setup writes.
    expect "chrome with its sandbox" "$(in_container sh -c '/opt/google/chrome/chrome --headless=new --disable-gpu --user-data-dir=/tmp/e2e-chrome-a --dump-dom about:blank >/dev/null 2>&1; echo $?' | sed 's/^[1-9][0-9]*$/refused/')" refused
    reply=$(in_container sh -c 'google-chrome-stable --headless=new --disable-gpu --user-data-dir=/tmp/e2e-chrome-b --dump-dom https://example.com 2>/dev/null')
    grep -q 'Example Domain' <<<"$reply" || { echo "Chrome did not load https://example.com through the gateway: ${reply:0:300}" >&2; exit 1; }
    echo "Chrome loaded https://example.com through the gateway"
    in_container sh -c 'DISPLAY=:1 XAUTHORITY=/run/silo-desktop/user/Xauthority nohup google-chrome-stable --disable-gpu --no-first-run --user-data-dir=/tmp/e2e-chrome-c https://example.com >/tmp/e2e-chrome.log 2>&1 &'
    for _ in $(seq 1 30); do
      [[ "$(in_container sh -c 'DISPLAY=:1 XAUTHORITY=/run/silo-desktop/user/Xauthority xwininfo -root -tree 2>/dev/null | grep -ci "example domain\|chrome"')" -gt 0 ]] && break
      sleep 2
    done
    expect "chrome window on the desktop" "$([[ "$(in_container sh -c 'DISPLAY=:1 XAUTHORITY=/run/silo-desktop/user/Xauthority xwininfo -root -tree 2>/dev/null | grep -ci "example domain\|chrome"')" -gt 0 ]] && echo yes || echo no)" yes
    expect "chrome runs with --no-sandbox" "$(in_container sh -c 'pgrep -af "[/]opt/google/chrome/chrome" | grep -c -- --no-sandbox | sed "s/^[1-9][0-9]*$/yes/"')" yes
    windows=""
    desktop_screenshot desktop-chrome-screenshot.png
    in_container sh -c 'pkill -u silo -f "[/]opt/google/chrome/chrome" || true'
  fi

  step "Forking $ctr with --gui"
  reply=$("$silo" --json fork "$ctr" --gui --name "$ctr-gui")
  echo "$reply" | python3 -c 'import json, sys; v = json.load(sys.stdin); print("fork --gui:", v["name"], v["state"], v.get("computerUse", {}).get("state"))'
  expect "the fork's desktop" "$("$silo" exec "$ctr-gui" -- sh -c 'curl -sS -o /dev/null --max-time 5 -w "%{http_code}" http://127.0.0.1:6901/ | sed "s/^401$/up/"')" up
  "$silo" stop "$ctr-gui"
  "$silo" drop "$ctr-gui"

  step "Turning the desktop of $ctr off"
  "$silo" desktop off "$ctr"
  desktop_answers && { echo "the desktop still answers after silo desktop off" >&2; exit 1; }
  expect "the session's environment file after desktop off" "$(in_container sh -c 'test -e /run/silo-desktop/user/session.env && echo there || echo gone')" gone
  if reply=$("$silo" fork "$ctr" --gui --name "$ctr-gui" 2>&1); then
    echo "fork --gui worked on a computer with its desktop off" >&2
    "$silo" drop "$ctr-gui" || true
    exit 1
  fi
  grep -q "$ctr has no desktop; turn it on with" <<<"$reply" || { echo "fork --gui said: $reply" >&2; exit 1; }
  echo "fork --gui: $reply"

  step "Restarting $ctr and forking it with its desktop off"
  "$silo" stop "$ctr"
  "$silo" start "$ctr"
  sleep 30
  desktop_answers && { echo "the desktop started with the computer although it is off" >&2; exit 1; }
  "$silo" fork "$ctr" --name "$ctr-plain"
  [[ "$("$silo" --json status "$ctr-plain" | python3 -c 'import json, sys; print(json.load(sys.stdin)["state"])')" == running ]] || "$silo" start "$ctr-plain"
  sleep 30
  expect "the fork's desktop" "$("$silo" exec "$ctr-plain" -- sh -c 'curl -sS -o /dev/null --max-time 3 http://127.0.0.1:6901/ >/dev/null 2>&1 && echo up || echo down')" down
  "$silo" stop "$ctr-plain"
  "$silo" drop "$ctr-plain"

  step "Turning the desktop of $ctr on again"
  "$silo" desktop on "$ctr"
  wait_for_desktop
  echo "the desktop answers again"

  step "Stopping and dropping $ctr"
  "$silo" stop "$ctr"
  "$silo" drop "$ctr"
}

# How many times the gateway recorded using secret $1 toward host $2 (names and hosts, never
# values; the file `computers.credentials` reads for any Linux computer).
usage_count() {
  local file
  file=$(engine_find -path "*sandboxes/$ctr/logs/secret-usage.jsonl" | head -n 1)
  if [[ -z "$file" ]]; then echo 0; return; fi
  python3 - "$file" "$1" "$2" <<'PY'
import json, sys
count = 0
for line in open(sys.argv[1]):
    try:
        event = json.loads(line)
    except ValueError:
        continue
    if event.get("kind") == "used" and event.get("secret") == sys.argv[2] and event.get("host") == sys.argv[3]:
        count += 1
print(count)
PY
}

# Waits up to $2 seconds for the number of recorded uses of secret $1 toward host $3 to be $4 or
# more (the gateway writes its events a moment after the request).
wait_for_usage() {
  local secret="$1" seconds="$2" host="$3" wanted="$4" seen=0
  for _ in $(seq 1 "$((seconds * 2))"); do
    seen=$(usage_count "$secret" "$host")
    ((seen >= wanted)) && break
    sleep 0.5
  done
  echo "$seen"
}

# What to read when a published port does not become reachable: the engine's own view, the guest's
# listeners, the loopback port, and the port control socket asked directly.
port_diagnostics() {
  echo "== Port diagnostics for $ctr" >&2
  "$silo" --json ports "$ctr" >&2 || true
  in_container sh -c 'cat /proc/net/tcp; ps -ef' >&2 || true
  curl -sv --max-time 5 "http://127.0.0.1:$1/" >&2 || true
  local socket
  socket=$(engine_find -path '*/run/sandboxes/*/control.sock' | head -n 1)
  echo "control socket: ${socket:-none}" >&2
  if [[ -n "$socket" ]]; then
    python3 - "$socket" <<'PY' >&2 || true
import json, socket, sys
for request in ({"op": "ports_list"}, {"op": "port_probe", "guest_port": 8080}):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(10)
        s.connect(sys.argv[1])
        s.sendall(json.dumps(request).encode() + b"\n")
        print(request, "->", s.makefile().readline().strip())
PY
  fi
  engine_find -path "*containers/computers/$ctr/state/gateway.log" -exec sh -c 'echo "-- $1"; tail -n 30 "$1"' _ {} \; >&2
}

# Secrets, published ports and the URL proxy in the container, through the same commands a virtual
# machine uses.
container_network_features() {
  step "Giving $ctr a fake secret"
  # A random token shaped like a GitHub one: GitHub answers "Bad credentials" to it, and a
  # request with no token at all "Requires authentication".
  local fake placeholder='$MSB_E2E_FAKE_TOKEN'
  fake="ghp_$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 36)"
  "$silo" create "$ctr2" --container --cpus 1 --memory 2 --disk 2
  printf '%s' "$fake" | "$silo" secrets set E2E_FAKE_TOKEN --computer "$ctr" --computer "$ctr2" --domain api.github.com
  # A secret assigned to a running computer takes effect at its next start, as for a VM.
  "$silo" stop "$ctr"
  "$silo" start "$ctr"

  step "Checking what the guest sees"
  expect "guest placeholder" "$(in_container sh -c 'printf %s "$E2E_FAKE_TOKEN"')" "$placeholder"
  expect "GitHub placeholder" "$(in_container sh -c 'printf %s "$GH_TOKEN"')" '$MSB_SILO_GITHUB'
  expect "the value inside the guest" "$(in_container sh -c "cat /proc/self/environ /proc/1/environ 2>/dev/null | grep -c '$fake' || true")" 0

  step "Reinstalling ca-certificates in $ctr, as an upgrade of it would"
  # The guest's trust bundle is its own file: the package replaces it by renaming a new one over
  # it, which fails with EBUSY when a mount sits on the path. Silo adds only the gateway's CA
  # certificate, as a local one, and runs update-ca-certificates at each start. The substitution
  # check below then shows HTTPS through the gateway still verifies after the reinstall, with no
  # restart in between.
  local installed=0
  for _ in 1 2 3; do
    if root_in_container apt-get update -qq && root_in_container apt-get install -y -qq --reinstall ca-certificates; then
      installed=1
      break
    fi
    sleep 5
  done
  [[ "$installed" == 1 ]] || { echo "reinstalling ca-certificates in $ctr failed" >&2; exit 1; }
  root_in_container update-ca-certificates --fresh
  expect "gateway CA in the trust store" "$(in_container sh -c 'test -s /etc/ssl/certs/silo-gateway.pem && echo linked')" linked
  expect "bundle variable" "$(in_container sh -c 'printf %s "$SSL_CERT_FILE"')" /etc/ssl/certs/ca-certificates.crt
  expect "node variable" "$(in_container sh -c 'printf %s "$NODE_EXTRA_CA_CERTS"')" /usr/local/share/ca-certificates/silo-gateway.crt
  # The guest can add a CA of its own beside the gateway's.
  root_in_container sh -c 'cp /usr/local/share/ca-certificates/silo-gateway.crt /usr/local/share/ca-certificates/e2e-extra.crt && update-ca-certificates' >/dev/null
  expect "a CA the guest added" "$(in_container sh -c 'test -s /etc/ssl/certs/e2e-extra.pem && echo linked')" linked
  root_in_container sh -c 'rm -f /usr/local/share/ca-certificates/e2e-extra.crt && update-ca-certificates --fresh' >/dev/null

  step "Checking that the secret is substituted for its host and no other"
  local reply status
  for _ in 1 2 3; do
    reply=$(in_container curl -sS --max-time 20 -H 'Authorization: Bearer $MSB_E2E_FAKE_TOKEN' -w '\n%{http_code}' https://api.github.com/user || true)
    [[ "${reply##*$'\n'}" == 401 ]] && break
    sleep 3
  done
  status="${reply##*$'\n'}"
  expect "allowed host status" "$status" 401
  if [[ "$reply" != *"Bad credentials"* ]]; then echo "allowed host: GitHub did not answer Bad credentials: $reply" >&2; exit 1; fi
  echo "allowed host: Bad credentials"
  local used
  used=$(wait_for_usage E2E_FAKE_TOKEN 10 api.github.com 1)
  if ((used < 1)); then
    echo "the gateway recorded no use of the secret toward api.github.com" >&2
    engine_find -name 'secret-usage*' -exec sh -c 'echo "-- $1"; tail -n 5 "$1"' _ {} \; >&2
    engine_find -path "*containers/computers/$ctr/state/gateway.log" -exec sh -c 'echo "-- $1"; tail -n 20 "$1"' _ {} \; >&2
    exit 1
  fi
  echo "uses recorded toward api.github.com: $used"
  in_container curl -sS --max-time 20 -o /dev/null -H 'Authorization: Bearer $MSB_E2E_FAKE_TOKEN' https://example.com/ || true
  sleep 2
  expect "uses recorded toward another host" "$(usage_count E2E_FAKE_TOKEN example.com)" 0

  step "Publishing a port of $ctr and opening it through the URL proxy"
  in_container sh -c 'mkdir -p /tmp/www && echo ct3-port-ok > /tmp/www/index.html && cd /tmp/www && (nohup python3 -m http.server 8080 --bind 0.0.0.0 >/dev/null 2>&1 &)'
  "$silo" publish "$ctr" 8080 --http
  local url="" host_port="" state=""
  for _ in $(seq 1 40); do
    read -r state host_port url < <("$silo" --json ports "$ctr" | python3 -c '
import json, sys
port = next((p for p in json.load(sys.stdin)["ports"] if p["port"] == 8080), {})
print(port.get("state") or "-", port.get("hostPort") or "-", port.get("url") or "-")')
    [[ "$state" == reachable && "$url" != - ]] && break
    sleep 0.5
  done
  if [[ "$state" != reachable ]]; then port_diagnostics "$host_port"; fi
  expect "published port state" "$state" reachable
  echo "published at $url (host port $host_port)"
  expect "page through the URL proxy" "$(curl -sS --max-time 20 "$url")" ct3-port-ok
  expect "page on the loopback port" "$(curl -sS --max-time 20 "http://127.0.0.1:$host_port/")" ct3-port-ok
  "$silo" unpublish "$ctr" 8080
  for _ in $(seq 1 20); do
    curl -sS --max-time 3 -o /dev/null "http://127.0.0.1:$host_port/" 2>/dev/null || break
    sleep 0.5
  done
  if curl -sS --max-time 3 -o /dev/null "http://127.0.0.1:$host_port/" 2>/dev/null; then
    echo "the unpublished port still answers on the loopback" >&2
    exit 1
  fi
  echo "unpublished port: closed"

  step "Taking the secret away from the running $ctr"
  "$silo" secrets unassign E2E_FAKE_TOKEN "$ctr"
  local before after assigned
  before=$(usage_count E2E_FAKE_TOKEN api.github.com)
  in_container curl -sS --max-time 20 -o /dev/null -H 'Authorization: Bearer $MSB_E2E_FAKE_TOKEN' https://api.github.com/user || true
  sleep 3
  after=$(usage_count E2E_FAKE_TOKEN api.github.com)
  expect "uses recorded after the removal" "$after" "$before"
  expect "placeholder after the removal" "$(in_container sh -c 'printf %s "${E2E_FAKE_TOKEN-}"')" ""
  assigned=$("$silo" secrets list)
  if grep -w E2E_FAKE_TOKEN <<<"$assigned" | grep -qw -- "$ctr"; then
    echo "the secret is still assigned: $assigned" >&2
    exit 1
  fi
  echo "secret removal: no longer assigned"
  "$silo" drop "$ctr2"
}

# The runtime home of the container computer: the folder that holds containers/ and sandboxes/.
container_home() {
  local computer
  computer=$(engine_find -path "*containers/computers/$ctr" -type d | head -n 1)
  [[ -n "$computer" ]] || { echo "no runtime home found for $ctr" >&2; return 1; }
  dirname "$(dirname "$(dirname "$computer")")"
}

editor_key_file=""
remove_editor_key() {
  if [[ -n "$editor_key_file" && -f "${editor_home:-/nonexistent}/ssh/authorized_keys" ]]; then
    sed -i '/ silo-e2e-editor$/d' "$editor_home/ssh/authorized_keys" || true
  fi
}

# Port forwarding to the container's loopback, a large transfer in both directions at once, a
# client that dies, and idle sockets, over the SSH server of the container computer.
container_forwarding_and_transfers() {
  local host="$1" scratch="$2" forwarders=() pid
  local ssh_opts=(-o BatchMode=yes -o ConnectTimeout=30 -o ExitOnForwardFailure=yes)

  step "Forwarding ports of $ctr over SSH (-L and -D)"
  # A server on the container's loopback only: the host has nothing on this port, so an answer
  # proves the connection was made from inside the container's network namespace.
  in_container sh -c 'mkdir -p /tmp/fwd && echo fwd-ok > /tmp/fwd/index.html && cd /tmp/fwd && (nohup python3 -m http.server 18080 --bind 127.0.0.1 >/dev/null 2>&1 &)'
  for _ in $(seq 1 20); do
    [[ "$(in_container curl -sS --max-time 3 http://127.0.0.1:18080/index.html 2>/dev/null || true)" == fwd-ok ]] && break
    sleep 1
  done
  expect "container server" "$(in_container curl -sS --max-time 5 http://127.0.0.1:18080/index.html)" fwd-ok
  if curl -sS --max-time 3 -o /dev/null http://127.0.0.1:18080/ 2>/dev/null; then
    echo "the host serves port 18080 itself; the forwarding check would prove nothing" >&2
    exit 1
  fi
  ssh -N "${ssh_opts[@]}" -L 28080:127.0.0.1:18080 -L 28081:10.255.255.1:80 -L 28082:localhost:18080 "$host" &
  forwarders+=($!)
  ssh -N "${ssh_opts[@]}" -D 21080 "$host" &
  forwarders+=($!)
  local answer=""
  for _ in $(seq 1 20); do
    answer=$(curl -sS --max-time 5 http://127.0.0.1:28080/index.html 2>/dev/null || true)
    [[ "$answer" == fwd-ok ]] && break
    sleep 1
  done
  expect "ssh -L to a port served inside the container" "$answer" fwd-ok
  expect "ssh -L to localhost" "$(curl -sS --max-time 5 http://127.0.0.1:28082/index.html)" fwd-ok
  expect "ssh -D (SOCKS) to the container's loopback" "$(curl -sS --max-time 10 --socks5-hostname 127.0.0.1:21080 http://127.0.0.1:18080/index.html)" fwd-ok
  # Anything but the container's loopback is refused by the server.
  if curl -sS --max-time 5 -o /dev/null http://127.0.0.1:28081/ 2>/dev/null; then
    echo "a forward to 10.255.255.1 was served" >&2
    exit 1
  fi
  if curl -sS --max-time 10 -o /dev/null --socks5-hostname 127.0.0.1:21080 http://example.com/ 2>/dev/null; then
    echo "a SOCKS forward to a public host was served" >&2
    exit 1
  fi
  echo "forwards to the loopback answer; other destinations are refused"
  for pid in "${forwarders[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true

  step "Sending 60 MB both ways through an ssh -L forward, and a forward whose target does not read"
  # One server in the container echoes (so the transfer goes both ways at once); another accepts
  # a connection and never reads it. A forward is flow-controlled like a command: the echo comes
  # back whole, and the target that does not read holds back its own client and nothing else on
  # the connection (a shell command on it still answers).
  local servers
  servers=$(base64 -w0 <<'PY'
import socket, threading, time

def listener(port):
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", port))
    server.listen(8)
    return server

def echo(connection):
    while True:
        data = connection.recv(65536)
        if not data:
            break
        connection.sendall(data)
    connection.close()

def serve_echo(server):
    while True:
        connection, _ = server.accept()
        threading.Thread(target=echo, args=(connection,), daemon=True).start()

def serve_deaf(server):
    held = []
    while True:
        held.append(server.accept()[0])

threading.Thread(target=serve_echo, args=(listener(18081),), daemon=True).start()
threading.Thread(target=serve_deaf, args=(listener(18082),), daemon=True).start()
time.sleep(3600)
PY
)
  in_container sh -c "echo $servers | base64 -d > /tmp/fwd/servers.py && (nohup python3 /tmp/fwd/servers.py >/dev/null 2>&1 &)"
  for _ in $(seq 1 20); do
    in_container python3 -c 'import socket;socket.create_connection(("127.0.0.1",18082),2).close();socket.create_connection(("127.0.0.1",18081),2).close()' 2>/dev/null && break
    sleep 1
  done
  local control="$scratch/forward-control"
  ssh -M -S "$control" -N "${ssh_opts[@]}" -L 28083:127.0.0.1:18081 -L 28084:127.0.0.1:18082 "$host" &
  forwarders+=($!)
  for _ in $(seq 1 20); do
    ssh -S "$control" -O check "$host" 2>/dev/null && break
    sleep 1
  done
  # The deaf target first: a client sends it far more than the window and waits.
  python3 -I -c '
import socket, sys, time
connection = socket.create_connection(("127.0.0.1", 28084), 10)
chunk = b"x" * 65536
deadline = time.time() + 120
while time.time() < deadline:
    connection.sendall(chunk)
' &
  local stuck=$!
  sleep 3
  expect "a shell command on the connection while a forward's target does not read" "$(timeout 30 ssh -S "$control" "$host" 'echo still-answering')" still-answering
  # The echo through the same connection, 60 MB sent and read back at once, hashes compared.
  local echoed
  echoed=$(timeout 300 python3 -I -c '
import hashlib, os, socket, sys, threading
total = 60_000_000
connection = socket.create_connection(("127.0.0.1", 28083), 10)
sent = hashlib.sha256()
def send():
    left = total
    while left:
        chunk = os.urandom(min(65536, left))
        sent.update(chunk)
        connection.sendall(chunk)
        left -= len(chunk)
sender = threading.Thread(target=send)
sender.start()
received = hashlib.sha256()
left = total
while left:
    data = connection.recv(min(65536, left))
    if not data:
        sys.exit("the forward ended with %d bytes missing" % left)
    received.update(data)
    left -= len(data)
sender.join()
print("same" if sent.hexdigest() == received.hexdigest() else "different")
')
  expect "60 MB through an ssh -L forward, both ways at once" "$echoed" same
  expect "a shell command after the transfer" "$(timeout 30 ssh -S "$control" "$host" 'echo still-answering')" still-answering
  kill "$stuck" 2>/dev/null || true
  ssh -S "$control" -O exit "$host" 2>/dev/null || true
  wait 2>/dev/null || true
  in_container sh -c "pkill -f 'fwd/servers[.]py'" || true

  step "Sending a large file through ssh cat in both directions at once, five times"
  local ssh_plain=(-o BatchMode=yes -o ConnectTimeout=30) round big_hash
  head -c 60000000 /dev/urandom > "$scratch/big.bin"
  big_hash=$(sha256sum < "$scratch/big.bin" | cut -d' ' -f1)
  # Input and output both flow at full speed: a server that stops reading the client while it
  # waits for the command's input stalls here, and one that ends a command that falls behind
  # fails here at random, so every one of the five rounds must give the same hash.
  for round in 1 2 3 4 5; do
    timeout 180 ssh "${ssh_plain[@]}" "$host" cat < "$scratch/big.bin" > "$scratch/big.out"
    expect "hash of the file through cat (round $round of 5)" "$(sha256sum < "$scratch/big.out" | cut -d' ' -f1)" "$big_hash"
  done

  step "Sending a large file to a command that reads slowly"
  # The command takes 64 KiB every 10 ms, far slower than the client sends: the server holds the
  # client back with the SSH window instead of ending the command, so the command sees every byte.
  local slow_reader='python3 -c '"'"'import sys,time,hashlib;h=hashlib.sha256();[(h.update(c),time.sleep(0.01)) for c in iter(lambda:sys.stdin.buffer.read1(65536),b"")];print(h.hexdigest())'"'"
  expect "hash of the file read slowly" "$(timeout 300 ssh "${ssh_plain[@]}" "$host" "$slow_reader" < "$scratch/big.bin")" "$big_hash"
  rm -f "$scratch/big.out"
  timeout 180 ssh "${ssh_plain[@]}" "$host" 'cat > /workspace/e2e-big.bin' < "$scratch/big.bin"
  expect "hash of the file in the container" "$(in_container sha256sum /workspace/e2e-big.bin | cut -d' ' -f1)" "$big_hash"
  timeout 180 ssh "${ssh_plain[@]}" "$host" 'cat /workspace/e2e-big.bin' > "$scratch/big.down"
  cmp "$scratch/big.bin" "$scratch/big.down"
  in_container rm -f /workspace/e2e-big.bin
  rm -f "$scratch/big.bin" "$scratch/big.out" "$scratch/big.down"

  step "Ending a remote command when its client goes away"
  in_container rm -f /tmp/e2e-client-pid
  ssh "${ssh_plain[@]}" "$host" 'echo $$ > /tmp/e2e-client-pid; exec sleep 300' &
  local client=$!
  for _ in $(seq 1 20); do
    [[ -n "$(in_container cat /tmp/e2e-client-pid 2>/dev/null || true)" ]] && break
    sleep 1
  done
  local remote
  remote=$(in_container cat /tmp/e2e-client-pid)
  expect "remote command running" "$(in_container sh -c "kill -0 $remote 2>/dev/null && echo alive || echo gone")" alive
  kill -9 "$client" 2>/dev/null || true
  wait "$client" 2>/dev/null || true
  local state=alive
  for _ in $(seq 1 20); do
    state=$(in_container sh -c "kill -0 $remote 2>/dev/null && echo alive || echo gone")
    [[ "$state" == gone ]] && break
    sleep 1
  done
  expect "remote command after its client was killed" "$state" gone

  step "Idle sockets do not lock out a login"
  local port address
  address=$(ssh -G "$host" | awk '$1 == "hostname" { print $2 }')
  port=$(ssh -G "$host" | awk '$1 == "port" { print $2 }')
  # Twice the limit of logged-in connections, doing nothing.
  python3 -c '
import socket, sys, time
sockets = [socket.create_connection((sys.argv[1], int(sys.argv[2]))) for _ in range(40)]
open(sys.argv[3], "w").write("held")
time.sleep(60)' "$address" "$port" "$scratch/idle-held" &
  local idle=$!
  for _ in $(seq 1 20); do [[ -f "$scratch/idle-held" ]] && break; sleep 1; done
  expect "idle sockets held" "$(cat "$scratch/idle-held" 2>/dev/null || true)" held
  expect "a login while idle sockets are open" "$(ssh "${ssh_plain[@]}" "$host" id -un)" silo
  kill "$idle" 2>/dev/null || true
  wait "$idle" 2>/dev/null || true
}

# SSH, SFTP, a terminal, the editor's proxy and Silo's tools, which a container computer has
# through the same commands as a virtual machine.
container_access_features() {
  local scratch host
  scratch=$(mktemp -d)

  step "Setting up SSH for $ctr and running commands over it"
  host=$("$silo" --json ssh-setup "$ctr" | python3 -c 'import json, sys; print(json.load(sys.stdin)["host"])')
  local ssh_opts=(-o BatchMode=yes -o ConnectTimeout=30)
  # The listener is started by the engine when access is enabled; give it a moment.
  local seen=""
  for _ in $(seq 1 20); do
    seen=$(ssh "${ssh_opts[@]}" "$host" uname -a 2>/dev/null || true)
    [[ "$seen" == Linux* ]] && break
    sleep 1
  done
  if [[ "$seen" != Linux* ]]; then
    ssh -v "${ssh_opts[@]}" "$host" uname -a >&2 || true
    echo "ssh $host uname -a did not print a Linux kernel" >&2
    exit 1
  fi
  echo "ssh $host uname -a: $seen"
  expect "ssh account" "$(ssh "${ssh_opts[@]}" "$host" id -un)" silo
  expect "ssh exit status" "$(ssh "${ssh_opts[@]}" "$host" 'exit 7' >/dev/null 2>&1; echo $?)" 7
  expect "ssh without a terminal" "$(ssh "${ssh_opts[@]}" "$host" 'tty' 2>/dev/null || true)" "not a tty"
  expect "ssh standard input" "$(printf 'piped-input' | ssh "${ssh_opts[@]}" "$host" cat)" piped-input
  expect "ssh working directory" "$(ssh "${ssh_opts[@]}" "$host" pwd)" /home/silo

  step "Copying files to and from $ctr over SFTP"
  head -c 300000 /dev/urandom > "$scratch/up.bin"
  printf 'put %s /workspace/e2e-sftp.bin\nget /workspace/e2e-sftp.bin %s\nls -l /workspace\n' "$scratch/up.bin" "$scratch/down.bin" | sftp "${ssh_opts[@]}" -b - "$host"
  cmp "$scratch/up.bin" "$scratch/down.bin"
  expect "file written over sftp, read in the guest" "$(in_container sh -c 'wc -c < /workspace/e2e-sftp.bin | tr -d " "')" 300000
  expect "file owner" "$(in_container stat -c %U /workspace/e2e-sftp.bin)" silo

  container_forwarding_and_transfers "$host" "$scratch"

  step "Opening a terminal session in $ctr over SSH"
  local tty_name
  tty_name=$(ssh -tt "${ssh_opts[@]}" "$host" tty | tr -d '\r')
  if [[ ! "$tty_name" =~ ^/dev/pts/[0-9]+$ ]]; then echo "ssh -tt tty printed '$tty_name'" >&2; exit 1; fi
  echo "terminal: $tty_name"
  local sized
  sized=$(ssh -tt "${ssh_opts[@]}" "$host" 'stty size' | tr -d '\r')
  [[ "$sized" =~ ^[0-9]+\ [0-9]+$ ]] || { echo "stty size printed '$sized'" >&2; exit 1; }
  # A shell on a terminal reads the commands it is sent and ends with the connection.
  expect "interactive shell" "$(printf 'echo shell-$((6*7))\nexit\n' | ssh -tt "${ssh_opts[@]}" "$host" | tr -d '\r' | grep -o 'shell-42' | head -n 1)" shell-42

  step "Opening the terminal command of $ctr"
  # The command the app and `silo` launch in a terminal emulator, run on a pseudo-terminal.
  # The runtime home exactly as the engine gave it to the SSH listener it started (the unit name
  # of a container is derived from that text).
  local home listener
  listener=$(pgrep -f -- '--container-runtime ssh serve' | head -n 1)
  home=$(tr '\0' '\n' < "/proc/$listener/environ" | sed -n 's/^MSB_HOME=//p')
  [[ -n "$home" ]] || { echo "no SSH listener to read the runtime home from" >&2; exit 1; }
  echo "runtime home: $home (found: $(container_home))"
  local terminal=(/usr/bin/env "MSB_HOME=$home" "MSB_PATH=$engine_dir/msb" "MSB_LIBKRUNFW_PATH=$engine_dir/msb"
    "$engine_dir/silod" --container-runtime exec "$ctr" --user silo --env USER=silo --env LOGNAME=silo --no-start --workdir /workspace --tty)
  local typed
  local code=0
  typed=$(printf 'tty; id -un; pwd; exit\n' | script -qec "${terminal[*]}" /dev/null 2>&1 | tr -d '\r') || code=$?
  echo "terminal command output (status $code):"
  printf '%s\n' "$typed"
  grep -Eq '/dev/pts/[0-9]+$' <<<"$typed" || { echo "the terminal printed: $typed" >&2; exit 1; }
  grep -qx 'silo' <<<"$typed" || { echo "the terminal did not run as silo: $typed" >&2; exit 1; }
  grep -qx '/workspace' <<<"$typed" || { echo "the terminal did not start in /workspace: $typed" >&2; exit 1; }
  echo "terminal command: ok"

  step "Using the editor's proxy (ssh serve --stdio) in $ctr"
  editor_home="$home"
  mkdir -p "$home/ssh"
  ssh-keygen -q -t ed25519 -N '' -f "$scratch/editor_key" -C silo-e2e-editor
  editor_key_file="$scratch/editor_key"
  cat "$scratch/editor_key.pub" >> "$home/ssh/authorized_keys"
  local host_key="$home/sandboxes/$ctr/ssh/host_ed25519"
  [[ -f "$host_key" ]] || { echo "no host key at $host_key" >&2; exit 1; }
  printf 'silo-e2e-editor %s\n' "$(ssh-keygen -y -f "$host_key")" > "$scratch/known_hosts"
  local proxy="/usr/bin/env MSB_HOME=$home MSB_PATH=$engine_dir/msb MSB_LIBKRUNFW_PATH=$engine_dir/msb $engine_dir/silod --container-runtime ssh serve $ctr --stdio --no-start --no-inactivity-timeout"
  local editor_ssh=(ssh -F /dev/null -o BatchMode=yes -o IdentitiesOnly=yes -o IdentityAgent=none
    -o "UserKnownHostsFile=$scratch/known_hosts" -o StrictHostKeyChecking=yes -o "ProxyCommand=$proxy")
  expect "editor proxy" "$("${editor_ssh[@]}" -o "IdentityFile=$scratch/editor_key" -l silo silo-e2e-editor uname -s)" Linux
  # A key that is not listed is refused, and so is a login for another account.
  ssh-keygen -q -t ed25519 -N '' -f "$scratch/stranger" -C stranger
  if "${editor_ssh[@]}" -o "IdentityFile=$scratch/stranger" -l silo silo-e2e-editor true 2>/dev/null; then
    echo "a key that is not authorized logged in" >&2
    exit 1
  fi
  if "${editor_ssh[@]}" -o "IdentityFile=$scratch/editor_key" -l root silo-e2e-editor true 2>/dev/null; then
    echo "a login for another account succeeded" >&2
    exit 1
  fi
  echo "editor proxy refuses an unlisted key and another account"
  remove_editor_key
  expect "authorized key removed" "$(grep -c silo-e2e-editor "$home/ssh/authorized_keys" || true)" 0

  step "Silo's tools in $ctr"
  mkdir -p "$scratch/project"
  (cd "$scratch/project" && "$silo" tools enable "$ctr" --binary "$silo")
  expect "client binary" "$(in_container sh -c 'test -x /usr/local/bin/silo && echo installed')" installed
  expect "client account" "$(in_container id -u)" 1001
  expect "endpoint file" "$(in_container sh -c 'test -r /etc/silo/endpoint.json && echo readable')" readable
  local listed_inside
  listed_inside=$(in_container silo --json list)
  if ! grep -q "\"$ctr\"" <<<"$listed_inside"; then
    echo "silo inside $ctr did not list it: $listed_inside" >&2
    exit 1
  fi
  echo "list inside the computer: ok"
  # The engine answers a computer's calls as that computer: an owner-only call is refused.
  if in_container silo secrets list >/dev/null 2>&1; then
    echo "silo inside $ctr read the device's secrets" >&2
    exit 1
  fi
  (cd "$scratch/project" && "$silo" tools disable "$ctr") >/dev/null
  rm -rf "$scratch"
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
if ! existing=$(listed "$name" "$fork" "$ctr" "$ctr2"); then
  echo "Could not list the engine's computers; this test never runs without knowing them" >&2
  exit 1
fi
if [[ -n "$existing" ]]; then
  echo "A computer named $name, $fork, $ctr, $ctr2 or $ctr3 already exists; this test never touches it" >&2
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

vm_exports_hold_no_memory

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
left=$(listed "$name" "$fork" "$ckpt_fork")
if [[ -n "$left" ]]; then
  echo "Still listed after drop: $left" >&2
  exit 1
fi
if [[ "${SILO_E2E_CONTAINER:-0}" == 1 ]]; then
  container_checks
  container_export_import
  container_desktop_features
  container_without_desktop
  left=$(listed "$ctr" "$ctr2" "$ctr3" "$ctr4" "$ctr5" "$ctr6" "$ctr7")
  if [[ -n "$left" ]]; then
    echo "Still listed after drop: $left" >&2
    exit 1
  fi
fi
trap - EXIT
step "Passed"
