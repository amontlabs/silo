#!/bin/bash
# Restore: how long it takes to put a computer's 8 GiB disk back to a checkpoint, and how fast the
# restored disk reads (4 KiB random reads, 15 s; then 2 GiB sequential, as a boot would read).
# $1 = xfs|btrfs|btrfs-subvol. Image fragmented by N random 4k writes after a clone, as in
# xtenant.sh. A fresh, unfragmented image is read the same way for comparison.
set -e
FS=$1; N=${N:-300000}; W=/var/tmp/lh; mkdir -p $W; M=$W/mnt-r; mkdir -p $M
umount $M 2>/dev/null || true; rm -f $W/r.raw; truncate -s 30G $W/r.raw
if [ $FS = xfs ]; then mkfs.xfs -q -m reflink=1 $W/r.raw; else mkfs.btrfs -q $W/r.raw; fi
mount -o loop $W/r.raw $M
trap 'umount /var/tmp/lh/mnt-r; rm -f /var/tmp/lh/r.raw' EXIT
if [ $FS = btrfs-subvol ]; then btrfs subvolume create $M/live >/dev/null; D=$M/live; else D=$M; fi
dd if=/dev/urandom of=$D/disk.img bs=16M count=512 oflag=direct status=none
cp --reflink=always $D/disk.img $M/old.img
fio --name=frag --filename=$D/disk.img --rw=randwrite --bs=4k --direct=1 --io_size=$((N*4))k --size=8g --randrepeat=0 --ioengine=libaio --iodepth=32 --output=/dev/null
rm $M/old.img; sync
reads() { # $1 file, $2 label
  sync; echo 3 > /proc/sys/vm/drop_caches
  r=$(fio --name=r --filename=$1 --rw=randread --bs=4k --direct=1 --runtime=15 --time_based --size=8g --ioengine=libaio --iodepth=16 --output-format=json | python3 -c 'import json,sys;print(int(json.load(sys.stdin)["jobs"][0]["read"]["iops"]))')
  s=$(fio --name=s --filename=$1 --rw=read --bs=1M --direct=1 --size=2g --ioengine=libaio --iodepth=8 --output-format=json | python3 -c 'import json,sys;print(int(json.load(sys.stdin)["jobs"][0]["read"]["bw"]/1024))')
  echo "$FS $2: extents=$(filefrag $1 | awk '{print $2}') randread_iops=$r seqread_MBps=$s"
}
# checkpoint, then the computer goes on writing (20k more random writes), then restore
if [ $FS = btrfs-subvol ]; then btrfs subvolume snapshot -r $D $M/ck >/dev/null; else cp --reflink=always $D/disk.img $M/ck.img; fi
fio --name=more --filename=$D/disk.img --rw=randwrite --bs=4k --direct=1 --io_size=80m --size=8g --randrepeat=0 --ioengine=libaio --iodepth=32 --output=/dev/null
sync
s=$(date +%s.%N)
if [ $FS = btrfs-subvol ]; then
  btrfs subvolume snapshot $M/ck $M/live.new >/dev/null; mv $M/live $M/live.old; mv $M/live.new $M/live
  btrfs subvolume delete -c $M/live.old >/dev/null
else
  cp --reflink=always $M/ck.img $D/disk.new; mv $D/disk.new $D/disk.img
fi
sync; e=$(date +%s.%N)
echo "$FS restore_s=$(echo "$e - $s" | bc)"
if [ $FS = btrfs-subvol ]; then D=$M/live; fi
reads $D/disk.img restored
dd if=/dev/urandom of=$M/fresh.img bs=16M count=512 oflag=direct status=none
reads $M/fresh.img fresh
