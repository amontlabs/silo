#!/bin/bash
# Does one computer's checkpoint stall another computer on the same filesystem?
# $1 = xfs|btrfs. Makes a 24 GiB loop filesystem, image A 8 GiB fragmented by N random 4k
# writes, image B 2 GiB. B's writer runs throughout; A is reflink-copied three times, 6 s apart.
# Remove $W/$FS.raw afterwards.
set -e
FS=$1; N=${N:-300000}; H=$(dirname $0)
W=${W:-/var/tmp/lh}; mkdir -p $W; M=$W/mnt-$FS; mkdir -p $M
umount $M 2>/dev/null || true; rm -f $W/$FS.raw
truncate -s 24G $W/$FS.raw
if [ $FS = xfs ]; then mkfs.xfs -q -m reflink=1 $W/$FS.raw; else mkfs.btrfs -q $W/$FS.raw; fi
mount -o loop $W/$FS.raw $M
dd if=/dev/urandom of=$M/A.img bs=16M count=512 oflag=direct status=none
touch $M/B.img; if [ "$NOCOW" = 1 ]; then chattr +C $M/B.img; fi   # NOCOW=1: B written in place on Btrfs
dd if=/dev/urandom of=$M/B.img conv=notrunc bs=16M count=128 oflag=direct status=none
cp --reflink=always $M/A.img $M/old.img   # sharing makes each later write a new piece
fio --name=frag --filename=$M/A.img --rw=randwrite --bs=4k --direct=1 --io_size=$((N*4))k --size=8g --randrepeat=0 --ioengine=libaio --iodepth=32 --output=/dev/null
rm $M/old.img; sync; echo "A extents: $(filefrag $M/A.img | awk '{print $2}')"
run() { # $1 label, $2 copy command prefix
  rm -f $W/stop; python3 $H/bwriter.py $M/B.img $W/stop $W/lat-$FS-$1.json & P=$!
  : > $W/win-$FS-$1.txt
  for k in 1 2 3; do
    sleep 6
    s=$(date +%s.%N); $2 cp --reflink=always $M/A.img $M/ck$k.img; e=$(date +%s.%N)
    echo "$s $e" >> $W/win-$FS-$1.txt
  done
  sleep 6; touch $W/stop; wait $P; rm -f $M/ck*.img; sync
  python3 $H/summ.py $W/lat-$FS-$1.json $W/win-$FS-$1.txt "$FS $1"; rm -f $W/lat-$FS-$1.json
}
run plain${NOCOW:+-nocow} ""
# A's copy in a blkio cgroup throttled to 50 MB/s write, 2000 iops
CG=/sys/fs/cgroup/blkio/siloA; mkdir -p $CG
DEV=$(lsblk -no MAJ:MIN $(findmnt -no SOURCE $M) | tr -d ' ')
echo "$DEV 52428800" > $CG/blkio.throttle.write_bps_device
echo "$DEV 2000" > $CG/blkio.throttle.write_iops_device
cgexec_() { sh -c "echo \$\$ > $CG/cgroup.procs; exec \"\$@\"" _ "$@"; }
[ "$NOCOW" = 1 ] || run throttled cgexec_
umount $M
