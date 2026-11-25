#!/bin/bash
# The night, the morning and its pictures. Needs what #371 needs (Python 3, Node, strace, claude on
# the PATH, CODEX_BIN), openssl, and Playwright for the pictures.
#   ./build.sh <work folder>            the night, then the pages (the readers' answers come from answers.json)
#   ./build.sh <work folder> --pages    the pages again from a night already run
set -eu
HERE=$(cd "$(dirname "$0")"; pwd)
W=$(realpath -m "$1"); mkdir -p "$W/pages"
if [ "${2:-}" != "--pages" ]; then
  python3 "$HERE/patch_night.py" "$W/proto-screen" > /dev/null
  python3 "$W/proto-screen/night.py" "$W/screen"
  sleep 30   # let the refusing Codex retry a while longer
  python3 "$W/proto-screen/night.py" "$W/screen" --end || true
  pkill -f "$W/screen/" || true; pkill -f scripts/sessiond.js || true
fi
P="$W/pages"
python3 "$HERE/morning.py" "$W/screen" "$P/morning.json"
python3 "$HERE/morning.py" "$W/screen" "$P/option-a.json" --no-probe
python3 "$HERE/morning.py" "$W/screen" "$P/injected.json" --inject "claude #1"
python3 "$HERE/states.py" "$P/morning.json" "$P"
python3 "$HERE/page.py" "$P/morning.json" "$P/morning.html" --other option-a.html
python3 "$HERE/page.py" "$P/option-a.json" "$P/option-a.html" --other morning.html
for s in injected state-empty state-hundreds; do python3 "$HERE/page.py" "$P/$s.json" "$P/$s.html"; done
mkdir -p "$HERE/pictures"
node "$HERE/shots.js" "$P" "$HERE/pictures"
