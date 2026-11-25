#!/bin/bash
# big.sh: each strategy on an 8 GiB image in about 500,000 pieces (#352's worst case), one after
# another on fresh file systems. Results in results/<strategy>-8g.jsonl.
cd "$(dirname "$0")/.."
args="size_gib=8 frag=300000 rate=1000 interval_ms=5000 count=8 drop=3 unrelated=1"
for s in ${@:-none full range cbw btrfs}; do
  if [ $s = btrfs ]; then scripts/fresh.sh btrfs; d=/mnt/b/run; else scripts/fresh.sh xfs; d=/mnt/x/run; fi
  ./zig-out/bin/ckpt bench strategy=$s dir=$d $args > results/$s-8g.jsonl 2> results/$s-8g.err
  df -B1M --output=used $(dirname $d) | tail -1 | sed "s/^/{\"phase\":\"space\",\"used_mib\":/; s/$/}/" >> results/$s-8g.jsonl
done
