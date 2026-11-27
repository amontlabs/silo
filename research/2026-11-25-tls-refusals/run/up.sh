#!/bin/bash
# Starts one run of #395's set-up with computer c1 only (the CONNECT way in through #378's relay):
# the stand-in providers and hosts, the recorder, and a boundary. Every TLS case here ends in the
# same function of the boundary on both ways in, so c2 and gvproxy are left out.
# usage: up.sh <out> [boundary binary]          (as root, after ../2026-11-25-boundary-end-to-end/run/tls.sh)
set -eu
R=$(cd $(dirname $0); pwd); E=$(cd $R/../../2026-11-25-boundary-end-to-end; pwd); TLS=$E/run/tls
rm -rf $1; OUT=$(mkdir -p $1; cd $1; pwd)
BOUNDARY=${2:-$(cd $R/..; pwd)/../2026-11-25-boundary-end-to-end/boundary/zig-out/bin/boundary}
python3 $E/run/config.py $OUT finish "" "${EXTRA:-{\}}"
: > $OUT/pids
bg() { "$@" & echo $! >> $OUT/pids; }
OUT=$OUT SCENARIO=$E/../2026-11-24-spend-budgets/run/scenarios/short.json TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=8443 \
  bg python3 -I $E/../2026-11-24-spend-budgets/fake/provider.py 2> $OUT/provider.err
CONFIG=$OUT/config.json OUT=$OUT PROVIDER=127.0.0.1:8443 UP_CA=$TLS/up-ca.crt TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=9443 SSH_PORT=2022 \
  bg python3 -I $E/standin/hosts.py 2> $OUT/hosts.err
mkdir -p $OUT/record
bg $E/recorder/zig-out/bin/recorder $OUT/config.json $OUT/record $OUT/recorder.sock 2> $OUT/recorder.err; echo $! > $OUT/recorder.pid
SILO_CONFIG=$OUT/config.json RECORD=$OUT/trace.jsonl bg $BOUNDARY 2> $OUT/boundary.err
echo $! > $OUT/boundary.pid
sleep 0.5
ip netns add c1-$$ ; echo "c1-$$" > $OUT/ns1
mkdir -p /etc/netns/c1-$$; printf '127.0.0.1 localhost\n' > /etc/netns/c1-$$/hosts; : > /etc/netns/c1-$$/resolv.conf
ip netns exec c1-$$ ip link set lo up
NODELAY=1 SOCK=$OUT/c1.connect.sock READY=$OUT/relay.ready bg ip netns exec c1-$$ python3 -I $E/run/relay.py
for i in $(seq 1 20); do [ -f $OUT/relay.ready ] && break; sleep 0.1; done
printf 'NS1=c1-%s\nNS2=none\nTLS=%s\nOUT=%s\n' $$ $TLS $OUT > $OUT/env
echo up: $OUT
