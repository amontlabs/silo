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
engine_dir="$1"
silo="$1/silo"
name="${SILO_E2E_COMPUTER:-e2e-hosts-ci}"
fork="$name-fork"
ctr="$name-ctr"
# A second container that keeps the secret when it is taken away from the first (a secret always
# has a computer).
ctr2="$name-ctr2"
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
  rm -f "${stderr_file:-}"
  remove_editor_key
  for computer in "$ctr2" "$ctr" "$fork" "$name"; do
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
# need root, such as a package install, use this.
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
    "env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root", "DEBIAN_FRONTEND=noninteractive"],
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

  step "Sending a large file through ssh cat in both directions at once"
  local ssh_plain=(-o BatchMode=yes -o ConnectTimeout=30)
  head -c 60000000 /dev/urandom > "$scratch/big.bin"
  # Input and output both flow at full speed: a server that stops reading the client while it
  # waits for the command's input stalls here, and the timeout fails the check.
  timeout 180 ssh "${ssh_plain[@]}" "$host" cat < "$scratch/big.bin" > "$scratch/big.out"
  expect "hash of the file through cat" "$(sha256sum < "$scratch/big.out" | cut -d' ' -f1)" "$(sha256sum < "$scratch/big.bin" | cut -d' ' -f1)"
  timeout 180 ssh "${ssh_plain[@]}" "$host" 'cat > /workspace/e2e-big.bin' < "$scratch/big.bin"
  expect "hash of the file in the container" "$(in_container sha256sum /workspace/e2e-big.bin | cut -d' ' -f1)" "$(sha256sum < "$scratch/big.bin" | cut -d' ' -f1)"
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

# SSH, SFTP, a terminal, the editor's proxy and the in-guest client, which a container computer has
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

  step "Installing the in-guest client in $ctr"
  mkdir -p "$scratch/project"
  (cd "$scratch/project" && "$silo" guest-client install "$ctr" --binary "$silo")
  expect "client binary" "$(in_container sh -c 'test -x /usr/local/bin/silo && echo installed')" installed
  expect "client account" "$(in_container id -u)" 1001
  expect "bridge folder owner" "$(in_container stat -c %U /.msb/silo-bridge)" silo
  local listed_inside
  listed_inside=$(in_container silo --in-guest --json list)
  if ! grep -q "\"$ctr\"" <<<"$listed_inside"; then
    echo "the in-guest client did not list $ctr: $listed_inside" >&2
    engine_find -path "*sandboxes/$ctr/runtime" -exec ls -lan {} \; >&2 || true
    exit 1
  fi
  echo "in-guest list: ok"
  # The client reaches only its own computer and forks; a keep is not offered at all.
  if in_container silo --in-guest keep "$ctr" >/dev/null 2>&1; then
    echo "the in-guest client ran a keep" >&2
    exit 1
  fi
  (cd "$scratch/project" && "$silo" guest-client revoke "$ctr") >/dev/null
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
  echo "A computer named $name, $fork, $ctr or $ctr2 already exists; this test never touches it" >&2
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
  left=$(listed "$ctr" "$ctr2")
  if [[ -n "$left" ]]; then
    echo "Still listed after drop: $left" >&2
    exit 1
  fi
fi
trap - EXIT
step "Passed"
