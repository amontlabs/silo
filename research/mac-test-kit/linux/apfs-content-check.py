"""Checks py/apfs/apfscontent.py on disks Apple's APFS wrote (dissect.apfs's case_sensitive and
case_insensitive disks): every compressed file, in each of the six decmpfs methods, reads to the
contents the disks are known to hold; and a disk with one byte changed inside a compressed block
is never read as the original.

  python3 linux/apfs-content-check.py DIR
"""
import hashlib
import os
import shutil
import struct
import sys
import tempfile

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(here, "..", "py", "apfs"))
import apfscontent as C  # noqa: E402

d = sys.argv[1]
fails = 0
FORK = "5f46d97f947137dcf974fc19914c547acd18fcdb25124c846c1100f8b3fbca5f"
ATTR = hashlib.sha256(b"Compressed data in xattr " + b"a" * 90 + b"\n").hexdigest()
FILE = hashlib.sha256(" File System\n".encode()).hexdigest()


def check(name, ok, detail=""):
    global fails
    print("%s %s %s" % ("ok  " if ok else "FAIL", name, detail))
    fails += 0 if ok else 1


if C.LZ.fn is None:
    print("skip: no LZFSE decoder here (install liblzfse); zlib only")
for disk in ("case_sensitive", "case_insensitive"):
    img = os.path.join(d, disk + ".bin")
    t = C.open_volume(img)
    for alg in ("zlib", "lzvn", "lzfse"):
        if alg != "zlib" and C.LZ.fn is None:
            continue
        for where, want in (("fork", FORK), ("xattr", ATTR)):
            p = "/dir/compressed-%s-%s" % (alg, where)
            try:
                data, a = t.read(t.lookup(p))
                got = hashlib.sha256(data).hexdigest()
                check("%s %s" % (disk, p), got == want and C.ALGORITHMS.get(a) == "%s-%s" % (alg, where),
                      "%s %s" % (C.ALGORITHMS.get(a), got[:12]))
            except C.Gap as e:
                check("%s %s" % (disk, p), False, str(e))
    data, a = t.read(t.lookup("/dir/file"))
    check("%s /dir/file (plain)" % disk, a is None and hashlib.sha256(data).hexdigest() == FILE)
    hl, _ = t.read(t.lookup("/hardlink"))
    check("%s /hardlink reads as /dir/file" % disk, hl == data)
    try:
        t.read(t.lookup("/dir"))
        check("%s a directory is refused" % disk, False)
    except C.Gap:
        check("%s a directory is refused" % disk, True)

# One byte changed inside the zlib fork's compressed data: the reader must not give the original.
src = os.path.join(d, "case_sensitive.bin")
tmp = tempfile.mkdtemp()
try:
    for alg in ("zlib", "lzvn", "lzfse"):
        if alg != "zlib" and C.LZ.fn is None:
            continue
        mut = os.path.join(tmp, "mut-%s.bin" % alg)
        shutil.copy(src, mut)
        t = C.open_volume(mut)
        oid = t.lookup("/dir/compressed-%s-fork" % alg)
        sid, size = t.xattrs(oid)["com.apple.ResourceFork"][1]
        ext = t.records(sid, C.T_FEXT)
        lf, phys, _ = struct.unpack_from("<QQQ", ext[0][1], 0)
        # the middle of the first compressed block, past the fork's table or header
        pos = t.d.offset + phys * t.d.bs + min(size - 1, 300 + (size // 3))
        t.d.f.close()
        with open(mut, "r+b") as f:
            f.seek(pos)
            b = f.read(1)
            f.seek(pos)
            f.write(bytes([b[0] ^ 0x5A]))
        t = C.open_volume(mut)
        try:
            data, _ = t.read(t.lookup("/dir/compressed-%s-fork" % alg))
            same = hashlib.sha256(data).hexdigest() == FORK
            check("a changed byte in the %s fork is not read as the original" % alg, not same,
                  "read %d bytes, different" % len(data))
        except (C.Gap, Exception) as e:  # a gap is the right answer too
            check("a changed byte in the %s fork is not read as the original" % alg, True, "gap: %s" % str(e)[:80])
finally:
    shutil.rmtree(tmp)
sys.exit(1 if fails else 0)
