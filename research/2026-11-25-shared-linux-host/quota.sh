#!/bin/bash
# One member filling the host's disk, and what a per-member limit does about it.
# $1 = xfs|btrfs. Makes a 16 GiB loop file system with two members' folders, A and B.
# Each member's limit is 6 GiB. A's main disk is 3 GiB of data; A takes 4 checkpoints by
# reflink (XFS) or snapshot (Btrfs); then A writes until refused; then B writes 1 GiB.
# $1 = none: Btrfs with no limit, the case without one.
# Prints, after each step: real space used on the file system, what the limit counts for A,
# and whether B's write succeeded. Run as root. Removes its files afterwards.
set -u
MODE=$1; FS=$1; [ $FS = none ] && FS=btrfs; W=${W:-/var/tmp/shq}; M=$W/mnt-$FS; mkdir -p $W $M
umount $M 2>/dev/null; rm -f $W/$FS.raw; truncate -s 16G $W/$FS.raw
G=$((1024*1024*1024))
used() { df -B1 --output=used $M | tail -1; }
if [ $FS = xfs ]; then
  mkfs.xfs -q -m reflink=1 $W/$FS.raw; mount -o loop,prjquota $W/$FS.raw $M || { echo "XFS quotas not available in this kernel"; rm -f $W/$FS.raw; exit 1; }
  mkdir $M/A $M/B
  xfs_quota -x -c "project -s -p $M/A 1" -c "project -s -p $M/B 2" $M >/dev/null
  xfs_quota -x -c "limit -p bhard=6g 1" -c "limit -p bhard=6g 2" $M
  countA() { xfs_quota -x -c "quota -p -N -b 1" $M | awk '{print $2*1024}'; }
  ckpt() { cp --reflink=always $M/A/main.img $M/A/ck$1.img; }
  mkmain() { :; }
else
  mkfs.btrfs -q $W/$FS.raw; mount -o loop $W/$FS.raw $M || exit 1
  if [ $MODE = none ]; then mkdir $M/A $M/B; countA() { du -sb $M/A | cut -f1; }; ckpt() { cp --reflink=always $M/A/main.img $M/A/ck$1.img; }; else
  btrfs quota enable $M; btrfs subvolume create $M/A >/dev/null; btrfs subvolume create $M/B >/dev/null
  # a member's limit is a qgroup 1/n holding the member's subvolumes, limited on what it references
  btrfs qgroup create 1/1 $M; btrfs qgroup create 1/2 $M
  ida=$(btrfs subvolume show $M/A | awk '/Subvolume ID/{print $3}'); idb=$(btrfs subvolume show $M/B | awk '/Subvolume ID/{print $3}')
  btrfs qgroup assign 0/$ida 1/1 $M 2>/dev/null; btrfs qgroup assign 0/$idb 1/2 $M 2>/dev/null
  btrfs qgroup limit 6G 1/1 $M; btrfs qgroup limit 6G 1/2 $M
  countA() { sync; btrfs qgroup show --raw $M | awk '$1=="1/1"{print $2}'; }
  ckpt() { btrfs subvolume snapshot -r $M/A $M/A-ck$1 >/dev/null; id=$(btrfs subvolume show $M/A-ck$1 | awk '/Subvolume ID/{print $3}'); btrfs qgroup assign 0/$id 1/1 $M 2>/dev/null; }
  fi
fi
report() { sync; printf '%-34s real used %6.2f GiB   counted for A %6.2f GiB\n' "$1" $(echo "$(used)/$G" | bc -l) $(echo "$(countA)/$G" | bc -l); }
report "empty"
dd if=/dev/urandom of=$M/A/main.img bs=16M count=192 status=none; report "A: main disk 3 GiB"
for k in 1 2 3 4; do
  ckpt $k
  dd if=/dev/urandom of=$M/A/main.img bs=4k count=25600 seek=$((RANDOM*20)) conv=notrunc status=none  # 100 MiB of new writes
  report "A: checkpoint $k, then 100 MiB written"
done
out=$(dd if=/dev/zero of=$M/A/fill bs=16M status=none 2>&1); report "A writes until refused"; echo "  A refused with: ${out##*: }"
out=$(dd if=/dev/zero of=$M/B/work bs=16M count=64 status=none 2>&1 && echo ok); echo "  B writes 1 GiB: ${out##*: }"
umount $M; rm -f $W/$FS.raw
