#!/bin/bash
# One real Claude Code run, signed in with a made-up plan token, against the fake API of #355
# (fake_anthropic_usage.py), with its OpenTelemetry export pointed at otlp_collector.py.
# usage: [KILL_AFTER=<s>] run_otel.sh <scenario.json> <out-dir> [extra env]
# KILL_AFTER sends SIGKILL to Claude Code after that many seconds, as a computer stopped hard.
set -u
HERE=$(cd "$(dirname "$0")"; pwd)
SC=$(cd "$(dirname "$1")"; pwd)/$(basename "$1"); OUTD=$(mkdir -p "$2"; cd "$2"; pwd); shift 2
rm -f "$OUTD/trace.jsonl" "$OUTD/otlp.jsonl"; H=$(mktemp -d); W=$(mktemp -d)
PORT=$((8400 + RANDOM % 500)); OPORT=$((4400 + RANDOM % 500))
OUT=$OUTD SCENARIO=$SC PORT=$PORT python3 "$HERE/fake_anthropic_usage.py" & SP=$!
OUT=$OUTD PORT=$OPORT python3 "$HERE/otlp_collector.py" & OP=$!
sleep 0.7
cd "$W"; git init -q
env -i PATH=/opt/node22/bin:/usr/local/bin:/usr/bin:/bin \
  HOME=$H CLAUDE_CONFIG_DIR=$H/.claude ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT \
  CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-made-up-for-a-test IS_SANDBOX=1 DISABLE_AUTOUPDATER=1 \
  CLAUDE_CODE_ENABLE_TELEMETRY=1 OTEL_METRICS_EXPORTER=otlp OTEL_LOGS_EXPORTER=otlp \
  OTEL_EXPORTER_OTLP_PROTOCOL=http/json OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:$OPORT \
  OTEL_METRIC_EXPORT_INTERVAL=1000 OTEL_LOGS_EXPORT_INTERVAL=500 "$@" \
  timeout ${KILL_AFTER:+-s KILL} ${KILL_AFTER:-180} claude -p 'Do the task.' --model claude-opus-5-5 --dangerously-skip-permissions \
  --output-format stream-json --verbose < /dev/null > "$OUTD/cc.jsonl" 2> "$OUTD/cc.err"
echo "claude exit=$?"
sleep 2; kill $SP $OP; rm -rf "$H" "$W"
