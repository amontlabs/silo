#!/bin/bash
# Claude Code in each computer, holding only its placeholder: c1 with #378's proxy settings, c2
# with none. Prints what Claude Code said, what reached the stand-in host, and the record's lines.
# usage: program.sh <out>      (a run started by up.sh)
R=$(cd $(dirname $0); pwd); O=$1; . $O/env
SILOREC=$(dirname $(dirname $R))/2026-11-25-record-format/proto/zig-out/bin/silorec
for c in c1 c2; do
  mkdir -p $O/work-$c
  before=$(wc -l < $O/hosts.log 2>/dev/null || echo 0)
  start=$(date +%s.%N)
  $R/in.sh $O $c bash -c "cd $O/work-$c && ANTHROPIC_API_KEY=sk-ant-api03-silo-placeholder-$c-0000 IS_SANDBOX=1 DISABLE_AUTOUPDATER=1 timeout 300 claude -p 'Do the task.' --dangerously-skip-permissions --output-format json --max-turns 4 --model claude-opus-5-5" > $O/claude-$c.json 2> $O/claude-$c.err
  rc=$?
  echo "$c: claude exit=$rc in $(python3 -c "import time;print(round(time.time()-$start,1))")s, result: $(python3 -c "import json;d=json.load(open('$O/claude-$c.json'));print(d.get('subtype'), d.get('num_turns'), repr(d.get('result',''))[:80])" 2>&1 | tail -1)"
  echo "$c: what reached the hosts: $(tail -n +$((before+1)) $O/hosts.log | python3 -c "import sys,json,collections;print(dict(collections.Counter(json.loads(l)['cred']+'@'+json.loads(l)['host'] for l in sys.stdin)))")"
  echo "$c: refused by the boundary: $(grep '"kind":"refused"' $O/trace.jsonl | python3 -c "import sys,json,collections;print(dict(collections.Counter((json.loads(l).get('host') or json.loads(l).get('sni') or '')+' '+json.loads(l)['why'] for l in sys.stdin)))")"
done
sleep 1
for c in c1 c2; do echo "record $c: $($SILOREC verify $O/record/$c $c)"; $SILOREC print $O/record/$c | python3 -c "import sys,json,collections;print(dict(collections.Counter(json.loads(l)['kind']+'/'+json.loads(l).get('what','') for l in sys.stdin)))"; done
