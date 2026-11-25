#!/bin/bash
# mac.sh: the same prototype on a Mac's APFS, about 40 minutes, about 30 GB free, no sudo, no VM.
# Needs Zig 0.15.2 (brew install zig, or the tarball from ziglang.org). Results in ~/SiloKit/ckpt.
#
#   1. full  : a clonefile of the live image, while a stand-in computer writes it uncached
#              (#359's test 01 measures the same with a real computer; this one is the control)
#   2. cbw   : the copy-before-write checkpoint: clonefile of the last checkpoint, which nobody
#              writes, then the marked blocks copied. Answers: does cloning a file that shares
#              blocks with the live image make the live image's writes wait on APFS?
#   3. nbd   : the same, with every write going through Silo's NBD server over a socket
#   4. crash : the catalog under SIGKILL at every step, with F_FULLFSYNC and renamex_np(RENAME_SWAP)
set -e
cd "$(dirname "$0")/.."
OUT=${OUT:-$HOME/SiloKit/ckpt}
mkdir -p "$OUT"
zig build --release
B=./zig-out/bin/ckpt
args="size_gib=8 frag=300000 rate=1000 interval_ms=5000 count=8 drop=3 unrelated=1"
for s in full cbw nbd; do
  d="$OUT/run-$s-$(date +%s)"
  echo "== $s (in $d)"
  $B bench strategy=$s dir="$d" $args | tee "$OUT/$s.jsonl"
  rm -rf "$d"
done
d="$OUT/crash-$(date +%s)"
$B crash root="$d" rounds=300 systematic=1 | tee "$OUT/crash.jsonl"
rm -rf "$d"
for m in two-renames no-tmp; do
  d="$OUT/mutant-$m-$(date +%s)"
  CKPT_MUTANT=$m $B crash root="$d" rounds=300 systematic=1 > "$OUT/mutant-$m.txt" 2>&1 && echo "mutant $m NOT caught" || echo "mutant $m caught"
  rm -rf "$d"
done
python3 scripts/show.py "$OUT"/full.jsonl "$OUT"/cbw.jsonl "$OUT"/nbd.jsonl
