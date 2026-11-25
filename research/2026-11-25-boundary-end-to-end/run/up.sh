#!/bin/bash
# Starts one run: the stand-in providers (#374), the stand-in hosts, the recorder, the boundary,
# and two computers: c1 a network namespace with only its loopback and #378's relay, c2 a network
# namespace behind #380's patched gvproxy with the hand-off. Writes $OUT/pids and $OUT/env.
# usage: up.sh <out> [policy] [budget] [extra boundary JSON]       (as root, after run/tls.sh)
# GVPROXY_ARGS replaces the hand-off flags of gvproxy (default: #380's hand-off mode).
set -eu
R=$(cd $(dirname $0); pwd); E=$(dirname $R); TLS=$R/tls
rm -rf $1; OUT=$(mkdir -p $1; cd $1; pwd); shift
BIN=${BIN:-/root/e2e/bin}
python3 $R/config.py $OUT "${1:-finish}" "${2:-}" "${3:-{\}}"
: > $OUT/pids
bg() { "$@" & echo $! >> $OUT/pids; }
# The stand-ins.
OUT=$OUT SCENARIO=${SCENARIO:-$E/../2026-11-24-spend-budgets/run/scenarios/short.json} TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=8443 \
  bg python3 -I $E/../2026-11-24-spend-budgets/fake/provider.py 2> $OUT/provider.err
CONFIG=$OUT/config.json OUT=$OUT PROVIDER=127.0.0.1:8443 UP_CA=$TLS/up-ca.crt TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=9443 SSH_PORT=2022 \
  bg python3 -I $E/standin/hosts.py 2> $OUT/hosts.err
# The recorder and the boundary.
mkdir -p $OUT/record
[ -z "${NO_RECORDER:-}" ] && { bg $E/recorder/zig-out/bin/recorder $OUT/config.json $OUT/record $OUT/recorder.sock 2> $OUT/recorder.err; echo $! > $OUT/recorder.pid; }
SILO_CONFIG=$OUT/config.json RECORD=$OUT/trace.jsonl bg $E/boundary/zig-out/bin/boundary 2> $OUT/boundary.err
echo $! > $OUT/boundary.pid
sleep 0.5
# c1: only a loopback; its way out is the relay to the boundary's CONNECT socket.
ip netns add c1-$$ ; echo "c1-$$" > $OUT/ns1
mkdir -p /etc/netns/c1-$$; printf '127.0.0.1 localhost\n' > /etc/netns/c1-$$/hosts; : > /etc/netns/c1-$$/resolv.conf
ip netns exec c1-$$ ip link set lo up
SOCK=$OUT/c1.connect.sock READY=$OUT/relay.ready bg ip netns exec c1-$$ python3 -I $R/relay.py
# c2: gvproxy with the hand-off; its lookups go to the boundary.
GVA=${GVPROXY_ARGS:--no-gateway-api -no-host-loopback -outbound-deny private,host -outbound-handoff $OUT/c2.handoff.sock -dns-upstream 127.0.0.1:5302}
bg $BIN/gvproxy-silo -listen unix://$OUT/net.sock -mtu 1500 $GVA > $OUT/gvproxy.log 2>&1
sleep 0.5
ip netns add c2-$$ ; echo "c2-$$" > $OUT/ns2
mkdir -p /etc/netns/c2-$$; printf '127.0.0.1 localhost\n' > /etc/netns/c2-$$/hosts; echo "nameserver 192.168.127.1" > /etc/netns/c2-$$/resolv.conf
ip netns exec c2-$$ ip link set lo up
bg ip netns exec c2-$$ $BIN/gvforwarder -url unix://$OUT/net.sock -iface tap0 -stop-if-exist "" -mtu 1500 > $OUT/fwd.log 2>&1
for i in $(seq 1 40); do ip netns exec c2-$$ ip -4 addr show tap0 2>/dev/null | grep -q inet && break; sleep 0.25; done
for i in $(seq 1 20); do [ -f $OUT/relay.ready ] && break; sleep 0.1; done
cat > $OUT/env <<EV
NS1=c1-$$
NS2=c2-$$
TLS=$TLS
OUT=$OUT
EV
echo up: $OUT
