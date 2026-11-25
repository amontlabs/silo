#!/bin/sh
# How long ext4's metadata stays only in its journal on a running disk. Copies a mounted image
# 30, 120 and 300 s after a burst of changes, and counts what a reader that ignores the journal
# sees. Needs root, a loop device, e2fsprogs. No KVM. Run: sudo sh journal-lag.sh [workdir]
set -eu
w=${1:-$(mktemp -d)}; cd "$w"
rm -f d.img s*.img; truncate -s 1G d.img; mkfs.ext4 -q -F d.img; mkdir -p m; mount -o loop d.img m
trap 'umount m 2>/dev/null || true' EXIT
mkdir m/work; for i in $(seq 1 500); do echo $i > m/work/f$i; done; sync
rm -f m/work/f1??; mkdir m/work/new; for i in $(seq 1 200); do echo x > m/work/new/n$i; done
cnt() { debugfs -c -R "ls -p $1" "$2" 2>/dev/null | grep -c '/[^/]*/[^/]*/[^/]*/[^/]*/[^.][^/]*/'; }
prev=0
for t in 7 30 120 300; do
  sleep $((t - prev)); prev=$t; cp --sparse=always d.img s$t.img
  echo "after ${t}s, journal ignored: /work holds $(cnt /work s$t.img) names (truth 401), /work/new $(cnt /work/new s$t.img) (truth 200)"
done
