#!/usr/bin/env bash
# One scenario: fresh moto, fresh proxy, terraform (or aws) through it.
#   ./run.sh <name> <holds.json|-> <command...>
# Expects terraform and a venv with moto and awscli under $OPT (default /opt/burst), which is
# removed after the runs. Traces go to traces/<name>.jsonl, the tool's output to outputs/<name>.txt.
set -u
OPT=${OPT:-/opt/burst}
HERE=$(cd "$(dirname "$0")" && pwd)
name=$1; holds=$2; shift 2
mkdir -p "$HERE/traces" "$HERE/outputs"
trace="$HERE/traces/$name.jsonl"; out="$HERE/outputs/$name.txt"
[ "${KEEP_TRACE:-0}" = 1 ] || rm -f "$trace" "$HERE/traces/$name.asks.jsonl"
if [ "${KEEP_MOTO:-0}" != 1 ]; then
  pkill -f "moto_server -p 5000" 2>/dev/null; sleep 0.5
  "$OPT/venv/bin/moto_server" -p 5000 >"$OPT/moto.log" 2>&1 &
fi
pkill -f "proxy.py --listen 4566" 2>/dev/null; sleep 0.5
hargs=(); [ "$holds" != - ] && hargs=(--holds "$holds"); [ -n "${POLICY:-}" ] && hargs+=(--policy "$POLICY" --asks "$HERE/traces/$name.asks.jsonl")
python3 "$HERE/proxy.py" --listen 4566 --upstream 127.0.0.1:5000 --log "$trace" "${hargs[@]}" 2>>"$OPT/proxy.log" &
for i in $(seq 50); do curl -s -o /dev/null http://127.0.0.1:5000/ && curl -s -o /dev/null http://127.0.0.1:4566/ && break; sleep 0.2; done
start=$(date +%s.%N)
( cd "${WORKDIR:-$OPT/work}" && env -i PATH="$OPT:$OPT/venv/bin:/usr/bin:/bin" HOME="$OPT/home" \
    TF_DATA_DIR="$OPT/tfdata" TF_PLUGIN_CACHE_DIR="$OPT/plugin-cache" TF_IN_AUTOMATION=1 \
    AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test AWS_DEFAULT_REGION=us-east-1 \
    TF_LOG="${TF_LOG:-}" TF_LOG_PATH="${TF_LOG_PATH:-}" "$@" ) >"$out" 2>&1
code=$?
end=$(date +%s.%N)
printf '\n[exit %s after %.1f s]\n' "$code" "$(echo "$end - $start" | bc)" >>"$out"
sleep 0.5
pkill -f "proxy.py --listen 4566" 2>/dev/null
echo "$name: exit $code, $(wc -l <"$trace") requests"
