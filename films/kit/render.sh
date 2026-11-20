#!/bin/zsh
# render.sh <explainer.js> <outdir> <ask> [query…]   e.g. "scene=0" "t=3.2" "scene=2&theme=dark" (→ scene-2_theme-dark.png)
js=$1; out=$2; ask=$3; shift 3; mkdir -p $out
here=${0:A:h}
python3 -c 'import json,sys,pathlib;h,kit,ask,out=sys.argv[1:];pathlib.Path(out).write_text(pathlib.Path(h).read_text().replace("KIT/",kit).replace("ASK_JSON",json.dumps(ask).replace("</","<\\/")))' $here/kit/player.html "file://$here/kit/" "$ask" $out/index.html
cp $js $out/explainer.js
B=~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell
for q in "$@"; do
  "$B" --virtual-time-budget=4000 --screenshot="$out/${${q//=/-}//&/_}.png" --window-size=1000,800 --hide-scrollbars --allow-file-access-from-files "file://$out/index.html?$q" >/dev/null 2>&1
done
"$B" --virtual-time-budget=4000 --dump-dom --allow-file-access-from-files "file://$out/index.html?scene=0" 2>/dev/null | python3 -c "import sys,re,html;d=sys.stdin.read();m=re.search(r'<pre id=\"errors\">(.*?)</pre>',d,re.S);t=re.search(r'data-timing=\"([^\"]*)\"',d);print('TIMING:',html.unescape(t.group(1)) if t else 'none: build() did not finish');print('ERRORS:',html.unescape(m.group(1)) if m and m.group(1).strip() else 'none')"
