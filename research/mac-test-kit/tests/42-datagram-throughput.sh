#!/bin/bash
# id: 42
# title: Throughput over the datagram socket a Mac gives gvproxy, at MTU 1500 and the largest the framework takes
# tier: vm
# time: about 30 minutes (one computer started four times: Apple's NAT, then gvproxy at three MTUs)
# needs: the base computer of test 00, a 27 MB download of gvproxy (test 07's), no sudo
# step: 2
# decides: network-stack (#380: over a datagram socket, traffic into the computer was 0.28 to 0.90 Gbit/s at MTU 1500 against 1.05 to 1.13 over a stream socket, cause not found; Apple's framework gives only the datagram socket), own-cost-targets
# question: Through gvproxy, which a Mac reaches only over a Unix datagram socket (one frame per datagram), how fast does a 512 MB download into the computer and a 256 MB upload out of it run, at MTU 1500, 9000 and the largest VZFileHandleNetworkDeviceAttachment takes, against the same over Apple's NAT? What are the Mac's datagram limits (net.local.dgram.maxdgram, recvspace)?
# outcome fast-enough: Into the computer through gvproxy at MTU 1500 is at least half of Apple's NAT: the datagram socket is not the
#   limit for main; network-stack needs no MTU change for it.
# outcome needs-large-mtu: At MTU 1500 into the computer is under half of Apple's NAT, and at a larger MTU it is at least half: silovm and
#   gvproxy set that MTU from the first start (the fact mtu_max says the largest the framework took).
# outcome datagram-limits: Into the computer stays under half of Apple's NAT at every MTU: the Mac's datagram transport is the limit, which
#   #380 names as a case for a different stack; #378's card, which carries traffic over vsock, is not affected.
# outcome inconclusive: The computer did not start on gvproxy or on Apple's NAT, or a transfer failed; the sentence says which.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/net.sh"

W="$KIT_WORK"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi
if [ "$LINUX" = 1 ]; then
  [ "$(id -u)" = 0 ] || kit_skip "the Linux analogue needs root (a network namespace and a TAP device)"
  for c in unshare nsenter curl ssh python3; do kit_need_cmd "$c" "install it"; done
  [ -c /dev/net/tun ] || kit_skip "the Linux analogue needs /dev/net/tun"
  command -v sshd >/dev/null 2>&1 || [ -x /usr/sbin/sshd ] || kit_skip "the Linux analogue needs openssh-server"
  unshare -n true 2>/dev/null || kit_skip "the Linux analogue cannot make a network namespace here"
  G_USER=root
  MB_IN="${KIT_42_MB:-16}"; MB_OUT=$((MB_IN / 2)); MTUS="${KIT_42_MTUS:-1500 9000}"
else
  kit_need_mac
  kit_need_silovm
  kit_need_base_vm
  G_USER="$VM_USER"
  MB_IN="${KIT_42_MB:-512}"; MB_OUT=$((MB_IN / 2)); MTUS="${KIT_42_MTUS:-1500 9000 65520}"
  kit_fact dgram_limits "$(sysctl net.local.dgram.maxdgram net.local.dgram.recvspace 2>/dev/null | tr '\n' ' ')"
fi
if ! net_gvproxy_present; then
  kit_confirm "Download gvproxy $NET_GVPROXY_TAG (about 27 MB) into ${NET_GVPROXY%/*} and check its sha256 against the release."
  net_gvproxy_get || { kit_outcome inconclusive "gvproxy could not be downloaded"; exit 0; }
fi

D="$W/g42"; mkdir -p "$D"; chmod 700 "$W" "$D"
B=""; SRV=""
cleanup() {
  if [ "$LINUX" = 1 ]; then
    [ -f "$D/sshd.pid" ] && kill "$(cat "$D/sshd.pid")" 2>/dev/null
    [ -f "$D/guest.pid" ] && kill "$(cat "$D/guest.pid")" 2>/dev/null
  fi
  net_gvproxy_stop "$D"
  [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
  [ -n "$B" ] && vm_destroy "$B"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

rm -f "$W/port"
python3 -I "$KIT_ROOT/py/42-server.py" --port-file "$W/port" > "$W/server.log" 2>&1 &
SRV=$!
i=0; while [ ! -f "$W/port" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
PORT="$(cat "$W/port")" || kit_fail "the bulk server did not start"

# xfer RUNNER HOST LABEL: one download of MB_IN and one upload of MB_OUT, best of two; Gbit/s.
xfer() {
  local host="$2" label="$3" r best_in=0 best_out=0
  local run="$1"
  for _ in 1 2; do
    r="$($run "curl -s -m 600 -o /dev/null -w '%{speed_download}' http://$host:$PORT/bytes/$((MB_IN << 20))")" || r=0
    best_in="$(python3 -c "print(max($best_in, float('${r:-0}' or 0) * 8 / 1e9))")"
    r="$($run "head -c $((MB_OUT << 20)) /dev/zero | curl -s -m 600 -o /dev/null -w '%{speed_upload}' -H 'Content-Type: application/octet-stream' --data-binary @- http://$host:$PORT/sink")" || r=0
    best_out="$(python3 -c "print(max($best_out, float('${r:-0}' or 0) * 8 / 1e9))")"
  done
  kit_measure "${label}_in" "$(printf '%.3f' "$best_in")" Gbit/s "$MB_IN MB into the computer, best of two"
  kit_measure "${label}_out" "$(printf '%.3f' "$best_out")" Gbit/s "$MB_OUT MB out of it, best of two"
  echo "$best_in"
}
gv_sh() { net_ssh "$D" "$G_USER" "/bin/sh -c $(printf '%q' "$1")" < /dev/null; }
nat_sh() { vm_ssh "$B" "$G_USER" "/bin/sh -c $(printf '%q' "$1")" < /dev/null; }
local_sh() { /bin/sh -c "$1"; }

# --- the reference -----------------------------------------------------------------------------
if [ "$LINUX" = 1 ]; then
  ref="$(xfer local_sh 127.0.0.1 reference_loopback)"
else
  B="$(vm_fresh n42)" || exit 1
  # shellcheck disable=SC2046
  vm_start "$B" $(vm_provision_args) >/dev/null
  vm_ready "$B" "$VM_USER" 900 >/dev/null || { kit_outcome inconclusive "the computer did not answer SSH on Apple's NAT"; exit 0; }
  natgw="$(nat_sh 'route -n get default | awk "/gateway:/{print \$2}"')"
  ref="$(xfer nat_sh "$natgw" nat)"
  vm_stop "$B" 60 >/dev/null
fi
python3 -c "import sys; sys.exit(0 if $ref > 0 else 1)" || { kit_outcome inconclusive "the reference transfer failed"; exit 0; }

# --- gvproxy at each MTU -----------------------------------------------------------------------
: > "$W/mtu.txt"; mtu_max=""
for mtu in $MTUS; do
  if [ "$LINUX" = 1 ]; then mac="5a:94:ef:4b:11:42"; else mac="$(net_bundle_mac "$B")"; fi
  NET_MTU="$mtu" net_gvproxy_config "$D" default "$mac"
  net_gvproxy_start "$D" >/dev/null || { kit_note "gvproxy did not start at MTU $mtu"; continue; }
  if [ "$LINUX" = 1 ]; then
    rm -f "$D/guest-ready.json"
    unshare -n python3 -I "$KIT_ROOT/linux/fake-guest-net.py" --sock "$D/gv.sock" --mac "$mac" --ready "$D/guest-ready.json" > "$D/guest.log" 2>&1 &
    echo $! > "$D/guest.pid"
    i=0; while [ ! -f "$D/guest-ready.json" ] && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
    nsenter -t "$(cat "$D/guest.pid")" -n bash "$KIT_ROOT/linux/sshd-up.sh" "$D/sshd" 0.0.0.0 22 > "$D/sshd.pid" || kit_fail "sshd"
    cat "$(_vm_key).pub" >> "$D/sshd/authorized_keys"
  else
    vm_start "$B" --net "vfkit:$D/gv.sock" --mtu "$mtu" >/dev/null
    sleep 3
    if ! vm_running "$B"; then
      kit_note "silovm refused MTU $mtu: $(tail -1 "$B/run.log")"; echo "$mtu refused" >> "$W/mtu.txt"; net_gvproxy_stop "$D"; continue
    fi
  fi
  ip="$(net_wait_lease "$D" "$mac" "$([ "$LINUX" = 1 ] && echo 30 || echo 600)")" &&
    [ "$(net_expose_ssh "$D" "$ip")" = 200 ] &&
    net_ready "$D" "$B" "$G_USER" 600 >/dev/null || { kit_note "no SSH on gvproxy at MTU $mtu"; echo "$mtu no-ssh" >> "$W/mtu.txt"; }
  if gv_sh true 2>/dev/null; then
    got="$(gv_sh 'ifconfig 2>/dev/null | sed -n "s/.*mtu \([0-9]*\).*/\1/p" | sort -n | tail -1')"
    kit_fact "mtu_$mtu.inside" "${got:-unknown}"
    v="$(xfer gv_sh "$NET_HOST_ALIAS" "gvproxy_mtu$mtu")"
    echo "$mtu $v" >> "$W/mtu.txt"; mtu_max="$mtu"
  fi
  if [ "$LINUX" = 1 ]; then
    kill "$(cat "$D/sshd.pid")" "$(cat "$D/guest.pid")" 2>/dev/null; rm -f "$D/sshd.pid" "$D/guest.pid"
  else
    vm_stop "$B" 60 >/dev/null
  fi
  net_gvproxy_stop "$D"
done
kit_attach "$W/mtu.txt"
kit_fact mtu_max "${mtu_max:-none}"
[ -n "$mtu_max" ] || { kit_outcome inconclusive "no transfer through gvproxy at any MTU"; exit 0; }

res="$(python3 - "$W/mtu.txt" "$ref" <<'EOF'
import sys
ref = float(sys.argv[2]); rows = {}
for line in open(sys.argv[1]):
    f = line.split()
    if len(f) == 2:
        try: rows[int(f[0])] = float(f[1])
        except ValueError: pass
low = rows.get(1500)
best = max(rows.items(), key=lambda kv: kv[1]) if rows else (None, 0)
txt = "into the computer: " + ", ".join("MTU %d %.2f Gbit/s" % kv for kv in sorted(rows.items())) + "; reference %.2f" % ref
if low is not None and low >= ref / 2: print("fast-enough\t" + txt)
elif best[1] >= ref / 2: print("needs-large-mtu\t%s (MTU %d is enough)" % (txt, best[0]))
else: print("datagram-limits\t" + txt)
EOF
)"
where="$([ "$LINUX" = 1 ] && echo "Linux analogue, a Python card, so slow by construction: " || echo "")"
kit_outcome "${res%%	*}" "$where${res#*	}"
