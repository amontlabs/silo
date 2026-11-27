#!/bin/bash
# A hostile main against one cardend binary: each mode against a fresh cardend in way-in mode.
# usage: run.sh <cardend binary> <hostile binary> <frames> <mode>...   prints one JSON line per mode
CARD=$1 HOST=$2 N=$3; shift 3
W=$(mktemp -d)
for m in "$@"; do
  $CARD -frames $W/f.sock -counts $W/counts.json -dial $W/dial.sock 2>$W/log & P=$!
  sleep 0.3
  line=$($HOST -card $W/f.sock -mode $m -n $N -pid $P)
  sleep 0.6
  sz=$(stat -c %s $W/counts.json 2>/dev/null || echo 0)
  kinds=$(python3 -c "import json;print(len(json.load(open('$W/counts.json'))))" 2>/dev/null || echo 0)
  alive=$(kill -0 $P 2>/dev/null && echo yes || echo no)
  kill $P 2>/dev/null; wait $P 2>/dev/null
  echo "${line%\}},\"counts_bytes\":$sz,\"count_lines\":$kinds,\"alive\":\"$alive\"}"
  rm -f $W/*
done
rmdir $W
