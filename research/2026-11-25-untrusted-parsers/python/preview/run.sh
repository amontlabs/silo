#!/bin/bash
# usage: run.sh <silo_preview.py>   Runs the forwarder of #384 and checks its policy in Chromium,
# once with a well-formed answer from the app and once with a head ended by bare LFs.
H=$(cd $(dirname $0); pwd); W=/tmp/claude-0/prev; mkdir -p $W; rm -f $W/sink.txt
echo '{"computers": {"c1a7": {"name": "c1", "ports": {"3000": "127.0.0.1:3100"}}}}' > $W/config.json
CONFIG=$W/config.json LOG=$W/log.jsonl python3 $1 > /dev/null 2>&1 & F=$!
python3 $H/sink.py 7999 $W/sink.txt & S=$!
for mode in crlf lf; do
  python3 $H/app.py 3100 $mode & A=$!; sleep 0.7
  echo "$mode: $(node $H/csp.js)"; kill $A; wait $A 2>/dev/null
done
echo "requests that reached the host the policy forbids:"; cat $W/sink.txt 2>/dev/null
kill $F $S
