#!/bin/bash
# The preview forwarder of #384 against Chromium: for each answer app.py can give, the page runs
# under Silo's policy or is not shown. A request that reaches the sink is the policy broken.
# usage: test.sh [silo_preview.py]. Needs Python 3, Node and Playwright's Chromium. Exits 1 on a leak.
H=$(cd "$(dirname "$0")"; pwd)
F=${1:-$H/../../2026-11-25-port-previews/prototype/silo_preview.py}
W=$(mktemp -d); trap 'kill $FP $SP 2>/dev/null; rm -rf $W' EXIT
echo '{"computers": {"c1a7": {"name": "c1", "ports": {"3000": "127.0.0.1:3100"}}}}' > $W/config.json
CONFIG=$W/config.json LOG=$W/log.jsonl python3 "$F" > /dev/null 2>&1 & FP=$!
python3 $H/sink.py 7999 $W/sink.txt & SP=$!
sleep 0.7
for mode in crlf lf interim hints fold; do
  python3 $H/app.py 3100 $mode & A=$!; sleep 0.5
  printf '%-8s %s\n' $mode "$(cd $H && NODE_PATH=$(npm root -g) node csp.js)"
  kill $A; wait $A 2>/dev/null
done
echo "requests that reached the host the policy forbids: $(cat $W/sink.txt 2>/dev/null | tr '\n' ' ')"
[ ! -s $W/sink.txt ]
