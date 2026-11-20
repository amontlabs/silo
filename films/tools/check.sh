#!/bin/zsh
# check.sh <film dir>: the film's checks. Builds the page first. Exits 1 if any check fails.
# Long output goes to <film>/out/check.log; the summary is printed.
set -u
ch=${1:?usage: check.sh <film dir>}; ch=${ch:A}
W=${0:A:h:h}   # films/
# A film is a folder directly under films/ that holds a film.json (${ch:A} resolves links); nothing else is used.
[[ ${ch:h} == $W && -f $ch/film.json && ${ch:t} != (kit|player|tools|.*) ]] || { echo "FAIL: $1 is not a film: a film is a folder directly under $W that holds a film.json"; exit 1; }
[[ ! -e $ch/out || $ch/out == ${${:-$ch/out}:A} ]] || { echo "FAIL: $ch/out leads outside the film folder"; exit 1; }
PY=$W/.venv/bin/python; export HF_HUB_OFFLINE=1 FILM_IN_VENV=1   # the venv has misaki; nothing reaches the network
[[ -x $PY ]] || { echo "FAIL: $PY is missing (see the README)"; exit 1; }
B=~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell
out=$ch/out; mkdir -p $out/stills; log=$out/check.log; : > $log
fail=()

echo "== 1. sources"
node $W/tools/check_sources.mjs $ch || fail+=(sources)

echo "== build"
$PY $W/tools/build.py $ch || { echo "FAIL: build"; exit 1; }
page="file://$out/index.html"

echo "== 2. player"
for v in $($PY -c 'import json,sys;print(" ".join(v["id"] for v in json.load(open(sys.argv[1]))["voices"]))' $ch/voice.json); do
  "$B" --virtual-time-budget=6000 --dump-dom --allow-file-access-from-files "$page?scene=0&voice=$v" 2>>$log > $out/dom.html
  $PY $W/tools/check_player.py $ch $v < $out/dom.html || fail+=(player-$v)
done

echo "== 3. page parts"
$PY $W/tools/check_page.py $ch < $out/dom.html || fail+=(page)
rm -f $out/dom.html

echo "== 4. stills"
n=$($PY -c 'import json,sys;print(len(json.load(open(sys.argv[1]))["scenes"]))' $ch/narration.json)
rm -f $out/stills/*.png(N)
for theme in light dark; do
  for i in $(seq 0 $((n - 1))); do
    "$B" --virtual-time-budget=6000 --screenshot="$out/stills/scene-$((i + 1))-$theme.png" --window-size=1280,720 --hide-scrollbars \
      --allow-file-access-from-files "$page?scene=$i&theme=$theme&embed=1" >>$log 2>&1
  done
done
"$B" --virtual-time-budget=6000 --screenshot="$out/stills/page-light.png" --window-size=1100,9000 --hide-scrollbars \
  --allow-file-access-from-files "$page?theme=light" >>$log 2>&1
"$B" --virtual-time-budget=6000 --screenshot="$out/stills/page-dark.png" --window-size=1100,9000 --hide-scrollbars \
  --allow-file-access-from-files "$page?theme=dark&t=6.5" >>$log 2>&1
shots=($out/stills/scene-*-light.png(n)); darks=($out/stills/scene-*-dark.png(n))
if (( ${#shots} == n && ${#darks} == n )); then
  $PY $W/tools/sheet.py $out/sheet.png $shots -- $darks && echo "ok: $((2 * n)) held frames in out/stills, sheet in out/sheet.png, the page in out/stills/page-*.png"
else
  echo "FAIL: ${#shots} light and ${#darks} dark stills for $n scenes"; fail+=(stills)
fi

echo "== 5. listening"
$PY $W/tools/listen.py $ch 2>>$log; r=$?
(( r == 1 )) && fail+=(listening); skipped=""; (( r == 3 )) && skipped=" (the listening check was SKIPPED: see above)"

echo
if (( ${#fail} )); then echo "FAILED: ${fail[*]}"; exit 1; fi
echo "all checks passed$skipped"
