#!/bin/bash
# Stops what up.sh started and removes its namespaces.
OUT=$1; . $OUT/env 2>/dev/null
kill -TERM $(cat $OUT/recorder.pid 2>/dev/null) 2>/dev/null; sleep 0.3
kill $(cat $OUT/boundary.pid 2>/dev/null) 2>/dev/null
for p in $(tac $OUT/pids); do kill $p 2>/dev/null; done
sleep 0.3
for n in $NS1 $NS2; do ip netns del $n 2>/dev/null; rm -rf /etc/netns/$n; done
true
