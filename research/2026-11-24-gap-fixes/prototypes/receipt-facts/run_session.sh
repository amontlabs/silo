#!/bin/bash
# One recorded session: the real Claude Code, driven by the fake model in scenario.json, with the
# bpftrace collector running system-wide.
# Usage: run_session.sh <outdir> [scenario.json] [project template dir]   (root)
set -u
HERE=$(cd "$(dirname "$0")"; pwd); OUTD=$(mkdir -p "$1"; cd "$1"; pwd); rm -f "$OUTD"/*.jsonl "$OUTD"/trace.txt
PROJ=$OUTD/project; rm -rf "$PROJ"; cp -r "${3:-$HERE/project_template}" "$PROJ"; cd "$PROJ"
git init -q && git config user.email a@example.invalid && git config user.name agent
[ -f pre-commit ] && mv pre-commit .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
git add -A && git commit -q --no-verify -m start
PORT=$((8100 + RANDOM % 800))
OUT=$OUTD PROJECT=$PROJ SCENARIO=${2:-$HERE/scenario.json} PORT=$PORT python3 "$HERE/fake_api.py" & API=$!
mountpoint -q /sys/kernel/tracing || mount -t tracefs nodev /sys/kernel/tracing
BPFTRACE_MAX_STRLEN=160 bpftrace "$HERE/collector.bt" > "$OUTD/trace.txt" 2> "$OUTD/bpftrace.err" & BT=$!
sleep 4   # bpftrace attaches
H=$OUTD/home; mkdir -p "$H"
env -i PATH="$PATH" HOME="$H" CLAUDE_CONFIG_DIR="$H/.claude" ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT \
  ANTHROPIC_API_KEY=sk-ant-api03-FAKE-not-a-key IS_SANDBOX=1 DISABLE_TELEMETRY=1 \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 \
  timeout 180 claude -p 'Fix the failing test and make sure all tests pass.' \
  --dangerously-skip-permissions --output-format stream-json --verbose < /dev/null > "$OUTD/cc.jsonl" 2> "$OUTD/cc.err"
echo "claude exit=$?" > "$OUTD/run.txt"
sleep 1; kill -INT $BT; wait $BT 2>/dev/null; kill $API
echo "trace lines: $(wc -l < "$OUTD/trace.txt")" >> "$OUTD/run.txt"; cat "$OUTD/run.txt"
