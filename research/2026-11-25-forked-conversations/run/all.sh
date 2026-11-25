#!/usr/bin/env bash
# Runs every fork case with Claude Code and Codex, then writes results/<program>.md.
set -euo pipefail
R=$(cd "$(dirname "$0")/.." && pwd)
export OUTROOT=${OUTROOT:-/tmp/forkruns/out}
for p in cc cx; do "$R/run/scenarios.sh" $p; python3 "$R/analyse/analyse.py" "$OUTROOT/$p" > "$R/results/$p.md"; done
