#!/bin/bash
# The relay inside a computer (#378, as #395 copied it) in front of a boundary that writes a reply in
# two pieces. NODELAY=0 is #378's relay as written; NODELAY=1 is the relay with TCP_NODELAY set on
# the program's connection. Passes when a kept-alive request takes under 10 ms.
# usage: test.sh 0|1      (as root: it makes a network namespace)
set -u
H=$(cd $(dirname $0); pwd); RELAY=$H/../../2026-11-25-boundary-end-to-end/run/relay.py
T=$(mktemp -d); NS=relay-$$
python3 -I $H/two_pieces.py $T/b.sock & B=$!
ip netns add $NS
NODELAY=$1 SOCK=$T/b.sock READY=$T/ready ip netns exec $NS python3 -I $RELAY & R=$!
for i in $(seq 50); do [ -f $T/ready ] && break; sleep 0.1; done
ip netns exec $NS python3 -I $H/client.py; rc=$?
kill $R $B 2>/dev/null; wait 2>/dev/null; ip netns del $NS; rm -rf "${T:?}"
echo "NODELAY=$1: $([ $rc = 0 ] && echo PASS || echo FAIL)"; exit $rc
