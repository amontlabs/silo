#!/bin/bash
# id: 35
# title: Silo's gvproxy (#363's seal, grown in #380) with a macOS computer: what root inside reaches, sealed and closed
# tier: vm
# time: about 40 minutes, plus building gvproxy once (Go 1.26, about 65 MB, and gvproxy's modules)
# needs: the base computer of test 00, git (the Command Line Tools), a 65 MB download of Go unless Go 1.26.0 is installed, no sudo on the Mac
# step: 2
# blocks: option A of the first build (#373): main is on Silo's network stack from its first start, and #363 and #380 recommend the sealed gvproxy as that stack until Silo writes its own
# decides: network-stack (the sealed gvproxy as main's stack: #363's recommendation, #380's patch, #370's condition for a closed network and for "never your Mac, your local network"), main-on-macos-first (option A's stack)
# question: With #380's patch at gvisor-tap-vsock 197eea05, started with -sealed, does gvproxy give a macOS computer what it needs (a lease, DNS, the internet, Silo's own SSH forward) while nothing inside, root included, reaches the Mac (its loopback, its own addresses, gvproxy's API, the engine's socket) or the Mac's local network, by TCP or by ping? Started with -closed, does nothing go out at all (no connection, no ping, no name asked of a resolver) while Silo's SSH forward still gets in? How fast is traffic into the computer over the datagram socket, at MTU 1500 and 65520?
# outcome sealed-holds: Sealed, the computer got its lease, DNS, the internet and Silo's SSH forward, and root inside reached nothing of the Mac or its
#   network: no loopback through 192.168.127.254, no port of the gateway but DNS, no API, no service or ping on the Mac's own address, no
#   device on the Mac's network. Closed, nothing went out and SSH still got in. network-stack: the sealed gvproxy can carry main from its
#   first start (option A), the first run can say "never your Mac, your local network" (#370 finding 3) without Apple's NAT, and the closed
#   network #370 says Apple's NAT cannot give exists. Names still reach the Mac's resolver when sealed (the facts say so): a closed computer,
#   or the boundary (#380), is what stops a name carrying data out.
# outcome sealed-leaks: Root inside reached something of the Mac or its local network when sealed, or anything outside when closed (named in the
#   sentence): the patch misses a path on macOS; the sealed gvproxy cannot carry main until it is fixed, and the facts say what to add.
# outcome sealed-breaks: The sealed build gave no lease, no DNS, no internet or no SSH forward, or the closed one no SSH forward, while the same
#   computer has them on gvproxy as released (test 07): the patch refuses something the computer needs; the sentence says what.
# outcome inconclusive: Silo's gvproxy did not build, or the computer did not start on it; the sentence says why.
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
  kit_confirm "Build Silo's gvproxy: clone gvisor-tap-vsock from GitHub at ${NET_GVPROXY_COMMIT%"${NET_GVPROXY_COMMIT#????????}"}, apply net/gvproxy-silo.patch (#380) and build it with Go $NET_GO_VERSION (downloaded from go.dev, about 65 MB, and checked, unless that Go is installed), into ${NET_GVPROXY_SEALED%/*}."
  if ! net_sealed_build > "$W/build.log" 2>&1; then
    kit_attach "$W/build.log"
    kit_outcome inconclusive "the sealed gvproxy did not build: $(tail -3 "$W/build.log" | tr '\n' ' ')"
    exit 0
  fi
fi
kit_fact gvproxy_sealed "$("$NET_GVPROXY_SEALED" -version 2>&1 | head -1), gvisor-tap-vsock $NET_GVPROXY_COMMIT with net/gvproxy-silo.patch $(net_sha256 "$KIT_ROOT/net/gvproxy-silo.patch" | cut -c1-12)"

D="$W/gs"; mkdir -p "$D"; chmod 700 "$W" "$D"
B=""; STANDIN_PID=""
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

# --- the computer on Silo's gvproxy ---------------------------------------------------------------
if [ "$LINUX" = 1 ]; then MAC_ADDR="5a:94:ef:4b:11:35"; else B="$(vm_fresh n35)" || exit 1; MAC_ADDR="$(net_bundle_mac "$B")"; fi
lin_stop() {
  [ -f "$D/sshd.pid" ] && kill "$(cat "$D/sshd.pid")" 2>/dev/null
  [ -f "$D/guest.pid" ] && kill "$(cat "$D/guest.pid")" 2>/dev/null
  rm -f "$D/sshd.pid" "$D/guest.pid"
}
# boot MODE MTU [provision]: gvproxy in MODE (sealed or closed) at MTU, the computer on it, Silo's
# SSH forward. Sets ip_lease. Ends the test as sealed-breaks when the computer gets no lease or SSH.
boot() {
  local mode="$1" mtu="$2" code s
  NET_MTU="$mtu" net_gvproxy_config "$D" plain "$MAC_ADDR"
  net_gvproxy_start "$D" "$mode" >/dev/null || { kit_attach "$D/gvproxy.out"; kit_outcome inconclusive "Silo's gvproxy did not start with -$mode: $(tail -3 "$D/gvproxy.out" | tr '\n' ' ')"; exit 0; }
  if [ "$LINUX" = 1 ]; then
    rm -f "$D/guest-ready.json"
    unshare -n python3 -I "$KIT_ROOT/linux/fake-guest-net.py" --sock "$D/gv.sock" --mac "$MAC_ADDR" --ready "$D/guest-ready.json" > "$D/guest.log" 2>&1 &
    echo $! > "$D/guest.pid"
    i=0; while [ ! -f "$D/guest-ready.json" ] && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
    [ -f "$D/guest-ready.json" ] || { kit_outcome sealed-breaks "-$mode: the analogue's computer got no lease: $(tail -3 "$D/guest.log" | tr '\n' ' ')"; exit 0; }
    nsenter -t "$(cat "$D/guest.pid")" -n bash "$KIT_ROOT/linux/sshd-up.sh" "$D/sshd" 0.0.0.0 22 > "$D/sshd.pid" || kit_fail "sshd did not start in the namespace"
    cat "$(_vm_key).pub" >> "$D/sshd/authorized_keys"
  elif [ "${3:-}" = provision ]; then
    # shellcheck disable=SC2046
    vm_start "$B" $(vm_provision_args) --net "vfkit:$D/gv.sock" --mtu "$mtu" >/dev/null
  else
    vm_start "$B" --net "vfkit:$D/gv.sock" --mtu "$mtu" >/dev/null
  fi
  ip_lease="$(net_wait_lease "$D" "$MAC_ADDR" "$([ "$LINUX" = 1 ] && echo 30 || echo 900)")" || {
    kit_attach "$D/gvproxy.log" 2>/dev/null
    kit_outcome sealed-breaks "-$mode: no DHCP lease from Silo's gvproxy"; exit 0; }
  kit_fact "$mode.lease" "$ip_lease"
  code="$(net_expose_ssh "$D" "$ip_lease")"
  kit_fact "$mode.silo_forward" "Silo's SSH forward through the API on the Mac's own socket: HTTP $code"
  s="$(net_ready "$D" "$B" "$G_USER" "$([ "$LINUX" = 1 ] && echo 60 || echo 900)")" || {
    kit_attach "$D/gvproxy.log" 2>/dev/null
    kit_outcome sealed-breaks "-$mode: SSH through Silo's forward never answered (API: HTTP $code)"; exit 0; }
  kit_measure "${mode}_ssh_ready_after_start" "$s" s "through Silo's gvproxy's Unix socket forward, -$mode"
  put_helpers
}
# stop_all: the computer and gvproxy stop (a restart of gvproxy under a running computer cuts its link: test 36).
stop_all() {
  if [ "$LINUX" = 1 ]; then lin_stop; else kit_note "stopped: $(vm_stop "$B" 60)"; fi
  cp "$D/gvproxy.log" "$W/gvproxy-$1.log" 2>/dev/null && kit_attach "$W/gvproxy-$1.log"
  net_gvproxy_stop "$D"
}

DNSPL="perl /Users/$G_USER/sk/35-dns.pl"   # an absolute path: root runs it too
[ "$LINUX" = 1 ] && DNSPL="perl $KIT_ROOT/guest/35-dns.pl"
put_helpers() { [ "$LINUX" = 1 ] || g 'mkdir -p ~/sk && cat > ~/sk/35-dns.pl' < "$KIT_ROOT/guest/35-dns.pl"; }
g() { if [ -t 0 ]; then net_ssh "$D" "$G_USER" "/bin/sh -c $(printf '%q' "$1")" < /dev/null; else net_ssh "$D" "$G_USER" "/bin/sh -c $(printf '%q' "$1")"; fi; }
# As root inside: the provisioned user's sudo on macOS (its password on stdin), root itself on Linux.
groot() {
  if [ "$LINUX" = 1 ]; then g "$1"; return; fi
  vm_password | net_ssh "$D" "$G_USER" "sudo -S -p '' /bin/sh -c $(printf '%q' "$1")"
}

# pg HOST, inside: two pings, the exit status says whether one came back.
PG='pg() { if [ "$(uname)" = Darwin ]; then ping -c 2 -t 4 "$1"; else ping -c 2 -w 4 "$1"; fi > /dev/null 2>&1; }'
# rcode_left KEY: whether a DNS answer ("rcode N" or "none") shows the question left gvproxy.
rcode_left() { case "$1" in "rcode 5"|none|"") echo no ;; *) echo yes ;; esac; }
EXFIL_ZONE=example.com

# thr LABEL: 512 MiB from the Mac into the computer through a forward on gvproxy's own socket, to a
# listener inside (nc), as Silo's way in carries it. Prints Gbit/s, or nothing.
thr() {
  local code i=0
  g 'f=/tmp/silokit35-nc.pid; [ -f $f ] && kill "$(cat $f)" 2>/dev/null; nc -l 5001 < /dev/null > /dev/null 2>&1 & echo $! > $f; sleep 1; echo ok' >/dev/null
  rm -f "$D/thr.sock"
  code="$(net_api "$D" /services/forwarder/expose "{\"local\":\"$D/thr.sock\",\"protocol\":\"unix\",\"remote\":\"tcp://$ip_lease:5001\"}")"
  while [ ! -S "$D/thr.sock" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
  [ "$code" = 200 ] && [ -S "$D/thr.sock" ] || { kit_note "$1: the forward for the throughput was not made (HTTP $code)"; return 0; }
  python3 - "$D/thr.sock" <<'PY' 2>> "$W/thr-$1.err"
import socket, sys, time
s = socket.socket(socket.AF_UNIX)
s.settimeout(120)
s.connect(sys.argv[1])
buf, n = b"\0" * (1 << 20), 512
t = time.time()
for _ in range(n):
    s.sendall(buf)
s.shutdown(socket.SHUT_WR)
while s.recv(65536):
    pass
print("%.2f" % (n * 8 * 1048576 / 1e9 / (time.time() - t)))
PY
  [ -s "$W/thr-$1.err" ] && kit_note "$1: the throughput did not finish: $(tail -1 "$W/thr-$1.err")"
}

boot sealed 1500 provision

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
$PG
${LAN:+if pg $LAN; then echo 'ping_lan reached'; else echo 'ping_lan no'; fi}
${ROUTER:+if pg $ROUTER; then echo 'ping_router reached'; else echo 'ping_router no'; fi}
if pg 1.1.1.1; then echo 'ping_internet reached'; else echo 'ping_internet no'; fi
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
# -sealed refuses private, link-local, CGNAT and multicast addresses and the Mac's own: a local
# network on other addresses (some offices and universities) is outside it, and named so.
lan_kind() { python3 -c '
import ipaddress, sys
a = ipaddress.ip_address(sys.argv[1])
nets = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "100.64.0.0/10", "224.0.0.0/4"]
print("private" if any(a in ipaddress.ip_network(n) for n in nets) else "not-private")' "$1" 2>/dev/null; }
for n in ping_lan ping_router; do
  r="$(echo "$out" | awk -v n="$n" '$1 == n {print $2}')"
  h="$([ "$n" = ping_lan ] && echo "${LAN:-none}" || echo "${ROUTER:-none}")"
  kit_fact "reach.$n" "${r:-not probed} ($h)"
  if [ "$r" = reached ]; then
    if [ "$n" = ping_router ] && [ "$(lan_kind "$ROUTER")" = not-private ]; then leaks="$leaks ping_router:not-private-lan"; else leaks="$leaks $n"; fi
  fi
done
kit_fact reach.ping_internet "$(echo "$out" | awk '$1 == "ping_internet" {print $2}') (1.1.1.1; reached is expected when sealed)"

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

# A made-up name: when it gets an answer (even "no such name"), the question left the computer for
# the Mac's resolver and beyond, which is how a name can carry data out (#380: dns-exfil).
ex="$(g "$DNSPL $NET_GATEWAY silokit35-$$-$RANDOM.$EXFIL_ZONE")"
kit_fact sealed.dns_made_up_name "${ex:-none} (left gvproxy: $(rcode_left "$ex"))"
t="$(thr sealed)"
kit_fact sealed.into_computer_gbit_s "${t:-not measured} (MTU 1500, 512 MiB through a forward on gvproxy's socket)"
[ -n "$t" ] && kit_measure into_computer_mtu1500 "$t" Gbit/s "512 MiB from the Mac to nc inside, through gvproxy's datagram link at MTU 1500"
refused="$(grep -c 'refused' "$D/gvproxy.log" 2>/dev/null || echo 0)"
kit_fact sealed.gvproxy_refusals_logged "$refused"
stop_all sealed

# --- closed, at MTU 65520 ---------------------------------------------------------------------------
# The same computer started again on gvproxy -closed: nothing goes out, names are refused, Silo's
# SSH forward still gets in. The larger MTU measures traffic into the computer once more.
kit_log "closed: the computer again, on gvproxy -closed at MTU 65520"
boot closed 65520
out="$(groot "$PG
${LAN:+if pg $LAN; then echo 'ping_lan reached'; else echo 'ping_lan no'; fi}
if pg 1.1.1.1; then echo 'ping_internet reached'; else echo 'ping_internet no'; fi
if curl -s -o /dev/null -m 10 https://1.1.1.1/; then echo 'https_internet reached'; else echo 'https_internet no'; fi
if nc -z -w 3 ${LAN:-192.0.2.1} $P_ANY 2>/dev/null; then echo 'lan_any reached'; else echo 'lan_any no'; fi
echo \"dns_known \$($DNSPL $NET_GATEWAY registry.npmjs.org)\"
echo \"dns_made_up \$($DNSPL $NET_GATEWAY silokit35-$$-closed.$EXFIL_ZONE)\"
")" || true
echo "$out" > "$W/reach-closed.txt"; kit_attach "$W/reach-closed.txt"
for n in ping_lan ping_internet https_internet lan_any; do
  r="$(echo "$out" | awk -v n="$n" '$1 == n {print $2}')"
  kit_fact "closed.reach.$n" "${r:-not probed}"
  [ "$r" = reached ] && leaks="$leaks closed:$n"
done
for n in dns_known dns_made_up; do
  r="$(echo "$out" | awk -v n="$n" '$1 == n {$1 = ""; sub(/^ /, ""); print}')"
  kit_fact "closed.$n" "${r:-none} (left gvproxy: $(rcode_left "$r"))"
  [ "$(rcode_left "$r")" = yes ] && leaks="$leaks closed:$n"
done
g 'echo ok' >/dev/null || needs="$needs closed-ssh"
t="$(thr closed)"
kit_fact closed.into_computer_gbit_s "${t:-not measured} (MTU 65520)"
[ -n "$t" ] && kit_measure into_computer_mtu65520 "$t" Gbit/s "512 MiB from the Mac to nc inside, through gvproxy's datagram link at MTU 65520"
stop_all closed

where="$([ "$LINUX" = 1 ] && echo "Linux analogue: " || echo "")"
if [ -n "$leaks" ]; then
  kit_outcome sealed-leaks "${where}root inside reached:$leaks"
elif [ -n "$needs" ]; then
  kit_outcome sealed-breaks "${where}the computer lacked:$needs"
else
  kit_outcome sealed-holds "${where}sealed: lease, DNS, the internet and Silo's forward worked, and root inside reached nothing of the Mac (loopback, its address ${LAN:-none} by TCP or ping, the gateway's ports but DNS, the API) nor its router${ROUTER:+ ($ROUTER)}; a made-up name still left gvproxy ($(rcode_left "$ex")). Closed: nothing went out, names were refused, SSH got in"
fi
