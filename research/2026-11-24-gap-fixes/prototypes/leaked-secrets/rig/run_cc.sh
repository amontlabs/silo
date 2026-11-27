#!/bin/bash
# One run of the real Claude Code against the fake Anthropic API of research/2026-11-24-experiments,
# in a clone of this repository, with made-up credentials planted. Writes <out>/requests.jsonl and
# keeps Claude Code's own transcript under <out>/home/.claude.
# usage: run_cc.sh <out> [path to fake_anthropic.py]
set -u
HERE=$(cd "$(dirname "$0")"; pwd)
OUTD=$1; FAKE=${2:-/mnt/project-files/research/2026-11-24-experiments/fake_anthropic.py}
rm -rf "$OUTD"; mkdir -p "$OUTD/home"
python3 "$HERE/make_scenario.py" "$OUTD"
rm -rf /work/g5repo; git clone -q "$HERE/../../../../.." /work/g5repo
cp "$OUTD/dotenv" /work/g5repo/.env
PORT=$((8100 + RANDOM % 800))
OUT=$OUTD SCENARIO=$OUTD/scenario.json PORT=$PORT python3 "$FAKE" & SP=$!
sleep 0.8
cd /work/g5repo
HOME=$OUTD/home CLAUDE_CONFIG_DIR=$OUTD/home/.claude ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT \
  ANTHROPIC_API_KEY=sk-ant-api03-fake IS_SANDBOX=1 DISABLE_TELEMETRY=1 \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 \
  env -u CLAUDECODE -u CLAUDE_CODE_ENTRYPOINT -u CLAUDE_CODE_SSE_PORT \
  timeout 300 claude -p "$(cat "$OUTD/prompt.txt")" --dangerously-skip-permissions \
  --output-format stream-json --verbose < /dev/null > "$OUTD/cc.jsonl" 2> "$OUTD/cc.err"
echo "exit=$?"
kill $SP
wc -l "$OUTD/requests.jsonl"
