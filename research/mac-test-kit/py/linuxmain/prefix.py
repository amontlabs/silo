#!/usr/bin/env python3
"""For kit test K07: reads the numbered file from a clone of main's disk, through the trial ext4
reader (journal replayed), and says whether its blocks form a prefix of the writes.
  prefix.py DISK.raw /home/kit/seq     prints "prefix <last>" or "torn <slot> <found> <expected>"
"""
import os, re, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ext4read import Image

SLOTS = 16384

def root_offset(path):
    f = open(path, 'rb'); f.seek(512 + 72)
    lba, n, sz = struct.unpack('<QII', f.read(16))
    f.seek(lba * 512)
    return struct.unpack('<Q', f.read(sz)[32:40])[0] * 512   # partition 1's first sector

def lookup(img, path):
    ino = 2
    for part in path.strip('/').split('/'):
        for child, _t, name in img.dir_entries(img.inode(ino)):
            if name == part.encode():
                ino = child; break
        else:
            raise SystemExit(f'missing {path}')
    return img.inode(ino)

off = os.environ.get('KIT_FS_OFFSET')  # a bare filesystem image: 0
img = Image(sys.argv[1], True, int(off) if off is not None else root_offset(sys.argv[1]))
ib = lookup(img, sys.argv[2])
found = {}
for lblk, pblk, ln in img.extents(ib):
    for k in range(ln):
        b = img.block(pblk + k)
        m = re.match(rb'SILO(\d{12})', b)
        if m:
            found[lblk + k] = int(m.group(1))
last = max(found.values(), default=-1)
for s in range(SLOTS):
    exp = last - ((last - s) % SLOTS) if last >= s else -1
    got = found.get(s, -1)
    if got != exp:
        print(f'torn {s} {got} {exp}'); sys.exit(1)
print(f'prefix {last}')
