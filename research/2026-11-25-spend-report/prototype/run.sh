#!/bin/bash
# One real Claude Code run on a computer set for a Claude plan: its Anthropic traffic goes through
# passthrough.py unread (TLS end to end, to fake_anthropic_usage.py), and its OpenTelemetry export
# goes to listener.py, as Silo would set it up. Built on #407's run_otel.sh.
# usage: [KILL_AFTER=<s>] [PROJECT_SETTINGS=<json>] [USER_SETTINGS=<json>] [MANAGED_SETTINGS=<json>]
#        run.sh <scenario.json> <out-dir> [VAR=value ...]
# VAR=value pairs go into Claude Code's environment after Silo's own, as an agent could set them.
# PORT in a settings text becomes the listener's port. MANAGED_SETTINGS is written to /etc/claude-code/managed-settings.json for the run (needs root,
# as an agent with sudo has it) and removed after.
set -u
HERE=$(cd "$(dirname "$0")"; pwd)
SC=$(cd "$(dirname "$1")"; pwd)/$(basename "$1"); OUTD=$(mkdir -p "$2"; cd "$2"; pwd); shift 2
rm -f "$OUTD"/*.jsonl; H=$(mktemp -d); W=$(mktemp -d); T=$(mktemp -d)
openssl req -x509 -newkey rsa:2048 -nodes -keyout $T/key.pem -out $T/cert.pem -days 1 \
  -subj /CN=127.0.0.1 -addext subjectAltName=IP:127.0.0.1 2>/dev/null
PORT=$((8400 + RANDOM % 400)); RPORT=$((8800 + RANDOM % 400)); OPORT=$((4400 + RANDOM % 400))
OUT=$OUTD SCENARIO=$SC PORT=$PORT CERT=$T/cert.pem KEY=$T/key.pem python3 "$HERE/fake_anthropic_usage.py" & SP=$!
OUT=$OUTD LISTEN=$RPORT UPSTREAM=127.0.0.1:$PORT python3 "$HERE/passthrough.py" & RP=$!
OUT=$OUTD PORT=$OPORT python3 "$HERE/listener.py" & OP=$!
sleep 0.8
cd "$W"; git init -q
[ -n "${PROJECT_SETTINGS:-}" ] && mkdir -p .claude && echo "$PROJECT_SETTINGS" > .claude/settings.json
[ -n "${USER_SETTINGS:-}" ] && mkdir -p $H/.claude && echo "${USER_SETTINGS//PORT/$OPORT}" > $H/.claude/settings.json
if [ -n "${MANAGED_SETTINGS:-}" ]; then mkdir -p /etc/claude-code; echo "${MANAGED_SETTINGS//PORT/$OPORT}" > /etc/claude-code/managed-settings.json; fi
env -i PATH=/opt/node22/bin:/usr/local/bin:/usr/bin:/bin \
  HOME=$H CLAUDE_CONFIG_DIR=$H/.claude ANTHROPIC_BASE_URL=https://127.0.0.1:$RPORT NODE_EXTRA_CA_CERTS=$T/cert.pem \
  CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-made-up-for-a-test IS_SANDBOX=1 DISABLE_AUTOUPDATER=1 \
  CLAUDE_CODE_ENABLE_TELEMETRY=1 OTEL_METRICS_EXPORTER=otlp OTEL_LOGS_EXPORTER=otlp \
  OTEL_EXPORTER_OTLP_PROTOCOL=http/json OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:$OPORT \
  OTEL_METRIC_EXPORT_INTERVAL=1000 OTEL_LOGS_EXPORT_INTERVAL=500 SILO_OTLP_PORT=$OPORT "$@" \
  timeout ${KILL_AFTER:+-s KILL} ${KILL_AFTER:-240} claude -p 'Do the task.' --model claude-opus-5-5 --dangerously-skip-permissions \
  --output-format stream-json --verbose < /dev/null > "$OUTD/cc.jsonl" 2> "$OUTD/cc.err"
echo "claude exit=$?"
sleep 2; kill $SP $RP $OP 2>/dev/null
[ -n "${MANAGED_SETTINGS:-}" ] && rm -f /etc/claude-code/managed-settings.json
rm -rf "$H" "$W" "$T"
