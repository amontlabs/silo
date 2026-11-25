#!/bin/bash
# One night and its morning. Prints the morning as a transcript.
#   CODEX_BIN=... ./demo.sh <folder>
set -u
HERE=$(cd "$(dirname "$0")"; pwd)
export SILO_NIGHT=$(realpath -m "$1")
python3 "$HERE/night.py" "$SILO_NIGHT" > /dev/null
silo() { echo; echo "\$ silo $*"; "$HERE/silo" "$@"; }
first=$("$HERE/silo")
alert=$(echo "$first" | grep -o 'silo putback [^]]*' | head -1 | cut -d' ' -f3)
silo
silo receipt claude#1
silo chain "$alert"
silo timeline main --around "$alert"
silo receipt claude#2
silo receipt codex
silo network
silo diff main
silo diff try-bun
silo putback "$alert"
silo why '~/work/shared-fixtures/set-0/case-000.json'
silo chain "$alert" --json
silo verify
