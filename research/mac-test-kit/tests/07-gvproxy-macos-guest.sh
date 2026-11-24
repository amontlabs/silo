#!/bin/bash
# id: 07
# title: gvproxy with a macOS computer, against Apple's NAT
# tier: vm
# time: about an hour
# needs: the base computer of test 00, a 27 MB download of gvproxy, about 1 GB downloaded inside the computer (Node, git, npm packages), no sudo; you only for the optional sleep step
# decides: network-stack, computers-main-on-a-mac, who-can-call-the-engine
# blocks: option A of the first build (#373): main is on Silo's network stack from its first start, so this runs before main does
# question: Does a macOS computer work on gvproxy (DHCP, MTU, ssh through a Unix socket forward, bursts of connections, git clone and npm install, measured against Apple's NAT), and what of the Mac can it reach through each?
# outcome gvproxy-now: gvproxy works for a macOS computer, within 1.5 times of Apple's NAT, and the hardened configuration keeps the Mac's 127.0.0.1 and gvproxy's API out of its reach. network-stack: take it with gvproxy as
#   the first driver, with research 04's edits (NAT entry removed, API token, ssh through a Unix socket). Read the reach facts: the Mac's LAN address is still reachable through any stack that dials out for the computer, so device-out-of-reach needs a rule in the forwarder.
# outcome gvproxy-slow: gvproxy works but npm install or git clone takes more than 1.5 times as long as on Apple's NAT. network-stack: gvproxy stays the driver only with the MTU raised (silovm needs an MTU option) or
#   linked into the helper; compare the numbers before taking it.
# outcome gvproxy-stalls: bursts of new connections stall (over 5 s) or fail on gvproxy and not on Apple's NAT: the TIME_WAIT bug research 04 names. network-stack: gvproxy needs the gVisor fix (PR 17844) or a
#   workaround before agents run all night on it; the record should carry the stack's own stalls as facts.
# outcome gvproxy-fails: no DHCP lease or no ssh through the forward for a macOS computer. network-stack's fallback: the first build runs main on Apple's NAT and shows that main's network has no boundary.
# outcome hardening-fails: the hardened configuration still lets the computer reach the Mac's 127.0.0.1 or change gvproxy's port forwards. Silo must patch gvproxy (or link it with its own forwarder), not only
#   configure it, before network-stack can say the computer reaches neither the Mac's loopback nor the Mac.
# outcome inconclusive: something needed for the comparison did not run (no NAT phase, no internet inside); the notes say which, and the numbers that did run are in the results.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/net.sh"

W="$KIT_WORK"
SUM="$W/summary.env"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi

# What the computer downloads (pinned; checked 24 November 2026).
NODE_V=v26.13.0
NODE_TGZ="node-$NODE_V-darwin-arm64.tar.gz"
NODE_URL="https://nodejs.org/dist/$NODE_V/$NODE_TGZ"
NODE_SHA=d38bd5e5a2ddce6bbb5ce45e46e208337da4d51fffe611e567fd32073d6439f6
DUGITE=dugite@3.3.2        # GitHub Desktop's git for macOS arm64, checksummed by its installer
GIT_BIG=https://github.com/microsoft/TypeScript   # --depth 1: about 50 MB in one pack
GIT_MANY=https://github.com/expressjs/express     # full history: about 12 MB, many objects
NPM_DEP='"react-scripts":"5.0.1"'                 # about 1,300 packages, 300 MB on disk
CONNS="${KIT_07_CONNS:-2000}"

if [ "$LINUX" = 1 ]; then
  # The Linux analogue: gvproxy's Linux build, a network namespace with a TAP device for the
  # computer (linux/fake-guest-net.py does DHCP and moves frames), and an OpenSSH server in it.
  # It runs the same gvproxy configurations, API calls, forwards, bursts and reach probes; the
  # macOS-only parts (silovm, Apple's NAT, npm and git inside, sleep) are skipped.
  [ "$(id -u)" = 0 ] || kit_skip "the Linux analogue needs root (a network namespace and a TAP device)"
  for c in unshare nsenter curl ssh nc python3; do kit_need_cmd "$c" "install it"; done
  [ -c /dev/net/tun ] || kit_skip "the Linux analogue needs /dev/net/tun"
  command -v sshd >/dev/null 2>&1 || [ -x /usr/sbin/sshd ] || kit_skip "the Linux analogue needs openssh-server"
  unshare -n true 2>/dev/null || kit_skip "the Linux analogue cannot make a network namespace here"
  G_USER=root
else
  kit_need_mac
  kit_need_silovm
  kit_need_base_vm
  kit_need_cmd curl "it ships with macOS"
  kit_need_cmd nc "it ships with macOS"
  kit_need_free_gb 10
  G_USER="$VM_USER"
fi

: > "$SUM"
put() { echo "$1=$2" >> "$SUM"; }
isnum() { python3 -c 'import sys; float(sys.argv[1])' "$1" 2>/dev/null; }
m() { isnum "$2" && kit_measure "$1" "$2" "$3" "${4:-}"; }
jget() { python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get(sys.argv[2], ""))' "$1" "$2" 2>/dev/null; }
put platform "$([ "$LINUX" = 1 ] && echo linux || echo mac)"

if net_gvproxy_present; then
  kit_log "gvproxy $NET_GVPROXY_TAG is already downloaded and checked"
else
  kit_confirm "Download gvproxy $NET_GVPROXY_TAG (about 27 MB) from github.com/containers/gvisor-tap-vsock into ${NET_GVPROXY%/*} and check its sha256 against the release."
  net_gvproxy_get || kit_fail "could not get a gvproxy whose sha256 matches the release"
fi
kit_fact gvproxy_version "$("$NET_GVPROXY" -version 2>&1 | head -1)"
if [ "$LINUX" = 0 ]; then
  kit_fact gvproxy_signature "$(codesign -dv "$NET_GVPROXY" 2>&1 | grep -E '^(Signature|TeamIdentifier|Identifier)' | tr '\n' ' ')"
  kit_confirm "Start one macOS computer three times (gvproxy hardened, gvproxy as it comes, Apple's NAT), about an hour in all; inside it download Node $NODE_V, a git build, two git repositories and about 1,300 npm packages, twice (about 1 GB). On your Mac, a small stand-in server listens on all its network addresses while the test runs, to see what the computer reaches (macOS may ask whether python3 may accept incoming connections)."
fi

# --- the computer --------------------------------------------------------------------------
B=""; GVPID=""; NETDIR=""; CUR_MAC=""; STANDIN_PID=""
GH="$W/gh"; GD="$W/gd"   # short: a Unix socket's path must stay under 104 bytes
mkdir -p "$GH" "$GD"; chmod 700 "$W" "$GH" "$GD"

cleanup() {
  [ -n "$NETDIR" ] && c_stop_gv "$NETDIR"
  net_gvproxy_stop "$GH"; net_gvproxy_stop "$GD"
  [ -n "$STANDIN_PID" ] && kill "$STANDIN_PID" 2>/dev/null
  rm -f "$W/planted.sock" "$W/standin/engine.sock"
  [ -n "$B" ] && vm_destroy "$B"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

lin_stop() {
  local d="$1"
  [ -f "$d/sshd.pid" ] && kill "$(cat "$d/sshd.pid")" 2>/dev/null
  [ -f "$d/guest.pid" ] && kill "$(cat "$d/guest.pid")" 2>/dev/null
  rm -f "$d/sshd.pid" "$d/guest.pid"
}

# c_boot_gv D MODE [provision]: gvproxy with MODE's configuration, then the computer on it.
c_boot_gv() {
  local d="$1" mode="$2"
  if [ "$LINUX" = 1 ]; then CUR_MAC="5a:94:ef:4b:11:07"; else CUR_MAC="$(net_bundle_mac "$B")"; fi
  net_gvproxy_config "$d" "$mode" "$CUR_MAC"
  GVPID="$(net_gvproxy_start "$d")" || { kit_note "gvproxy did not start: $(tail -3 "$d/gvproxy.out" 2>/dev/null)"; return 1; }
  NETDIR="$d"
  if [ "$LINUX" = 1 ]; then
    rm -f "$d/guest-ready.json"
    unshare -n python3 -I "$KIT_ROOT/linux/fake-guest-net.py" --sock "$d/gv.sock" --mac "$CUR_MAC" \
      --ready "$d/guest-ready.json" > "$d/guest.log" 2>&1 &
    echo $! > "$d/guest.pid"
    local i=0
    while [ ! -f "$d/guest-ready.json" ] && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
    [ -f "$d/guest-ready.json" ] || { kit_note "fake guest: $(tail -3 "$d/guest.log")"; return 1; }
    nsenter -t "$(cat "$d/guest.pid")" -n bash "$KIT_ROOT/linux/sshd-up.sh" "$d/sshd" 0.0.0.0 22 > "$d/sshd.pid" || return 1
    cat "$(_vm_key).pub" >> "$d/sshd/authorized_keys"
  elif [ "${3:-}" = provision ]; then
    # shellcheck disable=SC2046
    vm_start "$B" $(vm_provision_args) --net "vfkit:$d/gv.sock" >/dev/null
  else
    vm_start "$B" --net "vfkit:$d/gv.sock" >/dev/null
  fi
}

c_stop_gv() {
  local d="$1"
  if [ "$LINUX" = 1 ]; then lin_stop "$d"; else vm_running "$B" && kit_note "stopped: $(vm_stop "$B" 60)"; fi
  cp "$d/gvproxy.log" "$W/gvproxy-$(basename "$d").log" 2>/dev/null && kit_attach "$W/gvproxy-$(basename "$d").log"
  net_gvproxy_stop "$d"
  NETDIR=""; GVPID=""
}

# g_raw cmd: one command inside, with stdin attached (for copies), over the current way in: the
# Unix socket forward on gvproxy, the address on Apple's NAT (NAT_IP, looked up once) otherwise.
NAT_IP=""
g_raw() {
  if [ -n "$NETDIR" ]; then
    net_ssh "$NETDIR" "$G_USER" "$1"
  else
    # shellcheck disable=SC2046
    ssh $(_vm_ssh_opts) -o BatchMode=yes -i "$(_vm_key)" "$G_USER@$NAT_IP" "$1"
  fi
}
# g_sh "script": run a POSIX sh script inside, as the computer's user.
g_sh() { g_raw "/bin/sh -c $(printf '%q' "$1")" < /dev/null; }

# --- the stand-in engine on the Mac ----------------------------------------------------------
mkdir -p "$W/standin"; chmod 700 "$W/standin"
rm -f "$W/standin/standin.json"
python3 -I "$KIT_ROOT/py/07-standin.py" --dir "$W/standin" > "$W/standin/out.log" 2>&1 &
STANDIN_PID=$!
i=0; while [ ! -f "$W/standin/standin.json" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
[ -f "$W/standin/standin.json" ] || kit_fail "the stand-in engine did not start: $(cat "$W/standin/out.log")"
SJ="$(cat "$W/standin/standin.json")"
P_LO="$(jget "$SJ" loopback_port)"; P_ANY="$(jget "$SJ" any_port)"
LAN="$(net_my_lan_ip)"
kit_note "stand-in engine: 127.0.0.1:$P_LO, 0.0.0.0:$P_ANY and $W/standin/engine.sock (0600); the Mac's address on its default route: ${LAN:-none}"
if [ "$LINUX" = 0 ]; then
  kit_fact mac_firewall "$(/usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate 2>/dev/null | tr -d '\n')"
  kit_note "If macOS asks whether python3 may accept incoming connections, either answer works: the test records what reached it."
fi

# --- measurements, each for the current way in (LABEL is gh, gd or nat) ----------------------

m_guest_net() {
  local l="$1" out
  out="$(g_sh 'if [ "$(uname)" = Darwin ]; then
  ifc=$(route -n get default 2>/dev/null | awk "/interface:/{print \$2}")
  echo "if=$ifc"
  ifconfig "$ifc" | sed -n "s/.*mtu \([0-9]*\).*/mtu=\1/p" | head -1
  echo "addr=$(ipconfig getifaddr "$ifc")"
  echo "gw=$(route -n get default | awk "/gateway:/{print \$2}")"
  echo "dhcp_mtu=$(ipconfig getoption "$ifc" 26 2>/dev/null)"
  { ifconfig -a; ipconfig getpacket "$ifc"; scutil --dns | head -40; netstat -rn -f inet; } > /tmp/silokit-net.txt 2>&1
else
  ifc=$(ip route show default | awk "{print \$5; exit}")
  echo "if=$ifc"
  echo "mtu=$(ip -o link show "$ifc" | sed -n "s/.* mtu \([0-9]*\).*/\1/p")"
  echo "addr=$(ip -4 -o addr show "$ifc" | awk "{print \$4}" | cut -d/ -f1)"
  echo "gw=$(ip route show default | awk "{print \$3; exit}")"
  echo "dhcp_mtu="
  { ip addr; ip route; cat /etc/resolv.conf; } > /tmp/silokit-net.txt 2>&1
fi')" || { kit_note "$l: could not read the computer's network settings"; return 1; }
  g_sh 'cat /tmp/silokit-net.txt; rm -f /tmp/silokit-net.txt' > "$W/net-$l.txt" 2>/dev/null && kit_attach "$W/net-$l.txt"
  G_IF="$(echo "$out" | sed -n 's/^if=//p')"
  G_MTU="$(echo "$out" | sed -n 's/^mtu=//p')"
  G_ADDR="$(echo "$out" | sed -n 's/^addr=//p')"
  G_GW="$(echo "$out" | sed -n 's/^gw=//p')"
  G_DHCP_MTU="$(echo "$out" | sed -n 's/^dhcp_mtu=//p')"
  kit_fact "$l.interface" "$G_IF address $G_ADDR gateway $G_GW mtu $G_MTU dhcp-option-26 ${G_DHCP_MTU:-none}"
  m "$l.mtu" "$G_MTU" bytes "the MTU inside the computer"
  put "${l}_mtu" "$G_MTU"; put "${l}_dhcp_mtu" "${G_DHCP_MTU:-none}"; put "${l}_gw" "$G_GW"
  kit_fact "$l.https_github" "$(g_sh "curl -s -o /dev/null -m 20 -w 'HTTP %{http_code}, name lookup %{time_namelookup} s, total %{time_total} s' https://github.com/" 2>/dev/null)"
}

# m_reach LABEL "name:host:port ...": which stand-in listeners a connection from inside reaches.
m_reach() {
  local l="$1" t out name res rc
  out="$(g_sh "for t in $2; do
  h=\$(echo \"\$t\" | cut -d: -f2); p=\$(echo \"\$t\" | cut -d: -f3); n=\$(echo \"\$t\" | cut -d: -f1)
  o=\$(curl -s -m 4 \"http://\$h:\$p/\" 2>/dev/null); rc=\$?
  case \"\$o\" in *silo-kit-standin*) echo \"\$n reached \$rc\" ;; *) echo \"\$n no \$rc\" ;; esac
done")" || { kit_note "$l: the reach probe did not run"; return 1; }
  echo "$out" | while read -r name res rc; do
    [ -n "$name" ] || continue
    kit_fact "$l.reach.$name" "$res (curl exit $rc)"
  done
  for t in $2; do
    name="${t%%:*}"
    res="$(echo "$out" | awk -v n="$name" '$1 == n {print $2}')"
    put "${l}_$name" "$res"
  done
}

# m_api_from_guest LABEL: can the computer use gvproxy's API at 192.168.127.1:80 to open a
# listener on the Mac (a Unix socket in the work folder, a TCP port on 127.0.0.1)?
m_api_from_guest() {
  local l="$1" d="$NETDIR" px out code_all code_unix code_tcp code_engine planted=no port=no
  px="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
  rm -f "$W/planted.sock"
  out="$(g_sh "a=\$(curl -s -m 4 -o /dev/null -w '%{http_code}' http://$NET_GATEWAY/services/forwarder/all)
u=\$(curl -s -m 4 -o /dev/null -w '%{http_code}' -X POST -d '{\"local\":\"$W/planted.sock\",\"protocol\":\"unix\",\"remote\":\"tcp://$NET_GUEST_IP:22\"}' http://$NET_GATEWAY/services/forwarder/expose)
t=\$(curl -s -m 4 -o /dev/null -w '%{http_code}' -X POST -d '{\"local\":\"127.0.0.1:$px\",\"protocol\":\"tcp\",\"remote\":\":22\"}' http://$NET_GATEWAY/services/forwarder/expose)
e=\$(curl -s -m 4 -o /dev/null -w '%{http_code}' -X POST -d '{\"local\":\"$W/standin/engine.sock\",\"protocol\":\"unix\",\"remote\":\"tcp://$NET_GUEST_IP:22\"}' http://$NET_GATEWAY/services/forwarder/expose)
echo \"\$a \$u \$t \$e\"")"
  read -r code_all code_unix code_tcp code_engine <<EOF
$out
EOF
  sleep 1
  [ -S "$W/planted.sock" ] && planted=yes
  if python3 -c '
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=5)
sys.exit(0 if s.recv(64).startswith(b"SSH-") else 1)' "$px" 2>/dev/null; then port=yes; fi
  kit_fact "$l.api_from_guest" "GET /services/forwarder/all: $code_all; expose a Unix socket on the Mac: $code_unix (socket made: $planted); expose 127.0.0.1:$px on the Mac: $code_tcp (it answered with the computer's SSH banner: $port); expose over the stand-in engine's socket: $code_engine"
  put "${l}_api_code" "$code_all"
  if [ "$planted" = yes ] || [ "$port" = yes ]; then put "${l}_api_expose" opened; else put "${l}_api_expose" refused; fi
  # Undo whatever the computer opened, through the Mac's side of the API.
  net_api "$d" /services/forwarder/unexpose "{\"local\":\"$W/planted.sock\",\"protocol\":\"unix\"}" >/dev/null
  net_api "$d" /services/forwarder/unexpose "{\"local\":\"127.0.0.1:$px\",\"protocol\":\"tcp\"}" >/dev/null
  rm -f "$W/planted.sock"
  [ -S "$W/standin/engine.sock" ] || kit_note "$l: the stand-in engine's socket is gone after the computer's call"
}

# m_conns LABEL: CONNS new connections to sshd inside, one at a time, then 8 at a time.
m_conns() {
  local l="$1" tgt r p
  if [ -n "$NETDIR" ]; then tgt="--unix $NETDIR/ssh.sock"; else tgt="--tcp $NAT_IP:22"; fi
  for p in 1 8; do
    # shellcheck disable=SC2086
    r="$(python3 -I "$KIT_ROOT/py/07-conns.py" $tgt --count "$CONNS" --parallel "$p" --wait 30)" || { kit_note "$l: 07-conns failed"; continue; }
    kit_fact "$l.conns_p$p" "$r"
    m "$l.conns_p$p.seconds" "$(jget "$r" seconds)" s "$CONNS connections to sshd, $p at a time"
    m "$l.conns_p$p.failed" "$(jget "$r" failed)" connections
    m "$l.conns_p$p.over_5s" "$(jget "$r" over_5s)" connections
    m "$l.conns_p$p.p99" "$(jget "$r" p99_ms)" ms
    put "${l}_c${p}_count" "$CONNS"; put "${l}_c${p}_s" "$(jget "$r" seconds)"
    put "${l}_c${p}_failed" "$(jget "$r" failed)"; put "${l}_c${p}_over5" "$(jget "$r" over_5s)"
  done
}

# m_ssh_latency LABEL: 20 full `ssh ... true`, median in ms.
m_ssh_latency() {
  local l="$1" i=0 t0 t1 ts="" ms
  while [ "$i" -lt 20 ]; do
    t0="$(kit_now)"; g_raw true < /dev/null >/dev/null 2>&1; t1="$(kit_now)"
    ts="$ts $(python3 -c "print(($t1 - $t0) * 1000)")"; i=$((i + 1))
  done
  ms="$(python3 -c 'import statistics,sys; print("%.0f" % statistics.median([float(x) for x in sys.argv[1:]]))' $ts)"
  m "$l.ssh_true_median" "$ms" ms "20 times ssh true"
  put "${l}_ssh_ms" "$ms"
}

# m_throughput LABEL: 1 GiB over ssh each way.
m_throughput() {
  local l="$1" up down c0 c1 t0 t1
  [ -n "$GVPID" ] && c0="$(net_proc_sample "$GVPID")"
  t0="$(kit_now)"; dd if=/dev/zero bs=1048576 count=1024 2>/dev/null | g_raw 'cat > /dev/null'; t1="$(kit_now)"
  up="$(python3 -c "print('%.3f' % (8 * 2**30 / 1e9 / ($t1 - $t0)))")"
  t0="$(kit_now)"; g_raw 'dd if=/dev/zero bs=1048576 count=1024 2>/dev/null' < /dev/null > /dev/null; t1="$(kit_now)"
  down="$(python3 -c "print('%.3f' % (8 * 2**30 / 1e9 / ($t1 - $t0)))")"
  m "$l.ssh_copy_to_computer" "$up" Gbit/s "1 GiB through ssh"
  m "$l.ssh_copy_from_computer" "$down" Gbit/s "1 GiB through ssh"
  put "${l}_up_gbit" "$up"; put "${l}_down_gbit" "$down"
  if [ -n "$GVPID" ]; then
    c1="$(net_proc_sample "$GVPID")"
    m "$l.gvproxy_cpu_during_copies" "$(python3 -c "print('%.1f' % (${c1#* } - ${c0#* }))")" "cpu-seconds" "for 2 GiB"
  fi
}

# m_outbound LABEL HOST PORT: CONNS new connections from inside to the stand-in, one at a time,
# then 8 loops at once. Each is a curl, so this is also what a script's many short requests see.
m_outbound() {
  local l="$1" h="$2" p="$3" per out
  per=$((CONNS / 8))
  out="$(g_sh "rm -f /tmp/silokit-out.*; i=0; while [ \$i -lt $CONNS ]; do curl -s -o /dev/null -m 15 -w '%{time_total}\n' http://$h:$p/ >> /tmp/silokit-out.1 || echo fail >> /tmp/silokit-out.1; i=\$((i+1)); done
k=0; while [ \$k -lt 8 ]; do ( j=0; while [ \$j -lt $per ]; do curl -s -o /dev/null -m 15 -w '%{time_total}\n' http://$h:$p/ >> /tmp/silokit-out.p\$k || echo fail >> /tmp/silokit-out.p\$k; j=\$((j+1)); done ) & k=\$((k+1)); done; wait
cat /tmp/silokit-out.1 | awk '/fail/{f++; next} {n++; if (\$1>1) s++; if (\$1>m) m=\$1} END {printf \"%d %d %d %.3f\n\", n+f, f+0, s+0, m}'
cat /tmp/silokit-out.p* | awk '/fail/{f++; next} {n++; if (\$1>1) s++; if (\$1>m) m=\$1} END {printf \"%d %d %d %.3f\n\", n+f, f+0, s+0, m}'
rm -f /tmp/silokit-out.*")" || { kit_note "$l: the outbound burst did not run"; return 1; }
  local seq par
  seq="$(echo "$out" | sed -n 1p)"; par="$(echo "$out" | sed -n 2p)"
  kit_fact "$l.outbound_sequential" "count failed over_1s max_s: $seq"
  kit_fact "$l.outbound_parallel8" "count failed over_1s max_s: $par"
  set -- $seq
  put "${l}_out_count" "$1"; put "${l}_out_failed" "$2"; put "${l}_out_over1" "$3"; put "${l}_out_max" "$4"
  m "$l.outbound_failed" "$2" connections "of $1 sequential"
  m "$l.outbound_max" "$4" s "longest of $1 sequential"
  set -- $par
  m "$l.outbound_parallel_failed" "$2" connections "of $1, 8 at a time"
  m "$l.outbound_parallel_max" "$4" s "longest of $1, 8 at a time"
  put "${l}_outp_failed" "$2"; put "${l}_outp_over1" "$3"
}

# m_internet_setup: Node and a git build inside the computer (once; they stay on its disk).
m_internet_setup() {
  g_sh "set -e; mkdir -p ~/silokit; cd ~/silokit
[ -x node/bin/node ] || { curl -fsSL -o node.tgz $NODE_URL; shasum -a 256 node.tgz | grep -q $NODE_SHA || { echo node.tgz: sha256 mismatch; exit 1; }; tar xzf node.tgz; rm -rf node; mv node-$NODE_V-darwin-arm64 node; rm -f node.tgz; }
export PATH=\$HOME/silokit/node/bin:\$PATH
mkdir -p dug; cd dug; [ -f package.json ] || echo '{\"private\":true}' > package.json
[ -d node_modules/dugite/git ] || npm install --no-audit --no-fund --loglevel=error $DUGITE >/dev/null
cat > clone.js <<'EOF'
const { exec } = require(process.env.HOME + '/silokit/dug/node_modules/dugite');
const t0 = Date.now();
exec(process.argv.slice(2), process.env.HOME + '/silokit').then(r => {
  console.log(JSON.stringify({ exitCode: r.exitCode, seconds: (Date.now() - t0) / 1000, err: r.exitCode ? String(r.stderr).slice(-300) : '' }));
  process.exit(r.exitCode);
});
EOF
node --version"
}

# m_internet LABEL: the same downloads, clones and install through the current network.
m_internet() {
  local l="$1" r s1 s2 t0 t1 n
  r="$(g_sh "curl -s -o /dev/null -m 300 -w '%{time_total} %{speed_download}' $NODE_URL")" || r=""
  m "$l.download_node_s" "${r%% *}" s "$NODE_TGZ from nodejs.org"
  m "$l.download_node_speed" "$(python3 -c "print('%.1f' % (float('${r#* }' or 0) * 8 / 1e6))" 2>/dev/null)" Mbit/s
  r="$(g_sh "export PATH=\$HOME/silokit/node/bin:\$PATH; cd ~/silokit; rm -rf c1; node dug/clone.js clone --depth 1 $GIT_BIG c1; rm -rf c1")"
  s1="$(jget "$r" seconds)"; kit_fact "$l.git_clone_big" "$r"
  m "$l.git_clone_depth1_typescript" "$s1" s "$GIT_BIG --depth 1"
  r="$(g_sh "export PATH=\$HOME/silokit/node/bin:\$PATH; cd ~/silokit; rm -rf c2; node dug/clone.js clone $GIT_MANY c2; rm -rf c2")"
  s2="$(jget "$r" seconds)"; kit_fact "$l.git_clone_many" "$r"
  m "$l.git_clone_full_express" "$s2" s "$GIT_MANY, full history"
  isnum "$s1" && isnum "$s2" && put "${l}_git_s" "$(python3 -c "print($s1 + $s2)")"
  local c0 c1
  [ -n "$GVPID" ] && c0="$(net_proc_sample "$GVPID")"
  t0="$(kit_now)"
  n="$(g_sh "export PATH=\$HOME/silokit/node/bin:\$PATH; rm -rf ~/silokit/np; mkdir -p ~/silokit/np; cd ~/silokit/np
echo '{\"name\":\"silo-kit-npm\",\"version\":\"1.0.0\",\"private\":true,\"dependencies\":{$NPM_DEP}}' > package.json
npm install --no-audit --no-fund --cache ~/silokit/np/cache --loglevel=error >/tmp/silokit-npm.log 2>&1 || { tail -5 /tmp/silokit-npm.log; exit 1; }
ls node_modules | wc -l | tr -d ' '
cd ~; rm -rf ~/silokit/np")" || { kit_note "$l: npm install failed: $n"; n=""; }
  t1="$(kit_now)"
  if [ -n "$n" ]; then
    s1="$(python3 -c "print('%.1f' % ($t1 - $t0))")"
    m "$l.npm_install" "$s1" s "react-scripts 5.0.1, empty cache, $n folders in node_modules"
    put "${l}_npm_s" "$s1"
  fi
  if [ -n "$GVPID" ]; then
    c1="$(net_proc_sample "$GVPID")"
    put "${l}_cpu_npm_pct" "$(python3 -c "print('%.0f' % (100 * (${c1#* } - ${c0#* }) / ($t1 - $t0)))")"
    m "$l.gvproxy_cpu_during_npm" "$(python3 -c "print('%.1f' % (100 * (${c1#* } - ${c0#* }) / ($t1 - $t0)))")" "% of a core"
    m "$l.gvproxy_rss_after_npm" "${c1%% *}" KB
  fi
}

# m_idle_cpu LABEL SECONDS: gvproxy's processor use while the computer is idle.
m_idle_cpu() {
  local l="$1" s="$2" c0 c1 pct
  [ -n "$GVPID" ] || return 0
  c0="$(net_proc_sample "$GVPID")"; sleep "$s"; c1="$(net_proc_sample "$GVPID")"
  pct="$(python3 -c "print('%.2f' % (100 * (${c1#* } - ${c0#* }) / $s))")"
  echo "$pct"
}

# --- phase 1: gvproxy, hardened (first start, with provisioning) -----------------------------
if [ "$LINUX" = 0 ]; then
  B="$(vm_fresh n07)" || exit 1  # vm_fresh already recorded why
fi
kit_log "phase 1: gvproxy hardened (no NAT to 127.0.0.1, API token, no port on the Mac)"
if c_boot_gv "$GH" hardened provision; then
  t_start="$(kit_now)"
  if ip="$(net_wait_lease "$GH" "$CUR_MAC" "$([ "$LINUX" = 1 ] && echo 30 || echo 900)")"; then
    put gh_lease "$ip"
    if [ "$LINUX" = 0 ]; then
      dhcp_s="$(python3 - "$B/run.jsonl" "$(kit_now)" <<'EOF'
import json, sys
t = None
for line in open(sys.argv[1]):
    try: e = json.loads(line)
    except ValueError: continue
    if e.get("event") == "started": t = e.get("t")
print("%.1f" % (float(sys.argv[2]) - t) if t else "nan")
EOF
)"
    else
      dhcp_s="$(python3 -c "print('%.1f' % ($(kit_now) - $t_start))")"
    fi
    m gh.dhcp_lease_after_start "$dhcp_s" s "lease seen in gvproxy's API, counted from the started event"
    put gh_dhcp_s "$dhcp_s"
    kit_fact gh.lease "$ip for $CUR_MAC"
    code="$(net_expose_ssh "$GH" "$ip")"
    kit_fact gh.expose_ssh "HTTP $code, socket $(ls -l "$GH/ssh.sock" 2>/dev/null | awk '{print $1}')"
    if s="$(net_ready "$GH" "$B" "$G_USER" "$([ "$LINUX" = 1 ] && echo 60 || echo 900)")"; then
      put gh_ssh ok; put gh_ssh_ready_s "$s"
      m gh.ssh_ready_after_start "$s" s "through the Unix socket forward"
      m_guest_net gh
      pct="$(m_idle_cpu gh 60)"; m gh.gvproxy_idle_cpu "$pct" "% of a core" "60 s, computer idle"; put gh_cpu_idle_pct "$pct"
      smp="$(net_proc_sample "$GVPID")"; m gh.gvproxy_rss_idle "${smp%% *}" KB
      m_ssh_latency gh
      m_conns gh
      m_throughput gh
      m_reach gh "alias_loopback:$NET_HOST_ALIAS:$P_LO alias_any:$NET_HOST_ALIAS:$P_ANY gateway_loopback:$NET_GATEWAY:$P_LO gateway_any:$NET_GATEWAY:$P_ANY${LAN:+ lan_loopback:$LAN:$P_LO lan_any:$LAN:$P_ANY}"
      m_api_from_guest gh
      [ -n "$LAN" ] && m_outbound gh "$LAN" "$P_ANY"
      if [ "$LINUX" = 0 ]; then
        if m_internet_setup > "$W/setup.out" 2>&1; then
          kit_fact gh.node_inside "$(tail -1 "$W/setup.out")"
          m_internet gh
        else
          kit_note "Node or git could not be set up inside: $(tail -3 "$W/setup.out")"
        fi
        smp="$(net_proc_sample "$GVPID")"; put gh_rss_mb "$(python3 -c "print('%.0f' % (${smp%% *} / 1024))")"
        # The optional sleep step: only with someone answering, never with --yes.
        if [ "${KIT_YES:-0}" != 1 ] && [ -t 0 ] && [ "$(kit_ask_yn "Optional: let the Mac sleep for a minute to measure gvproxy after a wake (the computer keeps running)?")" = yes ]; then
          kit_human "Put the Mac to sleep now (close the lid, or Apple menu > Sleep). Wait about a minute, wake it, log in, then come back here and press Enter."
          kit_fact gh.sleep_log "$(pmset -g log 2>/dev/null | grep -E ' (Sleep|Wake) ' | tail -2 | tr -s ' ' | cut -c1-200 | tr '\n' '|')"
          pct="$(m_idle_cpu gh 30)"; m gh.gvproxy_cpu_after_sleep "$pct" "% of a core" "30 s after the wake"; put gh_cpu_after_sleep_pct "$pct"
          if net_limit 30 net_ssh "$GH" "$G_USER" true < /dev/null; then kit_fact gh.ssh_after_sleep ok; else kit_fact gh.ssh_after_sleep failed; fi
        else
          kit_note "the sleep step was skipped (it needs a person, and no --yes)"
        fi
      fi
    else
      put gh_ssh failed; put gh_fail "ssh through the forward never answered"
      kit_note "ssh through $GH/ssh.sock did not answer"
    fi
  else
    put gh_fail "no DHCP lease from gvproxy"
    kit_note "no DHCP lease for $CUR_MAC from gvproxy; leases: $(net_api "$GH" /services/dhcp/leases >/dev/null; cat "$GH/api.body" 2>/dev/null)"
  fi
  c_stop_gv "$GH"
  [ "$LINUX" = 0 ] && cp "$B/run.log" "$W/silovm-run-gh.log" && kit_attach "$W/silovm-run-gh.log"
else
  put gh_fail "gvproxy or the computer did not start"
fi

# --- phase 2: gvproxy as it comes (reach only) ------------------------------------------------
if grep -q '^gh_ssh=ok' "$SUM"; then
  kit_log "phase 2: gvproxy as it comes (192.168.127.254 is the Mac's 127.0.0.1, no API token)"
  if c_boot_gv "$GD" default && ip="$(net_wait_lease "$GD" "$CUR_MAC" 600)" \
     && [ "$(net_expose_ssh "$GD" "$ip")" = 200 ] && net_ready "$GD" "$B" "$G_USER" 600 >/dev/null; then
    m_reach gd "alias_loopback:$NET_HOST_ALIAS:$P_LO alias_any:$NET_HOST_ALIAS:$P_ANY"
    m_api_from_guest gd
  else
    kit_note "phase 2 did not reach ssh; no facts for gvproxy as it comes"
  fi
  c_stop_gv "$GD"
fi

# --- phase 3: Apple's NAT ---------------------------------------------------------------------
if [ "$LINUX" = 0 ]; then
  kit_log "phase 3: Apple's NAT"
  mkdir -p "$KIT_SSH_DIR" && : > "$KIT_SSH_DIR/known_hosts"
  if grep -q '^gh_ssh=ok' "$SUM"; then
    vm_start "$B" --net nat >/dev/null
  else
    # shellcheck disable=SC2046
    vm_start "$B" $(vm_provision_args) --continue-on-provisioning-error --net nat >/dev/null
  fi
  if s="$(vm_wait_ssh "$B" "$G_USER" 900)"; then
    vm_setup_key "$B" "$G_USER" >/dev/null 2>&1
    NAT_IP="$(vm_ip "$B" 30)"
    kit_fact nat.address "$NAT_IP"
    put nat_ssh ok
    m nat.ssh_ready_after_start "$s" s
    m_guest_net nat
    m_ssh_latency nat
    m_conns nat
    m_throughput nat
    m_reach nat "bridge_loopback:$G_GW:$P_LO bridge_any:$G_GW:$P_ANY${LAN:+ lan_loopback:$LAN:$P_LO lan_any:$LAN:$P_ANY}"
    [ -n "$LAN" ] && m_outbound nat "$LAN" "$P_ANY"
    if m_internet_setup > "$W/setup-nat.out" 2>&1; then m_internet nat; else kit_note "Node or git could not be set up inside on NAT: $(tail -3 "$W/setup-nat.out")"; fi
  else
    kit_note "the computer did not answer SSH on Apple's NAT"
  fi
  kit_note "stopped: $(vm_stop "$B" 60)"
  cp "$B/run.log" "$W/silovm-run-nat.log" && kit_attach "$W/silovm-run-nat.log"
  kit_note "Order: gvproxy ran first, so npm's and GitHub's caches were warmer for NAT; a small lead for NAT is expected from that alone."
fi

# Who reached the stand-in, and from which address (through gvproxy, the Mac's own).
python3 -c '
import json, sys
seen = {}
for line in open(sys.argv[1]):
    try: e = json.loads(line)
    except ValueError: continue
    k = "%s from %s" % (e["listener"], e["peer"][0] if isinstance(e["peer"], list) else e["peer"])
    seen[k] = seen.get(k, 0) + 1
print("; ".join("%s: %d" % kv for kv in sorted(seen.items())))' "$W/standin/standin.log" > "$W/standin-peers.txt" 2>/dev/null
kit_fact standin_peers "$(cat "$W/standin-peers.txt")"
kit_attach "$SUM"

res="$(python3 -I "$KIT_ROOT/py/07-decide.py" "$SUM")"
kit_outcome "$(echo "$res" | sed -n 1p)" "$(echo "$res" | sed -n 2p)"
