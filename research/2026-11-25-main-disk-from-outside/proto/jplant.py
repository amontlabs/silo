"""Plants one fault in the live part of an ext4 image's journal, in place, for journal-faults.sh.

  python3 -I jplant.py IMAGE KIND

KIND is one of:
  data        one byte of the first data block of the first transaction
  descriptor  one byte of the first descriptor block, among its unused tags
  commit      one byte of the first commit block, after its checksum (commit_nsec)
  superblock  one byte of the journal's superblock, in its unused area
  sequence    the second transaction's descriptor gets the wrong sequence number (a gap)

Prints the journal block it changed.
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ext4read as E  # noqa: E402

path, kind = sys.argv[1], sys.argv[2]
img = E.Image(path, replay=False)
jmap = list(img.file_blocks(img.inode(img.journal_ino)))
bs = img.bs
f = open(path, "r+b")


def jread(i):
    f.seek(jmap[i] * bs)
    return f.read(bs)


def flip(i, at):
    f.seek(jmap[i] * bs + at)
    b = f.read(1)
    f.seek(jmap[i] * bs + at)
    f.write(bytes([b[0] ^ 0x5A]))
    print("journal block %d (fs block %d), byte %d" % (i, jmap[i], at))


sbj = jread(0)
first, start = E.be32(sbj, 20), E.be32(sbj, 28)
if start == 0:
    sys.exit("the journal is clean: nothing to plant in")
if kind == "superblock":
    flip(0, 0x200)
    sys.exit(0)
i, seen_desc, seq = start, 0, E.be32(sbj, 24)
for _ in range(E.be32(sbj, 16)):
    b = jread(i)
    # Data blocks carry no magic (an escaped one does not either): skip them. A block of this
    # journal's live part has the magic and a sequence not older than the journal's start.
    if E.be32(b, 0) != E.JBD2_MAGIC or E.be32(b, 8) < seq:
        i = i + 1 if i + 1 < E.be32(sbj, 16) else first
        continue
    t = E.be32(b, 4)
    if t == 1:
        seen_desc += 1
        if kind == "descriptor":
            flip(i, bs - 64)
            break
        if kind == "data":
            flip(i + 1, 100)
            break
        if kind == "sequence" and seen_desc == 2:
            seq = E.be32(b, 8)
            f.seek(jmap[i] * bs + 8)
            f.write(struct.pack(">I", seq + 7))
            print("journal block %d: sequence %d written as %d" % (i, seq, seq + 7))
            break
    if t == 2 and kind == "commit":
        flip(i, 57)
        break
    i = i + 1 if i + 1 < E.be32(sbj, 16) else first
else:
    sys.exit("no %s block found" % kind)
f.close()
