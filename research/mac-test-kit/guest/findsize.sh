#!/bin/sh
# Runs inside a macOS guest: prints the whole disk (/dev/diskN) whose size is within 2% of $1
# bytes, as diskutil reports it (test 33 tells its extra disks apart by size).
for d in /dev/disk[0-9]*; do
  case "$d" in *s[0-9]*) continue ;; esac
  b="$(diskutil info "$d" 2>/dev/null | sed -n 's/.*Disk Size:[^(]*(\([0-9]*\) Bytes).*/\1/p' | head -1)"
  [ -n "$b" ] || continue
  if [ $((b > $1 ? b - $1 : $1 - b)) -le $(($1 / 50)) ]; then echo "$d"; exit 0; fi
done
exit 1
