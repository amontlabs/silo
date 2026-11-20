#!/bin/zsh
# shot.sh <film dir> <query…>: one still per query of the built page, the way check.sh takes them, into
# <film>/out/shots/<query>.png. A query with embed=1 gives the stage alone at 1280×720; any other query gives
# the whole page at 1100 wide. Examples: "t=21.5&embed=1"  "scene=2&theme=dark&embed=1"  "theme=dark"
# Uses Playwright's headless shell, one run at a time, and lets each run exit by itself.
set -u
ch=${1:?usage: shot.sh <film dir> <query…>}; ch=${ch:A}
W=${0:A:h:h}   # films/
# A film is a folder directly under films/ that holds a film.json (${ch:A} resolves links); nothing else is used.
[[ ${ch:h} == $W && -f $ch/film.json && ${ch:t} != (kit|player|tools|.*) ]] || { echo "FAIL: $1 is not a film: a film is a folder directly under $W that holds a film.json"; exit 1; }
[[ ! -e $ch/out || $ch/out == ${${:-$ch/out}:A} ]] || { echo "FAIL: $ch/out leads outside the film folder"; exit 1; }
shift
B=~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell
[[ -f $ch/out/index.html ]] || { echo "build first: tools/build.py $ch"; exit 1; }
mkdir -p $ch/out/shots
for q in "$@"; do
  size=1100,9000; [[ $q == *embed=1* ]] && size=1280,720
  f=$ch/out/shots/${${q//=/-}//&/_}.png
  "$B" --virtual-time-budget=6000 --screenshot="$f" --window-size=$size --hide-scrollbars --allow-file-access-from-files "file://$ch/out/index.html?$q" >/dev/null 2>&1
  [[ -f $f ]] && echo "$f" || echo "FAIL: no still for $q"
done
