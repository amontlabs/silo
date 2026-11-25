#!/bin/bash
# Record one real Claude Code run against the fake API: the trace lands in <out>/trace.jsonl.
# usage: run_cc.sh <scenario.json> <out-dir> [extra env, e.g. CLAUDE_CODE_OAUTH_TOKEN=fake-oauth]
set -u
HERE=$(cd "$(dirname "$0")"; pwd)
SC=$(cd "$(dirname "$1")"; pwd)/$(basename "$1"); OUTD=$(mkdir -p "$2"; cd "$2"; pwd); shift 2
rm -f "$OUTD/trace.jsonl"; H=$(mktemp -d); W=$(mktemp -d)
PORT=$((8400 + RANDOM % 500))
OUT=$OUTD SCENARIO=$SC PORT=$PORT python3 "$HERE/fake_anthropic_usage.py" & SP=$!
sleep 0.7
cd "$W"; git init -q
# A clean environment: nothing of the session that runs this script reaches the program.
env -i PATH=/opt/node22/bin:/usr/local/bin:/usr/bin:/bin \
  HOME=$H CLAUDE_CONFIG_DIR=$H/.claude ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT \
  ${ANTHROPIC_API_KEY_FAKE:+ANTHROPIC_API_KEY=sk-ant-api03-fake-not-a-key} IS_SANDBOX=1 DISABLE_TELEMETRY=1 \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 "$@" \
  timeout 180 claude -p 'Do the task.' --model claude-opus-5-5 --dangerously-skip-permissions \
  --output-format stream-json --verbose < /dev/null > "$OUTD/cc.jsonl" 2> "$OUTD/cc.err"
echo "claude exit=$?"
kill $SP; rm -rf "$H" "$W"
