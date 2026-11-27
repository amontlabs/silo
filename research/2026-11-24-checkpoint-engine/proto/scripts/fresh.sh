#!/bin/bash
# fresh.sh xfs|btrfs: a new empty file system on its loop device, mounted at /mnt/x or /mnt/b.
# Each run starts on a fresh file system, so one run's pieces do not carry into the next.
set -e
case $1 in
  xfs)   dev=$(findmnt -n -o SOURCE /mnt/x); umount /mnt/x; mkfs.xfs -q -f -m reflink=1 "$dev"; mount "$dev" /mnt/x ;;
  btrfs) dev=$(findmnt -n -o SOURCE /mnt/b); umount /mnt/b; mkfs.btrfs -q -f "$dev"; mount "$dev" /mnt/b ;;
esac
