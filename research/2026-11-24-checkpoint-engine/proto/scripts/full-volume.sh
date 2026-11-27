#!/bin/bash
# full-volume.sh: what a full volume does to a computer whose disk shares blocks with a checkpoint.
# A 2 GiB XFS volume: a 200 MB reserve file, an 800 MB image, one checkpoint (a clone), then a
# file fills the rest. Overwriting the image's own blocks then needs new space; see when it fails,
# and whether removing the reserve lets it go on.
set -e
img=/scratch/s.img; mnt=/mnt/s
truncate -s 2G $img; dev=$(losetup -f --show $img); mkfs.xfs -q -f -m reflink=1 $dev; mkdir -p $mnt; mount $dev $mnt
mkdir -p $mnt/t; cd $mnt/t
fallocate -l 200M reserve
dd if=/dev/urandom of=live.img bs=1M count=800 status=none; sync
cp --reflink=always live.img ckpt.img
dd if=/dev/zero of=filler bs=1M status=none 2>/dev/null || true
python3 - <<'P'
import os
fd = os.open('/mnt/s/t/live.img', os.O_WRONLY); n = 0
try:
    for i in range(800 * 256):
        os.pwrite(fd, b'x' * 4096, i * 4096); n += 1
        if n % 2048 == 0: os.fsync(fd)
    os.fsync(fd)
except OSError as e: print(f"overwriting the image failed after {n} blocks: {e}")
P
rm reserve
python3 - <<'P'
import os
fd = os.open('/mnt/s/t/live.img', os.O_WRONLY)
for i in range(1000): os.pwrite(fd, b'y' * 4096, i * 4096 * 7)
os.fsync(fd); print("with the reserve removed, 1000 overwrites went through")
P
cd /; umount $mnt; losetup -d $dev
