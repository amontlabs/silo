#!/bin/zsh
# shot.sh <audit dir with index.html> <query> <out.png> [x0 y0 x1 y1 crop at 2x]: one frame at 2x, optionally cropped
B=~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell
"$B" --virtual-time-budget=4000 --force-device-scale-factor=2 --screenshot=$3 --window-size=1000,800 --hide-scrollbars --allow-file-access-from-files "file://$1/index.html?$2" >/dev/null 2>&1
[[ -n $4 ]] && /Library/Frameworks/Python.framework/Versions/3.12/bin/python3 -c "
import sys
from PIL import Image
im=Image.open(sys.argv[1]); im.crop(tuple(int(v) for v in sys.argv[2:6])).save(sys.argv[1])" $3 $4 $5 $6 $7
