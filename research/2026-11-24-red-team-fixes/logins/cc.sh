#!/bin/bash
# Run Claude Code once in computer $1 with HOME $2: one model turn with a Bash call, then an answer.
N=$1; HM=$2; CA=$3
mkdir -p /tmp/cw/site; chown cuser /tmp/cw/site; cd /tmp/cw/site
ip netns exec $N runuser -u cuser -- env -i PATH=/opt/node22/bin:/usr/bin:/bin HOME=$HM NODE_EXTRA_CA_CERTS=$CA IS_SANDBOX=1 \
  DISABLE_TELEMETRY=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 \
  timeout 60 claude -p 'Do the task.' --dangerously-skip-permissions --output-format stream-json --verbose < /dev/null > $HM/out.jsonl 2> $HM/err.txt
echo "exit=$? result=$(tail -1 $HM/out.jsonl | python3 -c 'import json,sys
try: j=json.load(sys.stdin); print(j.get("subtype"), repr(j.get("result"))[:80])
except Exception as e: print("none")')"
