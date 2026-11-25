#!/bin/bash
# id: 35
# title: #363's sealed gvproxy with a macOS computer: what root inside reaches
# tier: vm
# time: about 30 minutes, plus building gvproxy once (Go 1.26, about 65 MB, and gvproxy's modules)
# needs: the base computer of test 00, git (the Command Line Tools), a 65 MB download of Go unless Go 1.26.0 is installed, no sudo on the Mac
# step: 2
# blocks: option A of the first build (#373): main is on Silo's network stack from its first start, and #363 recommends the sealed gvproxy as that stack until Silo writes its own
# decides: network-stack (the sealed gvproxy as main's stack: #363's recommendation, #370's condition for a closed network and for "never your Mac, your local network"), main-on-macos-first (option A's stack)
# question: With #363's 64-line patch (-sealed), does gvproxy give a macOS computer what it needs (a lease, DNS, the internet, Silo's own SSH forward) while nothing inside, root included, reaches the Mac (its loopback, its own addresses, gvproxy's API, the engine's socket) or the Mac's local network?
# outcome sealed-holds: The computer got its lease, DNS, the internet and Silo's SSH forward, and root inside reached nothing of the Mac or its
#   network: no loopback through 192.168.127.254, no port of the gateway but DNS, no API, no service on the Mac's own address, no device on
#   the Mac's network. network-stack: the sealed gvproxy can carry main from its first start (option A), and the first run can say "never
#   your Mac, your local network" (#370 finding 3) without Apple's NAT.
# outcome sealed-leaks: Root inside reached something of the Mac or its local network (named in the sentence): the patch misses a path on
#   macOS; the sealed gvproxy cannot carry main until it is fixed, and the facts say what to add.
# outcome sealed-breaks: The sealed build gave no lease, no DNS, no internet or no SSH forward, while the same computer has them on gvproxy as
#   released (test 07): the patch refuses something the computer needs; the sentence says what.
# outcome inconclusive: The sealed gvproxy did not build, or the computer did not start on it; the sentence says why.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/net.sh"

W="$KIT_WORK"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi

if [ "$LINUX" = 1 ]; then
  # The Linux analogue, as test 07's: the sealed build of gvproxy's Linux code, a network
  # namespace with a TAP device for the computer (linux/fake-guest-net.py), an OpenSSH server in
  # it. It runs the same probes as root; Apple's NAT and the macOS computer are left out.
  [ "$(id -u)" = 0 ] || kit_skip "the Linux analogue needs root (a network namespace and a TAP device)"
  for c in unshare nsenter curl ssh nc python3 git; do kit_need_cmd "$c" "install it"; done
  [ -c /dev/net/tun ] || kit_skip "the Linux analogue needs /dev/net/tun"
  command -v sshd >/dev/null 2>&1 || [ -x /usr/sbin/sshd ] || kit_skip "the Linux analogue needs openssh-server"
  unshare -n true 2>/dev/null || kit_skip "the Linux analogue cannot make a network namespace here"
  G_USER=root
else
  kit_need_mac
  kit_need_silovm
  kit_need_base_vm
  kit_need_cmd git "xcode-select --install"
  kit_need_free_gb 10
  G_USER="$VM_USER"
fi

if net_sealed_present; then
  kit_log "the sealed gvproxy is already built: $NET_GVPROXY_SEALED"
else
  kit_confirm "Build #363's sealed gvproxy: clone gvisor-tap-vsock $NET_GVPROXY_TAG from GitHub, apply net/gvproxy-sealed.patch and build it with Go $NET_GO_VERSION (downloaded from go.dev, about 65 MB, and checked, unless that Go is installed), into ${NET_GVPROXY_SEALED%/*}."
  if ! net_sealed_build > "$W/build.log" 2>&1; then
    kit_attach "$W/build.log"
    kit_outcome inconclusive "the sealed gvproxy did not build: $(tail -3 "$W/build.log" | tr '\n' ' ')"
    exit 0
  fi
fi
kit_fact gvproxy_sealed "$("$NET_GVPROXY_SEALED" -version 2>&1 | head -1), $NET_GVPROXY_TAG ($NET_GVPROXY_COMMIT) with net/gvproxy-sealed.patch $(net_sha256 "$KIT_ROOT/net/gvproxy-sealed.patch" | cut -c1-12)"

D="$W/gs"; mkdir -p "$D"; chmod 700 "$W" "$D"
B=""; STANDIN_PID=""; GVPID=""
cleanup() {
  if [ "$LINUX" = 1 ]; then
    [ -f "$D/sshd.pid" ] && kill "$(cat "$D/sshd.pid")" 2>/dev/null
    [ -f "$D/guest.pid" ] && kill "$(cat "$D/guest.pid")" 2>/dev/null
  fi
  net_gvproxy_stop "$D"
  [ -n "$STANDIN_PID" ] && kill "$STANDIN_PID" 2>/dev/null
  [ -n "$B" ] && vm_destroy "$B"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

# The stand-in engine on the Mac (test 07's): 127.0.0.1, every address, and a Unix socket.
mkdir -p "$W/standin"; chmod 700 "$W/standin"; rm -f "$W/standin/standin.json"
python3 -I "$KIT_ROOT/py/07-standin.py" --dir "$W/standin" > "$W/standin/out.log" 2>&1 &
STANDIN_PID=$!
i=0; while [ ! -f "$W/standin/standin.json" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
[ -f "$W/standin/standin.json" ] || kit_fail "the stand-in engine did not start: $(cat "$W/standin/out.log")"
jget() { python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get(sys.argv[2], ""))' "$1" "$2" 2>/dev/null; }
SJ="$(cat "$W/standin/standin.json")"
P_LO="$(jget "$SJ" loopback_port)"; P_ANY="$(jget "$SJ" any_port)"
LAN="$(net_my_lan_ip)"
# The Mac's router: a device of the Mac's local network that answers on the Mac's side.
if [ "$LINUX" = 1 ]; then
  ROUTER="$(ip route show default 2>/dev/null | awk '{print $3; exit}')"
else
  ROUTER="$(route -n get default 2>/dev/null | awk '/gateway:/{print $2}')"
fi
ROUTER_SEEN=""
for p in 80 443 53 22; do
  if [ -n "$ROUTER" ] && python3 -c 'import socket,sys; socket.create_connection((sys.argv[1], int(sys.argv[2])), timeout=2)' "$ROUTER" "$p" 2>/dev/null; then
    ROUTER_SEEN="$p"; break
  fi
done
kit_note "stand-in engine: 127.0.0.1:$P_LO, 0.0.0.0:$P_ANY, $W/standin/engine.sock; the Mac's address ${LAN:-none}; its router ${ROUTER:-none}${ROUTER_SEEN:+, which answers the Mac on port $ROUTER_SEEN}"

# --- the computer on the sealed gvproxy ----------------------------------------------------------
if [ "$LINUX" = 1 ]; then MAC_ADDR="5a:94:ef:4b:11:35"; else B="$(vm_fresh n35)" || exit 1; MAC_ADDR="$(net_bundle_mac "$B")"; fi
net_gvproxy_config "$D" sealed "$MAC_ADDR"
GVPID="$(net_gvproxy_start "$D" sealed)" || { kit_attach "$D/gvproxy.out"; kit_outcome inconclusive "the sealed gvproxy did not start: $(tail -3 "$D/gvproxy.out" | tr '\n' ' ')"; exit 0; }
if [ "$LINUX" = 1 ]; then
  rm -f "$D/guest-ready.json"
  unshare -n python3 -I "$KIT_ROOT/linux/fake-guest-net.py" --sock "$D/gv.sock" --mac "$MAC_ADDR" --ready "$D/guest-ready.json" > "$D/guest.log" 2>&1 &
  echo $! > "$D/guest.pid"
  i=0; while [ ! -f "$D/guest-ready.json" ] && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
  [ -f "$D/guest-ready.json" ] || { kit_outcome sealed-breaks "the analogue's computer got no lease: $(tail -3 "$D/guest.log" | tr '\n' ' ')"; exit 0; }
  nsenter -t "$(cat "$D/guest.pid")" -n bash "$KIT_ROOT/linux/sshd-up.sh" "$D/sshd" 0.0.0.0 22 > "$D/sshd.pid" || kit_fail "sshd did not start in the namespace"
  cat "$(_vm_key).pub" >> "$D/sshd/authorized_keys"
else
  # shellcheck disable=SC2046
  vm_start "$B" $(vm_provision_args) --net "vfkit:$D/gv.sock" >/dev/null
fi
ip_lease="$(net_wait_lease "$D" "$MAC_ADDR" "$([ "$LINUX" = 1 ] && echo 30 || echo 900)")" || {
  kit_attach "$D/gvproxy.log" 2>/dev/null
  kit_outcome sealed-breaks "no DHCP lease from the sealed gvproxy"; exit 0; }
kit_fact lease "$ip_lease"
code="$(net_expose_ssh "$D" "$ip_lease")"
kit_fact silo_forward "Silo's SSH forward through the API on the Mac's own socket: HTTP $code"
s="$(net_ready "$D" "$B" "$G_USER" "$([ "$LINUX" = 1 ] && echo 60 || echo 900)")" || {
  kit_attach "$D/gvproxy.log" 2>/dev/null
  kit_outcome sealed-breaks "SSH through Silo's forward never answered (API: HTTP $code)"; exit 0; }
kit_measure ssh_ready_after_start "$s" s "through the sealed gvproxy's Unix socket forward"

g() { net_ssh "$D" "$G_USER" "/bin/sh -c $(printf '%q' "$1")" < /dev/null; }
# As root inside: the provisioned user's sudo on macOS (its password on stdin), root itself on Linux.
groot() {
  if [ "$LINUX" = 1 ]; then g "$1"; return; fi
  vm_password | net_ssh "$D" "$G_USER" "sudo -S -p '' /bin/sh -c $(printf '%q' "$1")"
}

# --- what root inside reaches -------------------------------------------------------------------
# Each target: name:host:port. "reached" means the Mac's stand-in (or the router) answered.
TARGETS="alias_loopback:$NET_HOST_ALIAS:$P_LO alias_any:$NET_HOST_ALIAS:$P_ANY gateway_loopback:$NET_GATEWAY:$P_LO gateway_any:$NET_GATEWAY:$P_ANY"
[ -n "$LAN" ] && TARGETS="$TARGETS lan_loopback:$LAN:$P_LO lan_any:$LAN:$P_ANY"
out="$(groot "id -u; for t in $TARGETS; do
  n=\${t%%:*}; r=\${t#*:}; h=\${r%%:*}; p=\${r#*:}
  o=\$(curl -s -m 4 http://\$h:\$p/ 2>/dev/null); rc=\$?
  case \"\$o\" in *silo-kit-standin*) echo \"\$n reached\" ;; *) echo \"\$n no \$rc\" ;; esac
done
for p in 22 53 80 443 2222 8080; do
  if nc -z -w 3 $NET_GATEWAY \$p 2>/dev/null; then echo \"gwport \$p open\"; fi
done
a=\$(curl -s -m 4 -o /dev/null -w '%{http_code}' http://$NET_GATEWAY/services/forwarder/all); echo \"api \$a\"
a=\$(curl -s -m 4 -o /dev/null -w '%{http_code}' -X POST -d '{\"local\":\"0.0.0.0:2299\",\"remote\":\"$NET_GUEST_IP:22\"}' http://$NET_GATEWAY/services/forwarder/expose); echo \"expose \$a\"
${ROUTER_SEEN:+if nc -z -w 3 $ROUTER $ROUTER_SEEN 2>/dev/null; then echo 'router reached'; else echo 'router no'; fi}
if nc -z -w 3 169.254.169.254 80 2>/dev/null; then echo 'metadata reached'; else echo 'metadata no'; fi
")" || true
echo "$out" > "$W/reach-root.txt"; kit_attach "$W/reach-root.txt"
kit_fact probe_user "$(echo "$out" | sed -n 1p) (0 is root)"
leaks=""
for t in $TARGETS; do
  n="${t%%:*}"; r="$(echo "$out" | awk -v n="$n" '$1 == n {print $2}')"
  kit_fact "reach.$n" "${r:-not probed}"
  [ "$r" = reached ] && leaks="$leaks $n"
done
gw="$(echo "$out" | awk '$1 == "gwport" {printf "%s ", $2}')"
kit_fact gateway_ports_open "${gw:-none}"
for p in $gw; do [ "$p" = 53 ] || leaks="$leaks gateway-port-$p"; done
api="$(echo "$out" | awk '$1 == "api" {print $2}')"; ex="$(echo "$out" | awk '$1 == "expose" {print $2}')"
kit_fact api_from_inside "GET /services/forwarder/all: ${api:-none}; expose 0.0.0.0:2299 on the Mac: ${ex:-none}"
case "$api$ex" in *200*) leaks="$leaks gvproxy-api" ;; esac
if [ -n "$ROUTER_SEEN" ]; then
  rr="$(echo "$out" | awk '$1 == "router" {print $2}')"
  kit_fact reach.router "$ROUTER:$ROUTER_SEEN ${rr:-not probed}"
  [ "$rr" = reached ] && leaks="$leaks router"
fi
kit_fact reach.metadata "$(echo "$out" | awk '$1 == "metadata" {print $2}')"
[ "$(echo "$out" | awk '$1 == "metadata" {print $2}')" = reached ] && leaks="$leaks metadata-169.254.169.254"
[ -S "$W/standin/engine.sock" ] || leaks="$leaks engine-socket-gone"

# What the Mac saw: any connection the stand-in logged came from inside, since nothing else here
# calls it.
peers="$(python3 -c '
import json, sys
seen = {}
try:
    for line in open(sys.argv[1]):
        e = json.loads(line); seen[e["listener"]] = seen.get(e["listener"], 0) + 1
except OSError: pass
print(" ".join("%s:%d" % kv for kv in sorted(seen.items())) or "none")' "$W/standin/standin.log")"
kit_fact standin_connections "$peers"
[ "$peers" = none ] || leaks="$leaks standin-saw:$peers"

# --- what the computer needs -------------------------------------------------------------------
needs=""
dns="$(g 'if command -v dscacheutil >/dev/null 2>&1; then dscacheutil -q host -a name registry.npmjs.org | awk "/ip_address/{print \$2; exit}"; else getent hosts registry.npmjs.org | awk "{print \$1; exit}"; fi')"
kit_fact dns_registry_npmjs_org "${dns:-no answer}"
[ -n "$dns" ] || needs="$needs dns"
web="$(g "curl -s -o /dev/null -m 20 -w '%{http_code} %{time_total}' https://registry.npmjs.org/")"
kit_fact https_registry_npmjs_org "$web"
case "$web" in 200*|30*) ;; *) needs="$needs https" ;; esac
big="$(g "curl -s -o /dev/null -m 120 -w '%{http_code} %{size_download} %{speed_download}' https://registry.npmjs.org/typescript/-/typescript-5.6.3.tgz")"
kit_fact download_typescript_tgz "$big"
case "$big" in 200*) m="$(echo "$big" | awk '{printf "%.1f", $3 * 8 / 1e6}')"; kit_measure download_speed "$m" Mbit/s "one npm tarball through the sealed gvproxy" ;; *) needs="$needs download" ;; esac
g 'echo ok' >/dev/null || needs="$needs ssh"

kit_attach "$D/gvproxy.log" 2>/dev/null
refused="$(grep -c 'refused' "$D/gvproxy.log" 2>/dev/null || echo 0)"
kit_fact gvproxy_refusals_logged "$refused"
if [ "$LINUX" = 0 ]; then kit_note "stopped: $(vm_stop "$B" 60)"; fi

where="$([ "$LINUX" = 1 ] && echo "Linux analogue: " || echo "")"
if [ -n "$leaks" ]; then
  kit_outcome sealed-leaks "${where}root inside reached:$leaks"
elif [ -n "$needs" ]; then
  kit_outcome sealed-breaks "${where}the computer lacked:$needs"
else
  kit_outcome sealed-holds "${where}lease $ip_lease, DNS, the internet and Silo's forward worked; root inside reached nothing of the Mac (loopback, its address ${LAN:-none}, the gateway's ports but DNS, the API) nor its router${ROUTER_SEEN:+ ($ROUTER)}; gvproxy logged $refused refusals"
fi
