#!/usr/bin/env bash
# Pushes three commits of a real repository's history to the fake GitHub, with and without the
# no-thin capability in the advertisement, and reports what the boundary can read from each pack.
#   bash thin_experiment.sh SOURCE_REPO OUT_DIR
set -u
if [ -z "${RESOURCE_SCOPES_CLEAN:-}" ]; then exec env -i RESOURCE_SCOPES_CLEAN=1 PATH="$PATH" bash "$0" "$@"; fi
HERE="$(cd "$(dirname "$0")" && pwd)"; SRC="$(cd "$1" && pwd)"; OUT="$(mkdir -p "$2" && cd "$2" && pwd)"
for mode in thin no-thin; do
  S="$OUT/$mode"; rm -rf "$S"; mkdir -p "$S/state/git/victim"
  B="$S/state/git/victim/big.git"; git init -q --bare "$B"; git -C "$B" config http.receivepack true
  git -C "$SRC" branch -f scopes-base HEAD~3; git -C "$B" fetch -q "$SRC" scopes-base:refs/heads/main
  flag=""; [ $mode = no-thin ] && flag="--no-thin"
  python3 -I "$HERE/record_proxy.py" --port 8898 --state "$S/state" --out "$S/corpus.jsonl" $flag > "$S/proxy.log" 2>&1 & P=$!
  for _ in $(seq 50); do grep -q ready "$S/proxy.log" 2>/dev/null && break; sleep 0.1; done
  ( cd "$SRC" && HOME="$S" GIT_SSL_CAINFO="$S/state/ca.pem" HTTPS_PROXY=http://127.0.0.1:8898 \
      git push -q https://github.com/victim/big.git HEAD~2:refs/heads/main HEAD~1:refs/heads/deep HEAD:refs/heads/wf ) 2>&1 | tail -2
  kill $P
  (cd "$HERE" && python3 pack_report.py "$S/corpus.jsonl" "$mode")
done
