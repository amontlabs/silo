# shellcheck shell=bash
# Helpers for tests 40 and 43: a computer whose network card leads nowhere (#378). Sourced after
# lib/common.sh, lib/vm.sh and lib/net.sh. Written for macOS /bin/bash 3.2.
#
# One folder D holds everything of one such computer on the Mac's side:
#   D/de.sock         the card's other end: py/deadend.py, which answers ARP for the router
#                     10.254.0.1 and drops every other frame (silovm --net vfkit:D/de.sock)
#   D/de-counts.json  what it saw: frames by kind, and the IP destinations the computer tried
#   D/proxy.sock      Silo's proxy stand-in, py/kitproxy.py: the only thing that looks a name up
#                     or dials out; one JSON line per request in D/proxy.jsonl
#   D/ssh.sock        the way in: silovm --vsock-in 22:D/ssh.sock, to vsockrelay in the computer,
#                     to its own SSH server on 127.0.0.1:22 (net_ssh D USER reaches it)
# Inside, vsockrelay (c/vsockrelay.c) runs twice as a launch daemon: "in" on vsock port 22, and
# "out" on 127.0.0.1:3128, carried to vsock port 3128 of the Mac, which silovm hands to
# D/proxy.sock. The card has 10.254.0.2/24, router 10.254.0.1, and no name server.
#
# On Linux (the analogue) a network namespace stands for the computer, linux/fake-guest-net.py
# for its card, and Unix sockets for vsock, as #378 measured. D/guest.pid holds the namespace's
# process (de_boot runs in a command substitution, so no variable carries it); de_sh and de_root
# run a command inside it.

DE_ADDR=10.254.0.2
DE_ROUTER=10.254.0.1
DE_MASK=255.255.255.0
DE_PROXY_PORT=3128
DE_RELAY_GUEST=/usr/local/libexec/silokit-vsockrelay
de_linux() { [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; }
_de_ns() { cat "$1/guest.pid" 2>/dev/null; }

# de_card_start D [--no-arp]: the card's other end. Writes D/de.pid.
de_card_start() {
  local d="$1"; shift
  rm -f "$d/de.sock" "$d/de-counts.json"
  python3 -I "$KIT_ROOT/py/deadend.py" --sock "$d/de.sock" --counts "$d/de-counts.json" --router "$DE_ROUTER" "$@" \
    > "$d/de.log" 2>&1 &
  echo $! > "$d/de.pid"
  local i=0; while [ ! -S "$d/de.sock" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
  [ -S "$d/de.sock" ]
}
de_card_stop() {
  [ -f "$1/de.pid" ] && { kill "$(cat "$1/de.pid")" 2>/dev/null; sleep 1.2; rm -f "$1/de.pid"; }
  return 0
}

# de_proxy_start D [kitproxy options]: Silo's proxy stand-in on D/proxy.sock (default: any public
# host, as an open main's boundary would; --allow GLOB for a limited one). Writes D/proxy.pid.
de_proxy_start() {
  local d="$1"; shift
  [ $# -gt 0 ] || set -- --allow-public
  rm -f "$d/proxy.sock" "$d/proxy.ready"
  python3 -I "$KIT_ROOT/py/kitproxy.py" --unix "$d/proxy.sock" --log "$d/proxy.jsonl" --ready "$d/proxy.ready" "$@" \
    > "$d/proxy.log" 2>&1 &
  echo $! > "$d/proxy.pid"
  local i=0; while [ ! -f "$d/proxy.ready" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
  [ -f "$d/proxy.ready" ]
}
de_proxy_stop() {
  [ -f "$1/proxy.pid" ] && { kill "$(cat "$1/proxy.pid")" 2>/dev/null; rm -f "$1/proxy.pid"; }
  return 0
}

# de_ssh_config D: D/ssh_config for net_ssh, through D/ssh.sock.
de_ssh_config() {
  : > "$1/known_hosts"
  cat > "$1/ssh_config" <<EOF
Host guest
  HostName guest
  ProxyCommand nc -U "$1/ssh.sock"
  UserKnownHostsFile "$1/known_hosts"
  StrictHostKeyChecking no
  LogLevel ERROR
  ConnectTimeout 10
  ServerAliveInterval 5
  ServerAliveCountMax 3
EOF
}

# de_install_relay BUNDLE [GVD]: on a computer reached over Apple's NAT (vm_ssh), or with GVD over
# gvproxy's forward in that folder (net_ssh), install vsockrelay as two launch daemons and give
# the card its address, as #382's first start does. The address is set last, after a pause,
# since it ends the connection; the caller then stops the computer.
de_install_relay() {
  local b="$1" gvd="${2:-}" bin="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/vsockrelay" script
  [ -x "$bin" ] || { kit_log "vsockrelay is not built: run ./kit build"; return 1; }
  if [ -n "$gvd" ]; then
    net_ssh "$gvd" "$VM_USER" "cat > /tmp/silokit-vsockrelay" < "$bin" || return 1
  else
    vm_ssh "$b" "$VM_USER" "cat > /tmp/silokit-vsockrelay" < "$bin" || return 1
  fi
  script="
mkdir -p /usr/local/libexec && install -m 755 /tmp/silokit-vsockrelay $DE_RELAY_GUEST && rm -f /tmp/silokit-vsockrelay
plist() { # label args...
  l=\$1; shift
  { echo '<?xml version=\"1.0\" encoding=\"UTF-8\"?>'
    echo '<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">'
    echo '<plist version=\"1.0\"><dict><key>Label</key><string>'\$l'</string><key>ProgramArguments</key><array>'
    for a in \"\$@\"; do echo \"<string>\$a</string>\"; done
    echo '</array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/>'
    echo '<key>StandardErrorPath</key><string>/var/log/'\$l'.log</string></dict></plist>'
  } > /Library/LaunchDaemons/\$l.plist
  chmod 644 /Library/LaunchDaemons/\$l.plist
  launchctl bootstrap system /Library/LaunchDaemons/\$l.plist 2>/dev/null || launchctl kickstart -k system/\$l
}
plist com.silokit.relay-in $DE_RELAY_GUEST in --port 22 --to 127.0.0.1:22
plist com.silokit.relay-out $DE_RELAY_GUEST out --listen 127.0.0.1:$DE_PROXY_PORT --port $DE_PROXY_PORT
sleep 1
launchctl print system/com.silokit.relay-in >/dev/null && launchctl print system/com.silokit.relay-out >/dev/null || exit 1
svc=\$(networksetup -listallhardwareports | awk '/^Hardware Port:/{p=substr(\$0,16)} /^Device: en0\$/{print p; exit}')
[ -n \"\$svc\" ] || exit 1
echo \"\$svc\" > /var/db/silokit-netsvc
( sleep 3; networksetup -setmanual \"\$svc\" $DE_ADDR $DE_MASK $DE_ROUTER; networksetup -setdnsservers \"\$svc\" Empty ) >/dev/null 2>&1 &
exit 0
"
  if [ -n "$gvd" ]; then
    vm_password | net_ssh "$gvd" "$VM_USER" "sudo -S -p '' sh -c $(printf '%q' "$script")"
  else
    vm_sudo "$b" "$VM_USER" "$script"
  fi
}

# de_boot BUNDLE D [--no-arp]: start the computer on the card, with the vsock bridges. On Linux:
# the namespace, its card, its SSH server and the two relays. Then wait for SSH through
# D/ssh.sock. Prints the seconds from start to SSH; returns 1 when SSH never answers.
de_boot() {
  local b="$1" d="$2"; shift 2
  de_card_start "$d" "$@" || { kit_log "the card's end did not start: $(cat "$d/de.log")"; return 1; }
  de_ssh_config "$d"
  rm -f "$d/ssh.sock"
  if de_linux; then
    rm -f "$d/guest-ready.json"
    unshare -n python3 -I "$KIT_ROOT/linux/fake-guest-net.py" --sock "$d/de.sock" --mac 5a:94:ef:4b:11:40 \
      --static "$DE_ADDR,$DE_MASK,$DE_ROUTER" --ready "$d/guest-ready.json" > "$d/guest.log" 2>&1 &
    local ns=$!; echo "$ns" > "$d/guest.pid"
    local i=0; while [ ! -f "$d/guest-ready.json" ] && [ "$i" -lt 60 ]; do sleep 0.2; i=$((i + 1)); done
    [ -f "$d/guest-ready.json" ] || { kit_log "the namespace's card did not come up: $(tail -3 "$d/guest.log")"; return 1; }
    if [ -n "${DE_PLANT_LEAK_TO:-}" ]; then
      # linux/test-40.sh's planted fault: a second link out of the namespace, to this machine.
      ip link del kl0 2>/dev/null
      ip link add kl0 type veth peer name kl1 && ip link set kl1 netns "$ns" &&
        ip addr add 10.99.0.1/24 dev kl0 && ip link set kl0 up &&
        nsenter -t "$ns" -n sh -c "ip addr add 10.99.0.2/24 dev kl1 && ip link set kl1 up && ip route add $DE_PLANT_LEAK_TO/32 via 10.99.0.1" ||
        kit_log "the planted leak could not be made"
    fi
    nsenter -t "$ns" -n bash "$KIT_ROOT/linux/sshd-up.sh" "$d/sshd" 127.0.0.1 22 > "$d/sshd.pid" || return 1
    cat "$(_vm_key).pub" >> "$d/sshd/authorized_keys"
    local relay="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/vsockrelay"
    nsenter -t "$ns" -n "$relay" in --unix "$d/ssh.sock" --to 127.0.0.1:22 > "$d/relay-in.log" 2>&1 &
    echo $! > "$d/relay-in.pid"
    nsenter -t "$ns" -n "$relay" out --listen "127.0.0.1:$DE_PROXY_PORT" --unix "$d/proxy.sock" > "$d/relay-out.log" 2>&1 &
    echo $! > "$d/relay-out.pid"
    net_ready "$d" "" root 30
    return
  fi
  vm_start "$b" --net "vfkit:$d/de.sock" --vsock-in "22:$d/ssh.sock" --vsock-out "$DE_PROXY_PORT:$d/proxy.sock" >/dev/null
  net_ready "$d" "$b" "$VM_USER" 600
}

# de_down BUNDLE D: stop the computer (or the namespace) and the card's end; the proxy stays.
de_down() {
  local b="$1" d="$2" p
  if de_linux; then
    for p in relay-in relay-out guest; do
      [ -f "$d/$p.pid" ] && { kill "$(cat "$d/$p.pid")" 2>/dev/null; rm -f "$d/$p.pid"; }
    done
    [ -s "$d/sshd.pid" ] && { kill "$(cat "$d/sshd.pid")" 2>/dev/null; : > "$d/sshd.pid"; }
    [ -n "${DE_PLANT_LEAK_TO:-}" ] && ip link del kl0 2>/dev/null
  elif [ -n "$b" ]; then
    vm_stop "$b" 60 >/dev/null
  fi
  de_card_stop "$d"
}

# de_sh D "script": a POSIX sh script inside, as the user (on Linux, root in the namespace).
de_sh() {
  if de_linux; then
    nsenter -t "$(_de_ns "$1")" -n /bin/sh -c "$2" < /dev/null
  else
    net_ssh "$1" "$VM_USER" "/bin/sh -c $(printf '%q' "$GUEST_PATH $2")" < /dev/null
  fi
}

# de_root D "script": the same as root inside.
de_root() {
  if de_linux; then
    nsenter -t "$(_de_ns "$1")" -n /bin/sh -c "$2" < /dev/null
  else
    vm_password | net_ssh "$1" "$VM_USER" "sudo -S -p '' /bin/sh -c $(printf '%q' "$2")"
  fi
}

# de_proxy_hosts D: the hosts the proxy saw since the log began, one "host:port result" a line.
de_proxy_hosts() {
  python3 -c '
import json, sys
try:
    for line in open(sys.argv[1]):
        e = json.loads(line); print("%s:%s %s" % (e.get("host"), e.get("port"), e.get("result")))
except OSError: pass' "$1/proxy.jsonl"
}
