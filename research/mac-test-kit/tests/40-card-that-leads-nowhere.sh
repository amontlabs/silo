#!/bin/bash
# id: 40
# title: A network card that leads nowhere, with Silo's relay and proxy over vsock (#378)
# tier: vm
# time: about an hour (a first start on Apple's NAT to put the relay in, then two starts on the card, 4 settings of 9 programs)
# needs: the base computer of test 00, about 250 MB downloaded inside the computer on its first start (Node, Claude Code, Codex), the Mac on a network, no sudo on the Mac
# step: 2
# blocks: option A of the first build on #378's card instead of gvproxy, which #380 and #382 recommend for main's first start: then test 07 no longer blocks it
# decides: network-stack (#378's card as main's network, which #380 recommends for the first build), main-on-macos-first (option A without gvproxy), network-place (the frames the card drops)
# question: With a network card whose other end on the Mac answers ARP for one router and drops every other frame, and a relay that carries 127.0.0.1:3128 inside to Silo's proxy over vsock and the Mac's SSH in over vsock: does SSH get in; which programs (curl, git, npm, Node's fetch, pip, Claude Code, Codex, Homebrew, xcodebuild) reach the internet through the proxy with the system proxy set, with the proxy variables, with both and with neither; does anything reach anything without it, root included (a route, another address, ping, a name server, the Mac's own address); and is a macOS computer "offline" when the router's ARP goes unanswered?
# outcome closed-holds: SSH came in over vsock, the proxy carried curl and at least one agent program, and nothing reached anything without the
#   proxy: every attempt by root (a route, an address of its own, ping, a lookup at a public name server, the Mac's own address) ended at the
#   card, which counted it. network-stack: #378's card can carry main's first build without gvproxy (#380's recommendation), and test 07
#   stops blocking option A. The facts say which programs need which proxy setting; a program that follows neither stays closed.
# outcome leaks: Something reached a server without the proxy (named in the sentence): the card or the relay lets something through that
#   #378 says it cannot; the card cannot carry main until that is found.
# outcome relay-fails: SSH over vsock never answered, or nothing inside ever reached the proxy: the relay does not work in a macOS
#   computer as built (AF_VSOCK, launchd or silovm's bridges); the sentence says which. #378's card cannot be main's network until it does.
# outcome inconclusive: The computer did not start or did not take the relay on its first start on Apple's NAT; the sentence says why.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/net.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/deadend.sh"

W="$KIT_WORK"
D="$W/de"; mkdir -p "$D"; chmod 700 "$W" "$D"
RELAY="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/vsockrelay"
LINUX=0
if de_linux; then
  LINUX=1
  # The analogue (#378's own): a network namespace for the computer, Unix sockets for vsock.
  [ "$(id -u)" = 0 ] || kit_skip "the Linux analogue needs root (a network namespace and a TAP device)"
  for c in unshare nsenter curl ssh nc python3; do kit_need_cmd "$c" "install it"; done
  [ -c /dev/net/tun ] || kit_skip "the Linux analogue needs /dev/net/tun"
  command -v sshd >/dev/null 2>&1 || [ -x /usr/sbin/sshd ] || kit_skip "the Linux analogue needs openssh-server"
  unshare -n true 2>/dev/null || kit_skip "the Linux analogue cannot make a network namespace here"
else
  kit_need_mac
  kit_need_silovm
  kit_need_base_vm
  kit_need_free_gb 10
fi
[ -x "$RELAY" ] || kit_skip "vsockrelay is not built: run ./kit build"

B=""; STANDIN_PID=""; ORIGIN_PID=""
cleanup() {
  de_down "$B" "$D"
  de_proxy_stop "$D"
  [ -n "$STANDIN_PID" ] && kill "$STANDIN_PID" 2>/dev/null
  [ -n "$ORIGIN_PID" ] && kill "$ORIGIN_PID" 2>/dev/null
  [ -n "$B" ] && vm_destroy "$B"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

# A stand-in service on every address of the Mac (test 07's): whatever reaches it came from inside.
mkdir -p "$W/standin"; chmod 700 "$W/standin"; rm -f "$W/standin/standin.json"
python3 -I "$KIT_ROOT/py/07-standin.py" --dir "$W/standin" > "$W/standin/out.log" 2>&1 &
STANDIN_PID=$!
i=0; while [ ! -f "$W/standin/standin.json" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
[ -f "$W/standin/standin.json" ] || kit_fail "the stand-in did not start: $(cat "$W/standin/out.log")"
P_ANY="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["any_port"])' "$W/standin/standin.json")"
LAN="$(net_my_lan_ip)"
kit_fact mac_address_seen_from_inside "${LAN:-none}:$P_ANY (the stand-in on every address of the Mac)"

# The proxy. On the Mac it dials any public host, as an open main's boundary would; in the
# analogue an origin on this machine stands for the internet, under a name only the proxy knows.
if [ "$LINUX" = 1 ]; then
  python3 -I -u -m http.server --bind 127.0.0.1 0 --directory "$W" > "$W/origin.log" 2>&1 &
  ORIGIN_PID=$!
  i=0; ORIGIN_PORT=""
  while [ -z "$ORIGIN_PORT" ] && [ "$i" -lt 50 ]; do
    sleep 0.1; i=$((i + 1))
    ORIGIN_PORT="$(sed -n 's/.*port \([0-9]*\).*/\1/p' "$W/origin.log" | head -1)"
  done
  [ -n "$ORIGIN_PORT" ] || kit_fail "the analogue's origin did not start"
  de_proxy_start "$D" --allow origin.silokit.test --map origin.silokit.test=127.0.0.1 || kit_fail "the proxy did not start"
else
  de_proxy_start "$D" --allow-public || kit_fail "the proxy did not start: $(cat "$D/proxy.log")"
fi

# --- the first start, on Apple's NAT: the relay goes in (#382's recommended way in) ----------------
if [ "$LINUX" = 0 ]; then
  B="$(vm_fresh n40)" || exit 1
  guest_forget_hosts
  # shellcheck disable=SC2046
  vm_start "$B" $(vm_provision_args) >/dev/null
  kit_log "first start on Apple's NAT: provisioning, then the programs and the relay go in"
  s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_outcome inconclusive "the computer did not answer SSH on Apple's NAT"; exit 0; }
  kit_measure ssh_on_nat_after_start "$s" s "the first start, provisioned"
  GB="$B"
  # The programs a closed main would run, installed while the computer still has a road.
  NODE_V=v26.13.0; NODE_SHA=d38bd5e5a2ddce6bbb5ce45e46e208337da4d51fffe611e567fd32073d6439f6
  inst="$(guest_sh "mkdir -p \$HOME/silokit && cd \$HOME/silokit
[ -x node/bin/node ] || { curl -fsSL -o node.tgz https://nodejs.org/dist/$NODE_V/node-$NODE_V-darwin-arm64.tar.gz && shasum -a 256 node.tgz | grep -q $NODE_SHA && tar xzf node.tgz && mv node-$NODE_V-darwin-arm64 node && rm -f node.tgz; }
mkdir -p \$HOME/.local/bin && for t in node npm npx; do ln -sf \$HOME/silokit/node/bin/\$t \$HOME/.local/bin/\$t; done
node --version" 2>&1 | tail -1)"
  kit_fact node_inside "$inst"
  kit_fact claude_inside "$(guest_install_claude 2>&1 | tail -1)"
  kit_fact codex_inside "$(guest_install_codex 2>&1 | tail -1)"
  kit_fact clt_inside "$(guest_sh 'xcode-select -p 2>/dev/null || echo none')"
  if ! de_install_relay "$B" > "$W/install.log" 2>&1; then
    kit_attach "$W/install.log"
    kit_outcome inconclusive "the relay did not go in on Apple's NAT: $(tail -2 "$W/install.log" | tr '\n' ' ')"; exit 0
  fi
  kit_note "relay installed; the card's address is set; stopping: $(vm_stop "$B" 90)"
fi

# --- on the card ---------------------------------------------------------------------------------
s="$(de_boot "$B" "$D")" || {
  [ -n "$B" ] && kit_attach "$B/run.jsonl"
  kit_outcome relay-fails "SSH over vsock never answered after a start on the card (silovm's vsock events and the card's counts are attached)"
  kit_attach "$D/de-counts.json" 2>/dev/null; exit 0; }
kit_measure ssh_over_vsock_after_start "$s" s "a start on the card, SSH through vsockrelay"

PX="http://127.0.0.1:$DE_PROXY_PORT"
ENVP="HTTPS_PROXY=$PX HTTP_PROXY=$PX ALL_PROXY=$PX https_proxy=$PX http_proxy=$PX all_proxy=$PX NO_PROXY=localhost,127.0.0.1 no_proxy=localhost,127.0.0.1 NODE_USE_ENV_PROXY=1"
# Each program: name, the host it should ask the proxy for, its binary, the command (60 s at most).
if [ "$LINUX" = 1 ]; then
  O="origin.silokit.test:$ORIGIN_PORT"
  PROGS="curl|origin.silokit.test|curl|curl -sS -m 20 -o /dev/null http://$O/
python|origin.silokit.test|python3|python3 -c 'import urllib.request; urllib.request.urlopen(\"http://$O/\", timeout=20)'
git|origin.silokit.test|git|git ls-remote http://$O/none.git"
  MODES="none env"
else
  PROGS="curl|registry.npmjs.org|curl|curl -sS -m 30 -o /dev/null https://registry.npmjs.org/
git|github.com|git|git ls-remote https://github.com/expressjs/express HEAD
npm|registry.npmjs.org|npm|npm view left-pad version --fetch-timeout=30000 --fetch-retries=0
node-fetch|registry.npmjs.org|node|node -e 'fetch(\"https://registry.npmjs.org/\").then(r=>console.log(r.status))'
pip|pypi.org|python3|python3 -m pip download --no-deps -q -d /tmp/silokit-pip six
claude|api.anthropic.com|claude|ANTHROPIC_API_KEY=sk-ant-silokit-invalid claude -p hello
codex|api.openai.com|codex|OPENAI_API_KEY=sk-silokit-invalid codex exec --skip-git-repo-check hello
brew|formulae.brew.sh|brew|brew info --json=v2 jq
xcodebuild|developerservices2.apple.com|xcodebuild|xcodebuild -downloadPlatform iOS -exportPath /tmp/silokit-x"
  MODES="none env system both"
fi
system_proxy() { # on|off
  [ "$LINUX" = 1 ] && return 0
  de_root "$D" "svc=\$(cat /var/db/silokit-netsvc); networksetup -setwebproxy \"\$svc\" 127.0.0.1 $DE_PROXY_PORT; networksetup -setsecurewebproxy \"\$svc\" 127.0.0.1 $DE_PROXY_PORT; networksetup -setwebproxystate \"\$svc\" $1; networksetup -setsecurewebproxystate \"\$svc\" $1" >/dev/null 2>&1
}
count_host() { grep -c "\"host\": \"$1\"" "$D/proxy.jsonl" 2>/dev/null || true; }

: > "$W/programs.txt"
any_prog=""; curl_env=""
for mode in $MODES; do
  case "$mode" in system|both) system_proxy on ;; *) system_proxy off ;; esac
  pre=""; case "$mode" in env|both) pre="$ENVP " ;; esac
  while IFS='|' read -r name host bin cmd; do
    [ -n "$name" ] || continue
    if ! de_sh "$D" "command -v $bin >/dev/null 2>&1" ; then
      echo "$mode $name not-installed" >> "$W/programs.txt"; continue
    fi
    if [ "$name" = git ] || [ "$name" = pip ] || [ "$name" = xcodebuild ]; then
      if [ "$LINUX" = 0 ] && ! de_sh "$D" "xcode-select -p >/dev/null 2>&1"; then
        echo "$mode $name not-installed" >> "$W/programs.txt"; continue
      fi
    fi
    before="$(count_host "$host")"
    de_sh "$D" "$GUEST_LIMIT cd /tmp && $pre limit 60 $cmd" > /dev/null 2>&1; rc=$?
    after="$(count_host "$host")"
    via="no"; [ "${after:-0}" -gt "${before:-0}" ] && via="proxy"
    echo "$mode $name exit=$rc via=$via" >> "$W/programs.txt"
    kit_fact "program.$mode.$name" "exit $rc, $([ "$via" = proxy ] && echo "through the proxy ($host)" || echo "never reached the proxy")"
    if [ "$via" = proxy ]; then
      [ "$name" = curl ] && curl_env=1
      case "$name" in claude|codex|python|git) any_prog="$any_prog $mode:$name" ;; esac
    fi
  done <<EOF
$PROGS
EOF
done
system_proxy off
kit_attach "$W/programs.txt"

# --- root tries every other way out ---------------------------------------------------------------
before_frames="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("frames",0))' "$D/de-counts.json" 2>/dev/null || echo 0)"
if [ "$LINUX" = 1 ]; then
  PING="ping -c 2 -W 3"; NCZ="nc -z -w 3"; ALIAS="ip addr add 192.168.64.77/24 dev tap0 && ip route add 192.168.64.0/24 dev tap0 2>/dev/null"
  ROUTE="ip route replace default via $DE_ROUTER"
else
  PING="ping -c 2 -t 3"; NCZ="nc -z -G 3"; ALIAS="ifconfig en0 alias 192.168.64.77 netmask 255.255.255.0"
  ROUTE="route -n add -net 0.0.0.0/1 $DE_ROUTER"
fi
out="$(de_root "$D" "
try() { n=\$1; shift; if \"\$@\" >/dev/null 2>&1; then echo \"\$n reached\"; else echo \"\$n no\"; fi; }
id -u
try ping-router $PING $DE_ROUTER
try ping-internet $PING 1.1.1.1
${LAN:+try ping-mac $PING $LAN}
${LAN:+try mac-service $NCZ $LAN $P_ANY}
try dns-public sh -c 'dig +time=2 +tries=1 @1.1.1.1 example.com A 2>/dev/null | grep -q "status: NOERROR"'
try tcp-internet curl -s -m 5 -o /dev/null http://1.1.1.1/
$ROUTE >/dev/null 2>&1
try route-then-internet curl -s -m 5 -o /dev/null http://1.1.1.1/
$ALIAS >/dev/null 2>&1
try alias-then-nat-gateway $NCZ 192.168.64.1 22
${LAN:+try alias-then-mac-service curl -s -m 5 -o /dev/null http://$LAN:$P_ANY/}
")" || true
echo "$out" > "$W/root-tries.txt"; kit_attach "$W/root-tries.txt"
sleep 2
leaks=""
for k in $(echo "$out" | awk '$2 == "reached" {print $1}'); do leaks="$leaks $k"; done
for k in $(echo "$out" | awk '$2 == "no" {print $1}'); do kit_fact "root.$k" "ended at the card"; done
seen="$(python3 -c '
import json, sys
n = 0
try:
    for line in open(sys.argv[1]): n += 1
except OSError: pass
print(n)' "$W/standin/standin.log")"
kit_fact mac_standin_connections "$seen"
[ "$seen" = 0 ] || leaks="$leaks mac-standin-saw-$seen"
python3 - "$D/de-counts.json" "$before_frames" > "$W/card.txt" <<'EOF'
import json, sys
c = json.load(open(sys.argv[1]))
print("frames %d (%d during root's tries), ARP answered %d, other ARP %d" % (c.get("frames", 0), c.get("frames", 0) - int(sys.argv[2]), c.get("arp_answered", 0), c.get("arp_other", 0)))
print("dropped " + ", ".join("%s %d" % kv for kv in sorted(c.get("dropped", {}).items())))
top = sorted(c.get("ip_destinations", {}).items(), key=lambda kv: -kv[1])[:12]
print("destinations " + ", ".join("%s (%d)" % kv for kv in top))
EOF
kit_attach "$D/de-counts.json"
kit_fact card_frames "$(sed -n 1p "$W/card.txt")"
kit_fact card_dropped "$(sed -n 2p "$W/card.txt")"
kit_fact card_destinations "$(sed -n 3p "$W/card.txt")"
kit_attach "$D/proxy.jsonl" 2>/dev/null

# --- the control: nobody answers the router's ARP ------------------------------------------------
de_down "$B" "$D"
if de_boot "$B" "$D" --no-arp >/dev/null; then
  if [ "$LINUX" = 1 ]; then
    reach="n/a on Linux"
  else
    reach="$(de_sh "$D" 'scutil -r www.apple.com 2>&1 | head -1')"
  fi
  before="$(count_host "$([ "$LINUX" = 1 ] && echo origin.silokit.test || echo api.anthropic.com)")"
  if [ "$LINUX" = 1 ]; then
    de_sh "$D" "$ENVP curl -sS -m 20 -o /dev/null http://origin.silokit.test:$ORIGIN_PORT/" >/dev/null 2>&1
  else
    de_sh "$D" "$GUEST_LIMIT $ENVP limit 60 env ANTHROPIC_API_KEY=sk-ant-silokit-invalid claude -p hello" >/dev/null 2>&1
  fi
  after="$(count_host "$([ "$LINUX" = 1 ] && echo origin.silokit.test || echo api.anthropic.com)")"
  kit_fact no_arp_reachability "$reach"
  kit_fact no_arp_program_through_proxy "$([ "${after:-0}" -gt "${before:-0}" ] && echo yes || echo no)"
else
  kit_fact no_arp_reachability "the computer did not answer SSH with no ARP answer"
fi
if [ "$LINUX" = 0 ]; then
  de_sh "$D" 'scutil -r www.apple.com' >/dev/null 2>&1 || true
fi

where="$([ "$LINUX" = 1 ] && echo "Linux analogue: " || echo "")"
if [ -n "$leaks" ]; then
  kit_outcome leaks "${where}reached without the proxy:$leaks"
elif [ -z "$curl_env" ]; then
  kit_outcome relay-fails "${where}curl with the proxy variables never reached the proxy: the relay out (127.0.0.1:$DE_PROXY_PORT to vsock port $DE_PROXY_PORT) does not carry"
elif [ -z "$any_prog" ]; then
  kit_outcome relay-fails "${where}curl reached the proxy, but no agent program did in any setting"
else
  kit_outcome closed-holds "${where}SSH came in over vsock in ${s:-?} s; through the proxy:$any_prog; every try by root ended at the card ($(sed -n 1p "$W/card.txt"))"
fi
