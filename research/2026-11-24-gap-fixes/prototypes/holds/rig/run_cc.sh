#!/bin/bash
# One run of the real Claude Code against the fake Messages API, with no probe.
# usage: run_cc.sh <scenario.json> <outdir> <workdir>   (env: KILL_AFTER seconds, default 900)
set -u
RIG=$(cd $(dirname $0); pwd)
SC=$(realpath $1); OUTD=$2; WD=$3; rm -rf $OUTD; mkdir -p $OUTD/home
PORT=${FAKE_PORT:-$((9100 + RANDOM % 800))}
OUT=$OUTD SCENARIO=$SC PORT=$PORT python3 $RIG/fake_anthropic.py & SP=$!
sleep 0.7
cd $WD
T0=$(date +%s.%N)
# A clean environment: only what a user's shell would have, plus EXTRA_ENV (e.g. "BASH_MAX_TIMEOUT_MS=1200000").
timeout ${KILL_AFTER:-900} env -i PATH=/usr/local/bin:/usr/bin:/bin:$(dirname $(which claude)):$(dirname $(which node)) TERM=dumb \
  HOME=$OUTD/home CLAUDE_CONFIG_DIR=$OUTD/home/.claude ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT ANTHROPIC_API_KEY=sk-ant-api03-fake \
  IS_SANDBOX=1 DISABLE_TELEMETRY=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 ${EXTRA_ENV:-} \
  claude -p 'Do the task.' --dangerously-skip-permissions --output-format stream-json --verbose < /dev/null > $OUTD/cc.jsonl 2> $OUTD/cc.err
echo "exit=$? wall=$(echo "$(date +%s.%N) - $T0" | bc)" >> $OUTD/log.txt
kill $SP
