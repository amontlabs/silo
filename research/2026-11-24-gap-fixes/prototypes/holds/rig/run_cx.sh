#!/bin/bash
# One run of the real Codex CLI against the fake Responses API.
# usage: run_cx.sh <scenario.json> <outdir> <workdir>   (env: CODEX=path to codex, KILL_AFTER)
set -u
RIG=$(cd $(dirname $0); pwd)
SC=$(realpath $1); OUTD=$2; WD=$3; rm -rf $OUTD; mkdir -p $OUTD/home/.codex
PORT=${FAKE_PORT:-$((9100 + RANDOM % 800))}
cat > $OUTD/home/.codex/config.toml <<T
model = "gpt-5.5"
model_provider = "fake"
[model_providers.fake]
name = "fake"
base_url = "http://127.0.0.1:$PORT/v1"
wire_api = "responses"
env_key = "FAKE_KEY"
T
OUT=$OUTD SCENARIO=$SC PORT=$PORT python3 $RIG/fake_openai.py & SP=$!
sleep 0.7
cd $WD
T0=$(date +%s.%N)
timeout ${KILL_AFTER:-900} env -i PATH=/usr/local/bin:/usr/bin:/bin:$(dirname $(which node)) TERM=dumb HOME=$OUTD/home CODEX_HOME=$OUTD/home/.codex FAKE_KEY=sk-fake RUST_LOG=warn ${EXTRA_ENV:-} \
  ${CODEX:-codex} exec --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check 'Do the task.' < /dev/null > $OUTD/cx.jsonl 2> $OUTD/cx.err
echo "exit=$? wall=$(echo "$(date +%s.%N) - $T0" | bc)" >> $OUTD/log.txt
kill $SP
