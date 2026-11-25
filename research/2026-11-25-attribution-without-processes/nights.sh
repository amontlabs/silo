#!/bin/bash
# The nights this study reads. Each is #371's night with the prototype copied and changed by
# patch_proto.py. Needs what #371 needs: Python 3, Node, strace, claude on the PATH, CODEX_BIN.
#   ./nights.sh <folder> [one night's name]
set -u
HERE=$(cd "$(dirname "$0")"; pwd)
OUT=$(realpath -m "$1"); mkdir -p "$OUT"
ONLY=${2:-}
night() {  # name, patch flags, env...
  [ -n "$ONLY" ] && [ "$ONLY" != "$1" ] && return
  local name=$1 flags=$2; shift 2
  python3 "$HERE/patch_proto.py" "$OUT/proto-$name" $flags > /dev/null
  rm -rf "$OUT/$name"
  env "$@" python3 "$OUT/proto-$name/night.py" "$OUT/$name" > "$OUT/$name.log" 2>&1
  python3 "$OUT/proto-$name/night.py" "$OUT/$name" --end > /dev/null 2>&1
  pkill -f "$OUT/$name/" 2>/dev/null; pkill -f scripts/sessiond.js 2>/dev/null   # what --end leaves
  echo "$name: $(ls "$OUT/$name/record" | tr '\n' ' ')"
}
night exec-command  --fix
night code-mode     ""    SILO_CODEX_MODEL=gpt-6.2-sol CODE_MODE_PRINT=output
night code-mode-fix --fix SILO_CODEX_MODEL=gpt-6.2-sol CODE_MODE_PRINT=output
night code-mode-json --fix SILO_CODEX_MODEL=gpt-6.2-sol CODE_MODE_PRINT=json
night one-main "--fix --one-main"
night one-main-code-mode "--fix --one-main" SILO_CODEX_MODEL=gpt-6.2-sol CODE_MODE_PRINT=output
