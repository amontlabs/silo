#!/bin/bash
# Builds #385's boundary as it is (before) and with websocket-admission.patch (after), runs each
# case of ../boundary.sh through both, and prints what the record says. Needs Zig 0.15.2, OpenSSL,
# zlib and zstd, git, and Codex as CODEX.
#   boundary/run.sh <scratch folder> [ref of #385's branch, default pr385]
set -eu
H=$(cd "$(dirname "$0")/.." && pwd); W=$(realpath -m "$1"); REF=${2:-pr385}
S=research/2026-11-24-spend-budgets
rm -rf "$W/src"; mkdir -p "$W/src"
git -C "$(git -C "$H" rev-parse --show-toplevel)" archive "$REF" $S | tar -x -C "$W/src"
for side in before after; do
  rm -rf "$W/$side"; cp -r "$W/src/$S/boundary" "$W/$side"
  [ $side = after ] && (cd "$W/$side" && patch -s -p1 < "$H/boundary/websocket-admission.patch")
  (cd "$W/$side" && zig build && zig build test)
done
export PRICES=$W/src/$S/prices.json
for side in before after; do
  mkdir -p "$W/out-$side"
  for c in gpt gpt-http key gpt-nobudget gpt-rotated; do
    BOUNDARY=$W/$side/zig-out/bin/boundary LIMIT=60 "$H/boundary.sh" "$W/out-$side" $c -m gpt-6.2-sol > "$W/out-$side/$c.line" 2>&1 || true
    python3 - "$W/out-$side/$c" "$side" "$c" <<'PY'
import json, sys, collections, os
d, side, case = sys.argv[1:]
k = collections.Counter(); calls = 0.0
for l in open(f"{d}/record.jsonl"):
    o = json.loads(l); kind = o.get("kind")
    if kind == "call": calls += o["cost"]
    if kind == "refused": kind += ":" + o.get("why", "sni " + o.get("sni", ""))
    if kind in ("call", "ws-request", "ws-refused", "reached") or kind.startswith("refused"): k[kind] += 1
res = open(f"{d}/result.txt").read().strip()
files = " ".join(sorted(os.listdir(f"{d}/work"))) or "(none)"
last = open(f"{d}/codex.out").read().strip().splitlines()[-1][:110]
print(f"{side:6s} {case:13s} {res:7s} files: {files:18s} {dict(k)}\n{'':21s}{last}")
PY
  done
done
