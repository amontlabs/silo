#!/bin/bash
# id: 36
# title: What a computer's connections meet on gvproxy and on Apple's NAT, and a restart of the stack
# tier: vm
# time: about an hour (three starts of one computer, two replies held silent for 60 s and 330 s)
# needs: the base computer of test 00, a 27 MB download of gvproxy (test 07's), the Mac on a network (its own address is where the computer connects), no sudo
# step: 2
# decides: network-stack (#347's checks 6 to 10 against main's network, which #370 step 2 and the entry ask to be measured), when-the-engine-stops and #373 step 3 (a restart of the stack, or of the boundary in it, in the night)
# question: Through gvproxy, against Apple's NAT as the reference, does a computer's connection carry bytes exactly (100 bodies of 1 KB to 2 MB), pass a streamed reply's first event at once, keep a reply silent for 60 s and 330 s, and carry lines both ways on one connection? And when gvproxy restarts under a running computer, does the computer's network come back by itself, or only when the program holding the computer's network link connects again (a helper), or not at all?
# outcome transparent: Every check gave the same through gvproxy as through Apple's NAT, and after gvproxy restarted, the computer's new
#   connections worked again within 30 s with nothing done to the computer: the stack can be a process of its own, restarted in the night
#   (#373 step 3), and connections open at the restart end, as the facts say.
# outcome restart-needs-helper: The checks held, but after gvproxy restarted the computer's network stayed dead until the program between the
#   computer and the stack (here a relay standing in for Silo's helper) connected again and said who it is: the computer's network link must be
#   held by the helper, which reconnects, never handed by the Virtualization framework straight to the stack's socket (as silovm and vfkit do).
# outcome restart-breaks: The checks held, but after gvproxy restarted, the computer's network did not come back even through the relay: a
#   restart of the stack costs a restart of main; the stack must never stop while main runs (when-the-engine-stops, #373 step 3).
# outcome alters: A check gave less through gvproxy than through Apple's NAT (named in the sentence: bytes changed, the first event held
#   back, a silent reply cut, lines lost): network-stack's cost is not only speed; gvproxy needs that fixed before it carries main.
# outcome inconclusive: The computer did not start on gvproxy, the Mac has no network address, or a check did not run; the sentence says which.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/net.sh"

W="$KIT_WORK"
SUM="$W/summary.env"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi
if [ "$LINUX" = 1 ]; then
  [ "$(id -u)" = 0 ] || kit_skip "the Linux analogue needs root (a network namespace and a TAP device)"
  for c in unshare nsenter curl ssh nc python3 perl; do kit_need_cmd "$c" "install it"; done
  [ -c /dev/net/tun ] || kit_skip "the Linux analogue needs /dev/net/tun"
  command -v sshd >/dev/null 2>&1 || [ -x /usr/sbin/sshd ] || kit_skip "the Linux analogue needs openssh-server"
  unshare -n true 2>/dev/null || kit_skip "the Linux analogue cannot make a network namespace here"
  G_USER=root
  SLOWS="${KIT_36_SLOW:-5 20}"; BACK_WAIT="${KIT_36_BACK:-20}"; DUPLEX_S=10
else
  kit_need_mac
  kit_need_silovm
  kit_need_base_vm
  kit_need_free_gb 10
  G_USER="$VM_USER"
  SLOWS="${KIT_36_SLOW:-60 330}"; BACK_WAIT="${KIT_36_BACK:-60}"; DUPLEX_S=30
fi
: > "$SUM"
put() { echo "$1=$2" >> "$SUM"; }
put platform "$([ "$LINUX" = 1 ] && echo linux || echo mac)"
put back_wait "$BACK_WAIT"

LAN="$(net_my_lan_ip)"
[ -n "$LAN" ] || { kit_outcome inconclusive "the Mac has no address on a network: the computer has nothing to connect to"; exit 0; }
if net_gvproxy_present; then :; else
  kit_confirm "Download gvproxy $NET_GVPROXY_TAG (about 27 MB) from github.com/containers/gvisor-tap-vsock and check its sha256 against the release."
  net_gvproxy_get || kit_fail "could not get a gvproxy whose sha256 matches the release"
fi
[ "$LINUX" = 0 ] && kit_confirm "Start one macOS computer three times (gvproxy, gvproxy through a relay, Apple's NAT), about an hour, with a small server on all your Mac's addresses while it runs (macOS may ask whether python3 may accept incoming connections)."

D="$W/g"; mkdir -p "$D"; chmod 700 "$W" "$D"
B=""; SRV_PID=""; RELAY_PID=""; NAT_IP=""; NETDIR=""
lin_stop() {
  [ -f "$D/sshd.pid" ] && kill "$(cat "$D/sshd.pid")" 2>/dev/null
  [ -f "$D/guest.pid" ] && kill "$(cat "$D/guest.pid")" 2>/dev/null
  rm -f "$D/sshd.pid" "$D/guest.pid"
}
cleanup() {
  [ "$LINUX" = 1 ] && lin_stop
  net_gvproxy_stop "$D"
  [ -n "$RELAY_PID" ] && kill "$RELAY_PID" 2>/dev/null
  [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null
  [ -n "$B" ] && vm_destroy "$B"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

mkdir -p "$W/srv"; rm -f "$W/srv/server.json"
python3 -I "$KIT_ROOT/py/36-server.py" --dir "$W/srv" > "$W/srv/out.log" 2>&1 &
SRV_PID=$!
i=0; while [ ! -f "$W/srv/server.json" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
[ -f "$W/srv/server.json" ] || kit_fail "the server did not start: $(cat "$W/srv/out.log")"
PORT="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["port"])' "$W/srv/server.json")"
DPORT="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["duplex_port"])' "$W/srv/server.json")"
kit_note "the server listens on $LAN:$PORT and $LAN:$DPORT (duplex)"

# --- one computer, three ways in ------------------------------------------------------------------
# g CMD: a command inside, over the current way in: gvproxy's Unix socket forward, or Apple's NAT.
g() {
  if [ -n "$NETDIR" ]; then net_ssh "$NETDIR" "$G_USER" "/bin/sh -c $(printf '%q' "$1")" < /dev/null
  else
    # shellcheck disable=SC2046
    ssh $(_vm_ssh_opts) -o BatchMode=yes -i "$(_vm_key)" "$G_USER@$NAT_IP" "/bin/sh -c $(printf '%q' "$1")" < /dev/null
  fi
}
CHECKS="perl \$HOME/sk/36-checks.pl"
[ "$LINUX" = 1 ] && CHECKS="perl $KIT_ROOT/guest/36-checks.pl"
put_checks() { # copy the checks into the computer (on Linux, the namespace shares this disk)
  [ "$LINUX" = 1 ] && return 0
  if [ -n "$NETDIR" ]; then
    net_ssh "$NETDIR" "$G_USER" 'mkdir -p ~/sk && cat > ~/sk/36-checks.pl' < "$KIT_ROOT/guest/36-checks.pl"
  else
    # shellcheck disable=SC2046
    ssh $(_vm_ssh_opts) -o BatchMode=yes -i "$(_vm_key)" "$G_USER@$NAT_IP" 'mkdir -p ~/sk && cat > ~/sk/36-checks.pl' < "$KIT_ROOT/guest/36-checks.pl"
  fi
}

# boot_gv SOCKET [provision]: gvproxy (hardened, test 07's) is running in D; the computer's network
# device talks to SOCKET (gvproxy's own, or the relay's). Sets IP. 1 when SSH never answers.
MAC_ADDR=""
boot_gv() {
  local sock="$1" ip code
  if [ "$LINUX" = 1 ]; then
    rm -f "$D/guest-ready.json"
    unshare -n python3 -I "$KIT_ROOT/linux/fake-guest-net.py" --sock "$sock" --mac "$MAC_ADDR" --ready "$D/guest-ready.json" > "$D/guest.log" 2>&1 &
    echo $! > "$D/guest.pid"
    i=0; while [ ! -f "$D/guest-ready.json" ] && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
    [ -f "$D/guest-ready.json" ] || return 1
    nsenter -t "$(cat "$D/guest.pid")" -n bash "$KIT_ROOT/linux/sshd-up.sh" "$D/sshd" 0.0.0.0 22 > "$D/sshd.pid" || return 1
    cat "$(_vm_key).pub" >> "$D/sshd/authorized_keys"
  elif [ "${2:-}" = provision ]; then
    # shellcheck disable=SC2046
    vm_start "$B" $(vm_provision_args) --net "vfkit:$sock" >/dev/null
  else
    vm_start "$B" --net "vfkit:$sock" >/dev/null
  fi
  IP="$(net_wait_lease "$D" "$MAC_ADDR" "$([ "$LINUX" = 1 ] && echo 30 || echo 900)")" || return 1
  code="$(net_expose_ssh "$D" "$IP")"
  [ "$code" = 200 ] || return 1
  NETDIR="$D"
  net_ready "$D" "$B" "$G_USER" "$([ "$LINUX" = 1 ] && echo 60 || echo 900)" >/dev/null
}
stop_computer() {
  if [ "$LINUX" = 1 ]; then lin_stop; else vm_running "$B" && kit_note "stopped: $(vm_stop "$B" 60)"; fi
  NETDIR=""
}

# checks LABEL: #347's checks 6 to 9 from inside, against the server at the Mac's address.
checks() {
  local l="$1" r s
  r="$(g "$CHECKS bytes $LAN $PORT 100")"; kit_fact "$l.check6_bytes" "$r"
  put "${l}_bytes" "$(echo "$r" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("%d/%d" % (d["same"], d["n"]))' 2>/dev/null)"
  r="$(g "$CHECKS first $LAN $PORT 5")"; kit_fact "$l.check7_first_event" "$r"
  put "${l}_first_ms" "$(echo "$r" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["first_ms_median"] if d["whole"] == d["n"] else -1)' 2>/dev/null)"
  # The silent replies run at once, side by side, so the longest sets the time.
  r="$(g "for s in $SLOWS; do $CHECKS slow $LAN $PORT \$s & done; wait")"; kit_fact "$l.check8_silent" "$(echo "$r" | tr '\n' ' ')"
  for s in $SLOWS; do
    put "${l}_slow_$s" "$(echo "$r" | python3 -c 'import json,sys
for line in sys.stdin:
    d = json.loads(line)
    if str(d["idle"]) == sys.argv[1]: print(d["whole"])' "$s" 2>/dev/null)"
  done
  r="$(g "$CHECKS duplex $LAN $DPORT $DUPLEX_S")"; kit_fact "$l.check9_duplex" "$r"
  put "${l}_duplex" "$(echo "$r" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("%d/%d/%d" % (d["echoed"], d["sent"], d["out_of_order"]))' 2>/dev/null)"
}

# restart LABEL: a probe and a silent reply run inside; gvproxy is stopped and started again with
# the same configuration, and Silo's SSH forward is opened again as Silo would. Records whether
# and when the computer's connections came back.
restart() {
  local l="$1" t_kill t_up back hold
  hold=$((BACK_WAIT + 40))
  g "rm -f /tmp/silokit36-probe /tmp/silokit36-slow; (nohup $CHECKS probe $LAN $PORT $((BACK_WAIT + 30)) /tmp/silokit36-probe > /dev/null 2>&1 &); (nohup $CHECKS slow $LAN $PORT $hold > /tmp/silokit36-slow 2>&1 &); echo started" >/dev/null
  sleep 5
  t_kill="$(kit_now)"
  net_gvproxy_stop "$D"
  sleep 2
  net_gvproxy_start "$D" >/dev/null || { kit_note "$l: gvproxy did not start again"; put "${l}_back" never; return; }
  t_up="$(kit_now)"
  [ "$(net_expose_ssh "$D" "$IP")" = 200 ] || kit_note "$l: the SSH forward could not be opened again"
  back=never
  while python3 -c "import sys,time; sys.exit(0 if time.time() - $t_up < $BACK_WAIT else 1)"; do
    if net_limit 10 net_ssh "$D" "$G_USER" true < /dev/null 2>/dev/null; then
      back="$(python3 -c "print('%.1f' % ($(kit_now) - $t_up))")"; break
    fi
    sleep 1
  done
  put "${l}_ssh_back" "$back"
  kit_fact "$l.ssh_after_restart" "$back (seconds after gvproxy was up again)"
  if [ "$back" != never ]; then
    sleep 10
    # The outage as the computer saw it, on its own clock: from its last answer before the first
    # failure to its first answer after it.
    r="$(g "cat /tmp/silokit36-probe" | python3 -c '
import sys
rows = [l.split() for l in sys.stdin if len(l.split()) == 2]
last_ok = first_fail = None; back = None; fails = 0
for t, s in rows:
    t = float(t)
    if s == "fail":
        fails += 1
        if first_fail is None: first_fail = t
    elif first_fail is None: last_ok = t
    elif back is None: back = t
if first_fail is None: print("0.0 0")
elif back is None: print("never %d" % fails)
else: print("%.1f %d" % (back - (last_ok if last_ok is not None else first_fail), fails))')"
    put "${l}_back" "${r%% *}"
    kit_fact "$l.connections_after_restart" "requests from inside stopped working for ${r%% *} s (the computer's clock), ${r#* } failed in all"
    kit_fact "$l.silent_reply_across_restart" "$(g 'cat /tmp/silokit36-slow' | tr '\n' ' ')"
  else
    put "${l}_back" never
  fi
  kit_fact "$l.restart_window_s" "$(python3 -c "print('%.1f' % ($t_up - $t_kill))")"
}

# --- phase 1: the computer straight on gvproxy ------------------------------------------------------
if [ "$LINUX" = 1 ]; then MAC_ADDR="5a:94:ef:4b:11:36"; else B="$(vm_fresh n36)" || exit 1; MAC_ADDR="$(net_bundle_mac "$B")"; fi
net_gvproxy_config "$D" hardened "$MAC_ADDR"
net_gvproxy_start "$D" >/dev/null || kit_fail "gvproxy did not start: $(tail -3 "$D/gvproxy.out")"
kit_log "phase 1: the computer's network device straight on gvproxy's socket, as silovm and vfkit do"
if boot_gv "$D/gv.sock" provision; then
  put_checks
  checks g
  restart direct
else
  put g_fail "the computer did not answer SSH through gvproxy"
  kit_note "phase 1: the computer did not answer SSH through gvproxy"
fi
stop_computer
net_gvproxy_stop "$D"
cp "$D/gvproxy.log" "$W/gvproxy-direct.log" 2>/dev/null && kit_attach "$W/gvproxy-direct.log"

# --- phase 2: the same through a relay that reconnects, as a helper owning the link would -----------
if ! grep -q '^g_fail=' "$SUM"; then
  kit_log "phase 2: through a relay that connects again when gvproxy restarts"
  net_gvproxy_start "$D" >/dev/null || kit_fail "gvproxy did not start"
  rm -f "$D/relay.json"
  python3 -I "$KIT_ROOT/py/36-relay.py" --down "$D/rd.sock" --up "$D/gv.sock" --dir "$D" > "$D/relay.out" 2>&1 &
  RELAY_PID=$!
  i=0; while [ ! -f "$D/relay.json" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
  if [ -f "$D/relay.json" ] && boot_gv "$D/rd.sock"; then
    r="$(g "$CHECKS bytes $LAN $PORT 20")"; kit_fact relay.check6_bytes "$r"
    restart relay
    kit_fact relay.reconnects "$(grep -c connected "$D/relay.log" 2>/dev/null)"
  else
    put relay_back never
    kit_note "phase 2: the computer did not answer SSH through the relay: $(tail -3 "$D/relay.out" | tr '\n' ' ')"
  fi
  stop_computer
  kill "$RELAY_PID" 2>/dev/null; RELAY_PID=""
  net_gvproxy_stop "$D"
fi

# --- phase 3: Apple's NAT, the reference ----------------------------------------------------------------
if [ "$LINUX" = 0 ]; then
  kit_log "phase 3: Apple's NAT"
  mkdir -p "$KIT_SSH_DIR" && : > "$KIT_SSH_DIR/known_hosts"
  vm_start "$B" --net nat >/dev/null
  if vm_wait_ssh "$B" "$G_USER" 900 >/dev/null; then
    vm_setup_key "$B" "$G_USER" >/dev/null 2>&1
    NAT_IP="$(vm_ip "$B" 30)"
    put_checks
    checks n
  else
    kit_note "phase 3: the computer did not answer SSH on Apple's NAT"
  fi
  stop_computer
fi

kit_attach "$W/srv/server.log"
kit_attach "$SUM"
res="$(python3 -I "$KIT_ROOT/py/36-decide.py" "$SUM")"
kit_outcome "${res%%|*}" "${res#*|}"
