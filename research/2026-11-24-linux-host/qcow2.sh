#!/bin/bash
# qcow2 layers on ext4, the fallback of fork-shares-disk-blocks: cost of a checkpoint (a new
# layer), of a restore (drop the layers above), of thinning (merge a layer into the one below),
# and reads through the chain. Base: 8 GiB raw; 30 layers, each with 64 MiB of 4 KiB random writes.
set -e
W=${W:-/var/tmp/lh/q}; rm -rf $W; mkdir -p $W; cd $W
dd if=/dev/urandom of=base.raw bs=16M count=512 oflag=direct status=none
prev=base.raw; pf=raw
for i in $(seq 1 30); do
  s=$(date +%s.%N); qemu-img create -q -f qcow2 -F $pf -b $prev l$i.qcow2; e=$(date +%s.%N)
  [ $i = 1 ] && echo "checkpoint (new layer) s=$(echo "$e - $s" | bc)"
  python3 -c "
import random
random.seed($i)
print('\n'.join(f'write -P {i%250} {random.randrange(2097152)*4096} 4k' for i in range(16384)))" > w.txt
  qemu-io -f qcow2 -t none l$i.qcow2 < w.txt > /dev/null
  prev=l$i.qcow2; pf=qcow2
done
bench() { qemu-img bench -f $2 -t none -i native -c 20000 -s 4096 -S 1048576 -d 16 $1 2>&1 | grep -o "Run completed in [0-9.]* seconds"; }
echo "read 20000 x 4k spread, raw base: $(bench base.raw raw)"
echo "read 20000 x 4k spread, top of 30 layers: $(bench l30.qcow2 qcow2)"
s=$(date +%s.%N); qemu-img commit -q -f qcow2 -t none l15.qcow2; qemu-img rebase -q -u -f qcow2 -F qcow2 -b l14.qcow2 l16.qcow2; e=$(date +%s.%N)
echo "thin one checkpoint (commit layer 15 into 14, rebase 16) s=$(echo "$e - $s" | bc)"
s=$(date +%s.%N); rm -f l2[1-9].qcow2 l30.qcow2; qemu-img create -q -f qcow2 -F qcow2 -b l20.qcow2 top.qcow2; e=$(date +%s.%N)
echo "restore to checkpoint 20 (drop 10 layers, new top) s=$(echo "$e - $s" | bc)"
s=$(date +%s.%N); qemu-img convert -q -O qcow2 -t none top.qcow2 flat.qcow2; e=$(date +%s.%N)
echo "flatten the whole chain into one file s=$(echo "$e - $s" | bc)"
cd /; rm -rf $W
