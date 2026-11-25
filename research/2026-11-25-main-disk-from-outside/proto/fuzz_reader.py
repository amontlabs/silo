"""Coverage-guided fuzzing of an APFS content reader (#390's probe, or this folder's content_reader.py)
with what root in main can write: any bytes in any block of the volume, with valid Fletcher-64
checksums, since root can compute them as APFS does.

  python3 fuzz_reader.py READER_DIR IMAGE [libFuzzer flags]
  python3 fuzz_reader.py READER_DIR IMAGE --replay FILE...    run inputs once, print each outcome

READER_DIR holds apfsread.py and apfscontent.py (the probe), or content_reader.py. IMAGE is an APFS
disk with compressed files: dissect.apfs's case_sensitive.bin.

Each input is a list of patches: which block of the image (among the blocks the reader reads on a
clean run: the tree's nodes, the object map, the compressed files' forks), where in it, and which
bytes. Patched metadata blocks get their checksum recomputed. Then the reader lists every file
under / and reads each one, as `look` would.

The reader may refuse anything (a gap: Gap or Bad). Any other exception, more than MEM_MB of
memory, or more than the libFuzzer -timeout is a fault of the reader and stops the run.
"""
import os
import resource
import struct
import sys
import tempfile

import atheris

READER, IMAGE = sys.argv[1], sys.argv[2]
sys.path.insert(0, os.path.abspath(READER))
MEM_MB = int(os.environ.get("MEM_MB", "1024"))
if MEM_MB:  # 0 under AddressSanitizer, whose shadow memory needs the address space; then
    # libFuzzer's -rss_limit_mb is the limit
    resource.setrlimit(resource.RLIMIT_AS, (MEM_MB << 20, MEM_MB << 20))

with atheris.instrument_imports():
    if os.path.exists(os.path.join(READER, "content_reader.py")):
        import content_reader as R
        GAPS = R.GAPS
        BS = 4096

        def read_all(path):
            t = R.open_volume(path)
            for p, oid, typ in t.walk():
                if typ == 8:
                    try:
                        t.read(oid)
                    except R.GAPS:
                        pass
    else:
        import zlib
        import apfsread as A
        import apfscontent as C
        # What the probe's own `ls` turns into a gap: Bad around the whole run, and these four
        # around the read of each file.
        GAPS = (A.Bad,)
        BS = 4096

        def read_all(path):
            t = C.open_volume(path)
            for p, oid, typ in t.walk(C.ROOT_DIR, "/"):
                if typ == 8:
                    try:
                        t.read(oid)
                    except (C.Gap, A.Bad, zlib.error, struct.error):
                        pass

ORIG = open(IMAGE, "rb").read()


def fletcher64(block):
    import itertools
    from array import array
    M = 0xFFFFFFFF
    w = array("I", block[8:])
    s1 = sum(w) % M
    s2 = sum(itertools.accumulate(w)) % M
    c1 = M - ((s1 + s2) % M)
    c2 = M - ((s1 + c1) % M)
    return (c2 << 32) | c1


def metadata(b):
    return struct.unpack_from("<Q", b, 0)[0] == fletcher64(b) and any(b[:8])


# The blocks worth patching: every block that is metadata (its checksum holds) or that holds
# data of a compressed file. The data blocks are found as the blocks that differ from zero
# and are not metadata, in the container's used area.
TARGETS = []
for i in range(len(ORIG) // BS):
    b = ORIG[i * BS:(i + 1) * BS]
    if any(b):
        TARGETS.append((i, metadata(b)))

FD, TMP = tempfile.mkstemp(prefix="apfs-fuzz-", dir="/dev/shm" if os.path.isdir("/dev/shm") else None)


def build(data):
    fdp = atheris.FuzzedDataProvider(data)
    img = bytearray(ORIG)
    touched = set()
    for _ in range(fdp.ConsumeIntInRange(1, 4)):
        blk, meta = TARGETS[fdp.ConsumeIntInRange(0, len(TARGETS) - 1)]
        at = fdp.ConsumeIntInRange(32 if meta else 0, BS - 1)
        kind = fdp.ConsumeIntInRange(0, 3)
        if kind == 0:
            new = fdp.ConsumeBytes(fdp.ConsumeIntInRange(1, 16))
        elif kind == 1:  # an edge value of 2, 4 or 8 bytes
            w = (2, 4, 8)[fdp.ConsumeIntInRange(0, 2)]
            v = (0, 1, 0x7F, 0x80, 0xFF, 0xFFFF, 0x7FFFFFFF, 0xFFFFFFFF, 1 << 40, (1 << 64) - 1)[fdp.ConsumeIntInRange(0, 9)]
            new = (v & ((1 << (8 * w)) - 1)).to_bytes(w, "little")
        elif kind == 2:  # a bit
            new = bytes([img[blk * BS + at] ^ (1 << fdp.ConsumeIntInRange(0, 7))])
        else:  # a copy of bytes from elsewhere in the image
            src = fdp.ConsumeIntInRange(0, len(ORIG) - 64)
            new = ORIG[src:src + fdp.ConsumeIntInRange(1, 64)]
        new = new[:BS - at]
        img[blk * BS + at: blk * BS + at + len(new)] = new
        if meta:
            touched.add(blk)
    for blk in touched:
        b = bytes(img[blk * BS:(blk + 1) * BS])
        img[blk * BS:blk * BS + 8] = struct.pack("<Q", fletcher64(b))
    return bytes(img)


SEEN = set()


def one(data):
    img = build(data)
    os.pwrite(FD, img, 0)
    try:
        read_all(TMP)
    except GAPS:
        pass
    except Exception as e:  # noqa: BLE001
        if not COLLECT:
            raise
        # With COLLECT=FILE, each distinct fault (its type and the reader's line) is written
        # once, with the input that found it, and the run goes on to find the next.
        import traceback
        tb = [f for f in traceback.extract_tb(e.__traceback__) if "fuzz_reader" not in f.filename][-1:]
        where = "%s:%d" % (os.path.basename(tb[0].filename), tb[0].lineno) if tb else "?"
        sig = "%s at %s" % (type(e).__name__, where)
        if sig not in SEEN:
            SEEN.add(sig)
            with open(COLLECT, "a") as f:
                f.write("%s\t%s\t%s\t%s\n" % (sig, tb[0].line if tb else "", str(e)[:160].replace("\n", " "), data.hex()))


COLLECT = os.environ.get("COLLECT")


if "--replay" in sys.argv:
    import traceback
    for p in sys.argv[sys.argv.index("--replay") + 1:]:
        try:
            one(open(p, "rb").read())
            print("%s: no fault" % p)
        except BaseException as e:  # noqa: BLE001 - the point is to see any fault
            print("%s: %s: %s" % (p, type(e).__name__, str(e)[:200]))
            tb = traceback.extract_tb(e.__traceback__)[-1]
            print("    at %s:%d %s" % (os.path.basename(tb.filename), tb.lineno, tb.line))
    sys.exit(0)

os.ftruncate(FD, len(ORIG))
atheris.Setup([sys.argv[0]] + sys.argv[3:], one)
atheris.Fuzz()
