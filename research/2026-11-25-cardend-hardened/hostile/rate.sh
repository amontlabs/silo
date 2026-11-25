#!/bin/bash
# A hostile main sending one kind of frame for a while against one cardend, the rate every 5 s.
# usage: rate.sh <cardend binary> <hostile binary> <mode> <duration>
CARD=$1 HOST=$2 MODE=$3 DUR=$4
W=$(mktemp -d)
$CARD -frames $W/f.sock -counts $W/counts.json -dial $W/dial.sock 2>$W/log & P=$!
sleep 0.3
$HOST -card $W/f.sock -mode $MODE -for $DUR -pid $P -settle 1s
echo "{\"counts_bytes\":$(stat -c %s $W/counts.json)}"
kill $P; wait $P 2>/dev/null; rm -rf $W
