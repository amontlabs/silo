#!/bin/bash
# Inside the container: the night, then the morning, written to /out.
#   in-container.sh --dry            the scripted night of the morning review (no key)
#   in-container.sh [night.py opts]  a real night: --hours, --max-usd, --stuck-after, --hold-limit
set -u
P=/silo/proto
N=/out/night
export SILO_NIGHT=$N
if [ "${1:-}" = "--dry" ]; then
  shift
  unset SILO_KEY_ANTHROPIC SILO_KEY_OPENAI
  python3 $P/night.py $N > /out/night.log 2>&1
else
  [ -n "${SILO_KEY_ANTHROPIC:-}" ] || { echo "no Anthropic key: the Claude Code runs would all be refused" >&2; exit 2; }
  python3 $P/night.py $N --real "$@" > /out/night.log 2>&1
fi
echo "night over at $(date -u +%H:%M) UTC; writing the morning"
silo() { echo; echo "\$ silo $*"; $P/silo "$@"; }
{
  silo
  for r in $(python3 -c "
import sys; sys.path.insert(0, '$P'); import classify
for r in classify.View('$N/record').runs: print(r['name'].replace(' ', ''))"); do silo receipt "$r"; done
  first=$($P/silo)
  alert=$(echo "$first" | grep -o 'silo putback [^]]*' | head -1 | cut -d' ' -f3)
  [ -n "$alert" ] && silo chain "$alert"
  silo network
  silo diff main
  silo verify
} > /out/morning.txt 2>&1
python3 $P/page.py /out/morning.html >> /out/night.log 2>&1
SILO_PROTO=$P python3 /silo/kit/caught.py $N > /out/caught.json 2>> /out/night.log
python3 $P/night.py $N --end
echo "done: morning.txt, morning.html, caught.json"
