#!/bin/bash
# usage: sweep.sh <program> <since YYYY-MM-DD> <out dir>
# For each release of <program> published since <since> (stable versions only), downloads the
# Linux x64 package from npm, writes its markers to <out>/<version>.json, and deletes the package.
set -u
H=$(cd $(dirname $0); pwd); PROG=$1; SINCE=$2; OUT=$3; mkdir -p $OUT
case $PROG in
  claude-code) META=@anthropic-ai/claude-code; PKG() { echo @anthropic-ai/claude-code-linux-x64@$1; } ;;
  codex)       META=@openai/codex;             PKG() { echo @openai/codex@$1-linux-x64; } ;;
  gemini-cli)  META=@google/gemini-cli;         PKG() { echo @google/gemini-cli@$1; } ;;
esac
npm view $META time --json > $OUT/times.json
python3 -I - $OUT/times.json $SINCE > $OUT/versions.txt <<'PY'
import json, re, sys
d = json.load(open(sys.argv[1]))
vs = [(t, v) for v, t in d.items() if re.fullmatch(r"\d+\.\d+\.\d+", v) and t >= sys.argv[2]]
for t, v in sorted(vs): print(v, t[:10])
PY
W=$(mktemp -d)
while read v t; do
  [ -s $OUT/$v.json ] && continue
  (cd $W && rm -rf * && npm pack -q $(PKG $v) > /dev/null 2>&1 && mkdir x && tar xzf *.tgz -C x) || { echo "$v: no package" >&2; continue; }
  python3 -I $H/markers.py $PROG $W/x > $OUT/$v.json.tmp && mv $OUT/$v.json.tmp $OUT/$v.json
  echo "$v $t $(wc -c < $OUT/$v.json)"
done < $OUT/versions.txt
rm -rf $W
