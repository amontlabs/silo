#!/bin/bash
# One real Gemini CLI run, "signed in with Google" by a made-up access token, against
# fake_code_assist.py, with its OpenTelemetry export pointed at otlp_collector.py.
# usage: [KILL_AFTER=<s>] [STEPS=n] [TLS=1] [RELAY=1] run_gemini.sh <out-dir> [extra env]
# TLS=1 serves the stand-in over HTTPS with a throwaway authority. RELAY=1 puts relay.py, which
# passes the encrypted bytes on unread as a computer set for a plan would, between the two.
# KILL_AFTER sends SIGKILL to Gemini CLI after that many seconds.
set -u
HERE=$(cd "$(dirname "$0")"; pwd); GEMINI=${GEMINI:-gemini}
OUTD=$(mkdir -p "$1"; cd "$1"; pwd); shift
rm -f "$OUTD"/*.jsonl; H=$(mktemp -d); W=$(mktemp -d)
PORT=$((8500 + RANDOM % 400)); OPORT=$((4500 + RANDOM % 400)); RPORT=$((9000 + RANDOM % 400))
mkdir -p "$H/.gemini"
echo '{"privacy":{"usageStatisticsEnabled":false},"general":{"disableAutoUpdate":true}}' > "$H/.gemini/settings.json"
EXTRA=(); SCHEME=http
if [ -n "${TLS:-}" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$H/k.pem" -out "$H/c.pem" -days 1 \
    -subj /CN=localhost -addext subjectAltName=DNS:localhost 2>/dev/null
  EXTRA=(TLS_CERT=$H/c.pem TLS_KEY=$H/k.pem); SCHEME=https
fi
env OUT=$OUTD PORT=$PORT STEPS=${STEPS:-6} DELAY=${DELAY:-0.3} DAILY=${DAILY:-1500} "${EXTRA[@]}" python3 "$HERE/fake_code_assist.py" & SP=$!
OUT=$OUTD PORT=$OPORT python3 "$HERE/otlp_collector.py" & OP=$!
EP=$SCHEME://localhost:$PORT
if [ -n "${RELAY:-}" ]; then
  OUT=$OUTD LISTEN=$RPORT TARGET=$PORT python3 "$HERE/relay.py" & RP=$!; EP=$SCHEME://localhost:$RPORT
fi
sleep 0.8
cd "$W"; git init -q
env -i PATH=/opt/node22/bin:/usr/local/bin:/usr/bin:/bin HOME=$H TERM=dumb \
  GOOGLE_GENAI_USE_GCA=true GOOGLE_CLOUD_ACCESS_TOKEN=ya29.made-up-for-a-test \
  CODE_ASSIST_ENDPOINT=$EP ${TLS:+NODE_EXTRA_CA_CERTS=$H/c.pem} GEMINI_CLI_TRUST_WORKSPACE=true \
  GEMINI_TELEMETRY_ENABLED=true GEMINI_TELEMETRY_TARGET=local \
  GEMINI_TELEMETRY_OTLP_ENDPOINT=http://127.0.0.1:$OPORT GEMINI_TELEMETRY_OTLP_PROTOCOL=http \
  "$@" timeout ${KILL_AFTER:+-s KILL} ${KILL_AFTER:-240} "$GEMINI" -p 'Do the task.' --yolo \
  ${MODEL:+-m $MODEL} --output-format json < /dev/null > "$OUTD/gemini.out" 2> "$OUTD/gemini.err"
echo "gemini exit=$?"
sleep 3; kill $SP $OP ${RP:-} 2>/dev/null; rm -rf "$H" "$W"
