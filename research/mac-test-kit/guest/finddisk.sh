#!/bin/sh
# Runs as root inside a macOS guest: prints the whole disk (/dev/diskN) whose first block holds
# the text $1, which the host wrote there before the computer started.
for d in /dev/disk[0-9]*; do
  case "$d" in *s[0-9]*) continue ;; esac
  r="/dev/r${d#/dev/}"
  if dd if="$r" bs=4096 count=1 2>/dev/null | grep -q "$1"; then echo "$r"; exit 0; fi
done
exit 1
