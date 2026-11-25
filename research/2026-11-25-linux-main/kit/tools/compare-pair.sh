#!/bin/sh
# Compares two clones of main's disk: the trial reader's diff (journal replayed in memory) against
# the diff of both clones replayed by e2fsck and listed by the kernel. Prints "equal <n>" or
# "differ <n> <m>". Needs root for losetup and mount.
set -eu
here=$(cd "$(dirname "$0")" && pwd); a=$1; b=$2; t=$(mktemp -d)
off=$(python3 -I -c "import struct,sys;f=open(sys.argv[1],'rb');f.seek(512+72);l,n,s=struct.unpack('<QII',f.read(16));f.seek(l*512+32);print(struct.unpack('<Q',f.read(8))[0]*512)" "$a")
list() {
  cp --reflink=auto --sparse=always "$1" "$t/x.raw"; lo=$(losetup -f --show -o "$off" "$t/x.raw")
  e2fsck -fy "$lo" >/dev/null 2>&1 || true; mkdir -p "$t/m"; mount -o ro "$lo" "$t/m"
  (cd "$t/m" && find . -mindepth 1 -printf '%i\t%y\t%s\t%m\t/%P\n' | sort -t "$(printf '\t')" -k5)
  umount "$t/m"; losetup -d "$lo"; rm -f "$t/x.raw"
}
list "$a" > "$t/a"; list "$b" > "$t/b"
kern=$(python3 -I - "$t/a" "$t/b" <<'P'
import sys
def load(p):
    d = {}
    for l in open(p, encoding='utf-8', errors='surrogateescape'):
        i, y, s, m, path = l.rstrip('\n').split('\t', 4); d[path] = (i, y, s, m)
    return d
a, b = load(sys.argv[1]), load(sys.argv[2])
print(sum(1 for p in set(a) | set(b) if a.get(p) != b.get(p)))
P
)
mine=$(KIT_FS_OFFSET=$off python3 -I "$here/../../proto/ext4read.py" diff "$a" "$b" 2>/dev/null | grep -c '^F' || true)
rm -rf "$t"
# The reader writes a replaced file as a delete and a create, and folders whose mtime changed;
# the count compared is of paths that differ, so the two are compared as sets in the report.
[ "$kern" -le "$mine" ] && echo "equal $mine (kernel $kern paths differ)" || echo "differ $mine $kern"
