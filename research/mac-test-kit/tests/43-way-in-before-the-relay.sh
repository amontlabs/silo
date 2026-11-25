#!/bin/bash
# id: 43
# title: How the first run reaches main before the relay is in it (#382)
# tier: vm
# time: about 40 minutes (two first starts, each followed by a start on the card); 5 minutes more to download gvproxy if test 07 has not
# needs: the base computer of test 00, the Command Line Tools (the SDK's headers), the Mac on a network, a 27 MB download of gvproxy (test 07's) for the second way, no sudo on the Mac
# step: 2
# decides: way-in-before-the-relay (#382's three ways for the first SSH, when main's card leads nowhere), first-run-time-macos (what the extra start costs)
# question: A provisioned computer turns on SSH on its network card, and a card that leads nowhere gives SSH no road until the relay is in. Does any option of Apple's guest provisioning run a program at the first start (then the relay could start with no road at all)? How long does #382's recommended way take (first start on Apple's NAT, relay in over SSH, a stop, a start on the card, SSH over vsock), and what can the computer reach meanwhile? Does the second way (gvproxy with nothing out, SSH in through its forward) do the same?
# outcome nat-first-start-works: The first start on Apple's NAT took the relay, and the start on the card answered SSH over vsock; the
#   measures give what the extra start costs and the facts what the computer could reach during it. way-in-before-the-relay: #382's
#   recommendation can be built as written; the fact gvproxy_first_start says whether the second way is a working alternative.
# outcome nat-first-start-fails: The relay did not go in over Apple's NAT, or the computer did not come back over vsock after it did: the
#   recommended way does not work as written; the sentence says where it stopped.
# outcome inconclusive: The computer did not start or answer SSH on its first start; the sentence says why.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/net.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/deadend.sh"

W="$KIT_WORK"
# --- 1. what Apple's provisioning takes, from the SDK's headers ----------------------------------
if [ -n "${KIT_43_HEADERS:-}" ]; then
  HDRS="$KIT_43_HEADERS"
elif kit_is_mac && sdk="$(xcrun --show-sdk-path 2>/dev/null)"; then
  HDRS="$(grep -l -r 'ProvisioningOptions' "$sdk/System/Library/Frameworks/Virtualization.framework/Headers" 2>/dev/null | tr '\n' ' ')"
else
  HDRS=""
fi
if [ -n "$HDRS" ]; then
  # shellcheck disable=SC2086
  pj="$(python3 -I "$KIT_ROOT/py/43-provisioning.py" $HDRS)" || pj=""
else
  pj=""
fi
if [ -n "$pj" ]; then
  echo "$pj" > "$W/provisioning.json"; kit_attach "$W/provisioning.json"
  kit_fact provisioning_options "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print("; ".join("%s: %s" % (c, ", ".join(p) or "none") for c, p in d["classes"].items()))' "$pj")"
  ran="$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print("yes" if d["runs_program"] else "no")' "$pj")"
  kit_fact provisioning_can_run_program "$ran"
  [ "$ran" = yes ] && kit_fact provisioning_candidates "$(python3 -c 'import json,sys; print(", ".join(json.loads(sys.argv[1])["candidates"]))' "$pj")"
else
  kit_fact provisioning_can_run_program unknown
  kit_note "no SDK header names a provisioning class (no Command Line Tools for macOS 27?)"
fi

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  kit_skip "the starts need a Mac: Apple's NAT and guest provisioning have no Linux analogue (the header reading above ran)"
fi
kit_need_mac
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 10
RELAY="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/vsockrelay"
[ -x "$RELAY" ] || kit_skip "vsockrelay is not built: run ./kit build"

D="$W/de"; G="$W/gv"; mkdir -p "$D" "$G"; chmod 700 "$W" "$D" "$G"
B=""; B2=""; STANDIN_PID=""
cleanup() {
  de_down "$B" "$D"; de_down "$B2" "$D"
  de_proxy_stop "$D"
  net_gvproxy_stop "$G"
  [ -n "$STANDIN_PID" ] && kill "$STANDIN_PID" 2>/dev/null
  [ -n "$B" ] && vm_destroy "$B"
  [ -n "$B2" ] && vm_destroy "$B2"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

mkdir -p "$W/standin"; chmod 700 "$W/standin"
python3 -I "$KIT_ROOT/py/07-standin.py" --dir "$W/standin" > "$W/standin/out.log" 2>&1 &
STANDIN_PID=$!
i=0; while [ ! -f "$W/standin/standin.json" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
P_ANY="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["any_port"])' "$W/standin/standin.json" 2>/dev/null)"
LAN="$(net_my_lan_ip)"
ROUTER="$(route -n get default 2>/dev/null | awk '/gateway:/{print $2}')"
de_proxy_start "$D" --allow-public || kit_fail "the proxy did not start: $(cat "$D/proxy.log")"

# What a computer reaches from where it is: the Mac's stand-in, its router, the internet.
exposure() { # runner...
  "$@" "for t in ${LAN:+mac:$LAN:$P_ANY} ${ROUTER:+router:$ROUTER:80} internet:1.1.1.1:80; do
  n=\${t%%:*}; r=\${t#*:}; if nc -z -G 3 \${r%%:*} \${r#*:} >/dev/null 2>&1; then echo \"\$n reached\"; else echo \"\$n no\"; fi
done" 2>/dev/null | tr '\n' ' '
}

# --- 2. #382's recommended way: Apple's NAT for the first start only ------------------------------
B="$(vm_fresh n43)" || exit 1
guest_forget_hosts
t0="$(kit_now)"
# shellcheck disable=SC2046
vm_start "$B" $(vm_provision_args) >/dev/null
s1="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_outcome inconclusive "the computer did not answer SSH on its first start, on Apple's NAT"; exit 0; }
GB="$B"
kit_measure nat_first_start_to_ssh "$s1" s "provisioned, on Apple's NAT"
natx="$(exposure guest_sh)"
kit_fact nat_window_reached "$natx"
t1="$(kit_now)"
if ! de_install_relay "$B" > "$W/install.log" 2>&1; then
  kit_attach "$W/install.log"
  kit_outcome nat-first-start-fails "the relay did not go in over SSH on Apple's NAT: $(tail -2 "$W/install.log" | tr '\n' ' ')"; exit 0
fi
t2="$(kit_now)"
kit_measure relay_install "$(python3 -c "print('%.1f' % ($t2 - $t1))")" s "copy, two launch daemons, the card's address"
how="$(vm_stop "$B" 90)"
t3="$(kit_now)"
kit_measure stop_after_install "$(python3 -c "print('%.1f' % ($t3 - $t2))")" s "stopped: $how"
s2="$(de_boot "$B" "$D")" || {
  kit_attach "$B/run.jsonl"
  kit_outcome nat-first-start-fails "the relay went in, but the start on the card never answered SSH over vsock"; exit 0; }
t4="$(kit_now)"
kit_measure card_start_to_ssh "$s2" s "the second start, on the card"
kit_measure nat_way_total "$(python3 -c "print('%.1f' % ($t4 - $t0))")" s "from the first start to SSH over vsock on the card"
kit_measure nat_way_extra "$(python3 -c "print('%.1f' % ($t4 - $t1))")" s "what #382's way adds after the first SSH: the relay, a stop, a start"
kit_fact card_window_reached "$(exposure de_sh "$D")"
kit_fact card_address "$(de_sh "$D" 'ipconfig getifaddr en0; route -n get default 2>/dev/null | awk "/gateway:/{print \$2}"' | tr '\n' ' ')"
before="$(grep -c registry.npmjs.org "$D/proxy.jsonl" 2>/dev/null || true)"
de_sh "$D" "HTTPS_PROXY=http://127.0.0.1:$DE_PROXY_PORT curl -s -m 20 -o /dev/null https://registry.npmjs.org/" >/dev/null 2>&1
after="$(grep -c registry.npmjs.org "$D/proxy.jsonl" 2>/dev/null || true)"
kit_fact card_proxy_carries "$([ "${after:-0}" -gt "${before:-0}" ] && echo yes || echo no)"
de_down "$B" "$D"
vm_destroy "$B"; B=""
nat_ok=1

# --- 3. the second way: gvproxy, nothing out, SSH in through its forward -------------------------
gvw="not-run"
if net_gvproxy_present || { kit_confirm "Download gvproxy $NET_GVPROXY_TAG (about 27 MB) into ${NET_GVPROXY%/*} for the second way, and check its sha256 against the release." && net_gvproxy_get; }; then
  B2="$(vm_fresh n43b)" || exit 1
  MACA="$(net_bundle_mac "$B2")"
  net_gvproxy_config "$G" hardened "$MACA"
  if net_gvproxy_start "$G" hardened >/dev/null; then
    guest_forget_hosts
    t0="$(kit_now)"
    # shellcheck disable=SC2046
    vm_start "$B2" $(vm_provision_args) --net "vfkit:$G/gv.sock" >/dev/null
    if ip2="$(net_wait_lease "$G" "$MACA" 900)" && [ "$(net_expose_ssh "$G" "$ip2")" = 200 ] && s3="$(net_ready "$G" "$B2" "$VM_USER" 900)"; then
      kit_measure gvproxy_first_start_to_ssh "$s3" s "provisioned, on gvproxy (hardened), SSH through its Unix socket forward"
      kit_fact gvproxy_window_reached "$(exposure net_ssh "$G" "$VM_USER")"
      if de_install_relay "$B2" "$G" > "$W/install-gv.log" 2>&1; then
        vm_stop "$B2" 90 >/dev/null
        net_gvproxy_stop "$G"
        if s4="$(de_boot "$B2" "$D")"; then
          gvw="works"
          kit_measure gvproxy_way_total "$(python3 -c "print('%.1f' % ($(kit_now) - $t0))")" s "from the first start to SSH over vsock on the card"
          kit_measure gvproxy_card_start_to_ssh "$s4" s ""
        else
          gvw="fails"; kit_note "second way: no SSH over vsock after the relay went in through gvproxy"
        fi
      else
        gvw="fails"; kit_attach "$W/install-gv.log"; kit_note "second way: the relay did not go in through gvproxy's forward"
      fi
    else
      gvw="fails"; kit_note "second way: no lease, forward or SSH on gvproxy"
    fi
  else
    gvw="fails"; kit_note "second way: gvproxy did not start"
  fi
fi
kit_fact gvproxy_first_start "$gvw"

if [ "$nat_ok" = 1 ]; then
  kit_outcome nat-first-start-works "Apple's NAT for the first start, then the card: SSH over vsock after $(python3 -c "print('%.0f' % ($t4 - $t1))") s more than the first SSH; during the NAT window the computer reached: $natx; the second way (gvproxy): $gvw; provisioning runs a program: ${ran:-unknown}"
fi
