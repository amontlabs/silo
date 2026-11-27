#!/bin/bash
# usage: install.sh <program> <version>   prints the path of the program's command
# Puts one release of an agent program in /opt/agents/<program>-<version>, from npm, unchanged.
set -eu
P=$1; V=$2; D=/opt/agents/$P-$V
find_bin() { find $D -type f -name "$1" -perm -u+x 2>/dev/null | grep -v node_modules/.bin | head -1; }
case $P in
  claude-code) N=claude; PKG=@anthropic-ai/claude-code-linux-x64@$V ;;
  codex)       N=codex;  PKG=@openai/codex@$V-linux-x64 ;;
  gemini-cli)  N=gemini.js; PKG=@google/gemini-cli@$V ;;
esac
if [ -z "$(find_bin $N)" ]; then
  rm -rf $D; mkdir -p $D
  if [ $P = gemini-cli ]; then
    npm install -q --no-audit --no-fund --prefix $D $PKG > /dev/null 2>&1
    chmod +x $(find $D/node_modules/@google/gemini-cli -name gemini.js -path '*bundle*' | head -1)
  else
    (cd $D && npm pack -q $PKG > /dev/null 2>&1 && tar xzf *.tgz && rm *.tgz)
  fi
fi
B=$(find_bin $N)
[ $P = gemini-cli ] && B=$(find $D/node_modules/@google/gemini-cli -name gemini.js -path '*bundle*' | head -1)
[ -n "$B" ] || { echo "no $N in $D" >&2; exit 1; }
echo $B
