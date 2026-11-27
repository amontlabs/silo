#!/bin/bash
# id: 41
# title: Ping and name lookups past gvproxy: as released, #363's patch and #380's
# tier: vm
# time: about 40 minutes (four starts of one computer), plus building #380's gvproxy once (Go 1.26, as test 35)
# needs: the base computer of test 00, git (the Command Line Tools), Go 1.26.0 (downloaded, about 65 MB, unless installed), the Mac on a network; sudo with --allow-sudo only, to watch the Mac's name lookups with tcpdump (otherwise that half is not watched)
# step: 2
# decides: network-stack (#380: #363's patch leaves ping open, and only #380's -sealed and -closed close it; a lookup is a way out unless -closed), network-before-rules (#348: never your Mac, your local network or cloud metadata)
# question: On a Mac, with a macOS computer on gvproxy (as released and hardened, with #363's -sealed, with #380's -sealed and with #380's -closed), does root inside ping the Mac's own address, the Mac's router and the internet; does a TCP connection to a service on the Mac get through; and does a lookup of a random name leave through the Mac's resolver (watched with tcpdump on the Mac)? Does Silo's own SSH forward still get in under each?
# outcome silo-holds: Under #380's -sealed, no ping and no connection reached the Mac or its router; under -closed, nothing at all left: no
#   ping, no lookup (refused inside, and not seen on the Mac's network when watched); SSH came in under all four. network-stack: #380's
#   patch closes on macOS what #363's leaves open; the facts say whether #363's still pings the Mac, as #380 found on Linux.
# outcome silo-leaks: Under #380's -sealed or -closed, something reached the Mac, its router, or (under -closed) the internet or the Mac's
#   resolver; the sentence names it. #380's patch misses a path on macOS.
# outcome silo-breaks: #380's build gave no lease or no SSH forward where gvproxy as released did; the sentence says which setting.
# outcome inconclusive: #380's gvproxy did not build, or the computer did not start on gvproxy as released.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/net.sh"

W="$KIT_WORK"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi
if [ "$LINUX" = 1 ]; then
  [ "$(id -u)" = 0 ] || kit_skip "the Linux analogue needs root (a network namespace and a TAP device)"
  for c in unshare nsenter curl ssh nc python3 git ping dig; do kit_need_cmd "$c" "install it"; done
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

if ! net_gvproxy_present; then
  kit_confirm "Download gvproxy $NET_GVPROXY_TAG (about 27 MB) into ${NET_GVPROXY%/*} and check its sha256 against the release."
  net_gvproxy_get || { kit_outcome inconclusive "gvproxy as released could not be downloaded"; exit 0; }
fi
if ! net_silo_present; then
  kit_confirm "Build #380's gvproxy: fetch gvisor-tap-vsock at ${NET_GVPROXY_SILO_COMMIT:0:8} from GitHub, apply net/gvproxy-silo.patch and build it with Go $NET_GO_VERSION (downloaded from go.dev unless installed), into ${NET_GVPROXY_SILO%/*}."
  net_silo_build > "$W/build-silo.log" 2>&1 || { kit_attach "$W/build-silo.log"; kit_outcome inconclusive "#380's gvproxy did not build: $(tail -2 "$W/build-silo.log" | tr '\n' ' ')"; exit 0; }
fi
HAVE363=0
if net_sealed_present || { kit_confirm "Also build #363's sealed gvproxy (test 35's), to compare." && net_sealed_build > "$W/build-363.log" 2>&1; }; then HAVE363=1; fi
kit_fact gvproxy_silo "${NET_GVPROXY_SILO_COMMIT:0:8} with net/gvproxy-silo.patch $(net_sha256 "$KIT_ROOT/net/gvproxy-silo.patch" | cut -c1-12)"

D="$W/g41"; mkdir -p "$D"; chmod 700 "$W" "$D"
B=""; STANDIN_PID=""; DUMP_PID=""
cleanup() {
  if [ "$LINUX" = 1 ]; then
    [ -f "$D/sshd.pid" ] && kill "$(cat "$D/sshd.pid")" 2>/dev/null
    [ -f "$D/guest.pid" ] && kill "$(cat "$D/guest.pid")" 2>/dev/null
  fi
  net_gvproxy_stop "$D"
  [ -n "$DUMP_PID" ] && { if [ "$LINUX" = 1 ]; then kill "$DUMP_PID"; else sudo kill "$DUMP_PID"; fi; } 2>/dev/null
  [ -n "$STANDIN_PID" ] && kill "$STANDIN_PID" 2>/dev/null
  [ -n "$B" ] && vm_destroy "$B"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

mkdir -p "$W/standin"; chmod 700 "$W/standin"
python3 -I "$KIT_ROOT/py/07-standin.py" --dir "$W/standin" > "$W/standin/out.log" 2>&1 &
STANDIN_PID=$!
i=0; while [ ! -f "$W/standin/standin.json" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
P_ANY="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["any_port"])' "$W/standin/standin.json")"
LAN="$(net_my_lan_ip)"
if [ "$LINUX" = 1 ]; then ROUTER="$(ip route show default 2>/dev/null | awk '{print $3; exit}')"
else ROUTER="$(route -n get default 2>/dev/null | awk '/gateway:/{print $2}')"; fi
kit_note "the Mac's address ${LAN:-none}, its router ${ROUTER:-none}, a stand-in on port $P_ANY of every address"

# Who sees the lookups: tcpdump on every interface of the Mac (root), for port 53.
WATCH=0
if [ "$LINUX" = 1 ] && command -v tcpdump >/dev/null 2>&1; then
  tcpdump -l -n -i any udp port 53 or tcp port 53 > "$W/dns.txt" 2>/dev/null & DUMP_PID=$!; WATCH=1
elif [ "$LINUX" = 0 ] && [ "${KIT_SUDO:-0}" = 1 ]; then
  kit_sudo "watch the Mac's name lookups (port 53) for the random names the computer asks, until the test ends" true
  # shellcheck disable=SC2024  # the pid file is the user's own
  sudo sh -c "tcpdump -l -n -i any udp port 53 or tcp port 53 > '$W/dns.txt' 2>/dev/null & echo \$!" > "$W/dump.pid"
  DUMP_PID="$(cat "$W/dump.pid")"; WATCH=1
fi
sleep 1
kit_fact lookups_watched "$([ "$WATCH" = 1 ] && echo yes || echo "no (rerun with --allow-sudo)")"

boot() { # mode label [silo flags]
  local mode="$1" first="$2"; shift 2
  local mac
  if [ "$LINUX" = 1 ]; then mac="5a:94:ef:4b:11:41"; else mac="$(net_bundle_mac "$B")"; fi
  net_gvproxy_config "$D" "$([ "$mode" = sealed ] && echo sealed || echo hardened)" "$mac"
  net_gvproxy_start "$D" "$mode" "$@" >/dev/null || { kit_note "gvproxy ($mode $*) did not start: $(tail -2 "$D/gvproxy.out")"; return 1; }
  if [ "$LINUX" = 1 ]; then
    rm -f "$D/guest-ready.json"
    unshare -n python3 -I "$KIT_ROOT/linux/fake-guest-net.py" --sock "$D/gv.sock" --mac "$mac" --ready "$D/guest-ready.json" > "$D/guest.log" 2>&1 &
    echo $! > "$D/guest.pid"
    i=0; while [ ! -f "$D/guest-ready.json" ] && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
    [ -f "$D/guest-ready.json" ] || return 1
    nsenter -t "$(cat "$D/guest.pid")" -n bash "$KIT_ROOT/linux/sshd-up.sh" "$D/sshd" 0.0.0.0 22 > "$D/sshd.pid" || return 1
    cat "$(_vm_key).pub" >> "$D/sshd/authorized_keys"
  elif [ "$first" = 1 ]; then
    # shellcheck disable=SC2046
    vm_start "$B" $(vm_provision_args) --net "vfkit:$D/gv.sock" >/dev/null
  else
    vm_start "$B" --net "vfkit:$D/gv.sock" >/dev/null
  fi
  local ip; ip="$(net_wait_lease "$D" "$mac" "$([ "$LINUX" = 1 ] && echo 30 || echo 900)")" || return 1
  [ "$(net_expose_ssh "$D" "$ip")" = 200 ] || return 1
  net_ready "$D" "$B" "$G_USER" "$([ "$LINUX" = 1 ] && echo 60 || echo 900)" >/dev/null
}
down() {
  if [ "$LINUX" = 1 ]; then
    [ -f "$D/sshd.pid" ] && kill "$(cat "$D/sshd.pid")" 2>/dev/null
    [ -f "$D/guest.pid" ] && kill "$(cat "$D/guest.pid")" 2>/dev/null
    rm -f "$D/sshd.pid" "$D/guest.pid"
  else
    vm_stop "$B" 60 >/dev/null
  fi
  net_gvproxy_stop "$D"
}
groot() {
  if [ "$LINUX" = 1 ]; then net_ssh "$D" root "/bin/sh -c $(printf '%q' "$1")" < /dev/null; return; fi
  vm_password | net_ssh "$D" "$G_USER" "sudo -S -p '' /bin/sh -c $(printf '%q' "$1")"
}
if [ "$LINUX" = 1 ]; then PING="ping -c 2 -W 3"; NCZ="nc -z -w 3"; else PING="ping -c 2 -t 3"; NCZ="nc -z -G 3"; fi

: > "$W/reach.txt"
probe() { # label
  local l="$1" name out
  name="x$(python3 -c 'import secrets; print(secrets.token_hex(6))').silokit-leak.example"
  out="$(groot "
try() { n=\$1; shift; if \"\$@\" >/dev/null 2>&1; then echo \"\$n reached\"; else echo \"\$n no\"; fi; }
${LAN:+try ping-mac $PING $LAN}
${ROUTER:+try ping-router $PING $ROUTER}
try ping-internet $PING 1.1.1.1
${LAN:+try tcp-mac $NCZ $LAN $P_ANY}
try tcp-internet $NCZ 1.1.1.1 443
st=\$(dig +time=3 +tries=1 $name @$NET_GATEWAY A 2>/dev/null | sed -n 's/.*status: \\([A-Z]*\\).*/\\1/p'); echo \"lookup \${st:-none}\"
")" || true
  sleep 2
  local seen="no"
  [ "$WATCH" = 1 ] && grep -q "$name" "$W/dns.txt" 2>/dev/null && seen="yes"
  echo "$out" | sed "s/^/$l /" >> "$W/reach.txt"
  echo "$l lookup-seen-on-the-mac $([ "$WATCH" = 1 ] && echo "$seen" || echo unwatched)" >> "$W/reach.txt"
  kit_fact "$l" "$(echo "$out" | tr '\n' ';' | sed 's/;$//'); lookup seen on the Mac's network: $([ "$WATCH" = 1 ] && echo "$seen" || echo unwatched)"
}

if [ "$LINUX" = 0 ]; then B="$(vm_fresh n41)" || exit 1; fi
boot plain 1 || { kit_attach "$D/gvproxy.out" 2>/dev/null; kit_outcome inconclusive "the computer did not start or answer SSH on gvproxy as released"; exit 0; }
probe released; down
if [ "$HAVE363" = 1 ]; then
  if boot sealed 0; then probe sealed363; else kit_note "#363's build did not give a lease or SSH"; fi; down
fi
breaks=""
if boot silo 0 -sealed; then probe silo_sealed; else breaks="$breaks -sealed"; fi; down
if boot silo 0 -closed; then probe silo_closed; else breaks="$breaks -closed"; fi; down
kit_attach "$W/reach.txt"
[ "$WATCH" = 1 ] && kit_attach "$W/dns.txt"

r() { awk -v l="$1" -v k="$2" '$1 == l && $2 == k {print $3}' "$W/reach.txt"; }
leaks=""
for k in ping-mac ping-router tcp-mac; do
  [ "$(r silo_sealed "$k")" = reached ] && leaks="$leaks -sealed:$k"
  [ "$(r silo_closed "$k")" = reached ] && leaks="$leaks -closed:$k"
done
for k in ping-internet tcp-internet; do [ "$(r silo_closed "$k")" = reached ] && leaks="$leaks -closed:$k"; done
case "$(r silo_closed lookup)" in NOERROR|NXDOMAIN) leaks="$leaks -closed:lookup-answered" ;; esac
[ "$(r silo_closed lookup-seen-on-the-mac)" = yes ] && leaks="$leaks -closed:lookup-seen"
[ "$HAVE363" = 1 ] && kit_fact sealed363_ping_mac "$(r sealed363 ping-mac)"
where="$([ "$LINUX" = 1 ] && echo "Linux analogue: " || echo "")"
if [ -n "$leaks" ]; then
  kit_outcome silo-leaks "${where}#380's build let through:$leaks"
elif [ -n "$breaks" ]; then
  kit_outcome silo-breaks "${where}no lease or SSH forward with #380's$breaks"
else
  kit_outcome silo-holds "${where}#380's -sealed kept ping and TCP off the Mac and its router, -closed let nothing out (lookup $(r silo_closed lookup), seen on the Mac: $(r silo_closed lookup-seen-on-the-mac)); as released the computer pinged the Mac: $(r released ping-mac); #363's: $(r sealed363 ping-mac)"
fi
