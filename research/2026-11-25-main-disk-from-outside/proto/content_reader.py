"""The content mode of #370's reader, with the rules this research found, as a Python prototype:
#390's probe (mac-test-kit/py/apfs/apfscontent.py) rewritten so that a hostile disk gives a gap,
never a crash, never unbounded memory, and never more bytes than the file has.

  content_reader.py ls IMG [--under PATH]     one JSON line per file: path, size, compressed, sha256, or gap
  content_reader.py cat IMG PATH              the contents of one file

The rules, each one found failing in the probe by fuzz_reader.py (README, "The reader"):

  R1  Every length read from the disk is checked before it is used: a key or value too short for
      what is unpacked from it is a gap.
  R2  No size from the disk is trusted to allocate: a file, an xattr stream or a hole is bounded
      by the container's size and by the read budget (MAX_READ) before any byte is allocated.
  R3  A file's extents are in order, do not overlap, and end within its size.
  R4  decmpfs's table of blocks is checked whole before a block is decoded: the count from the
      header's size, offsets increasing, every block inside the fork.
  R5  The decoder is given its input with 8 bytes of zeroes after it (Apple's lzfse reads up to 7
      bytes past the end of a hostile LZFSE block), and must give exactly the block's size.
  R6  The walk has a depth limit, refuses a directory reached twice, and has a budget of entries.

What no rule can do: tell a corrupted LZVN or LZFSE block from the original when it decodes to the
right size (corrupt.c). The bytes are then what macOS itself reads; see the README.

Reads the container, object map and tree with apfsread.py (#390's probe, unchanged).
"""
import ctypes
import ctypes.util
import hashlib
import json
import os
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import apfsread as A  # noqa: E402

OID_MASK = (1 << 60) - 1
T_INODE, T_XATTR, T_FEXT, T_DREC = 3, 4, 8, 9
ROOT_DIR = 2
UF_COMPRESSED = 0x20
S_IFMT, S_IFREG = 0o170000, 0o100000
XATTR_DATA_STREAM, XATTR_DATA_EMBEDDED = 1, 2
INO_EXT_TYPE_DSTREAM = 8
BLOCK = 0x10000
PAD = 8
MAX_READ = int(os.environ.get("SILO_MAX_READ", str(1 << 30)))  # bytes one read may produce
MAX_ENTRIES = 10_000_000
ALGORITHMS = {3: "zlib-xattr", 4: "zlib-fork", 7: "lzvn-xattr", 8: "lzvn-fork", 9: "plain-xattr",
              10: "plain-fork", 11: "lzfse-xattr", 12: "lzfse-fork", 13: "lzbitmap-xattr",
              14: "lzbitmap-fork"}


class Gap(Exception):
    """What the reader cannot read, and the rule it broke: said, never guessed."""


def unpack(fmt, buf, off=0, what="record"):
    """R1: struct.unpack_from, or a gap when buf is too short."""
    if off < 0 or off + struct.calcsize(fmt) > len(buf):
        raise Gap("R1 %s too short: %d bytes, %s at %d" % (what, len(buf), fmt, off))
    return struct.unpack_from(fmt, buf, off)


# --- decoders ------------------------------------------------------------------------------------

class Lzfse:
    """LZFSE and LZVN through Apple's code: libcompression on a Mac, liblzfse elsewhere."""

    def __init__(self):
        self.name, self.fn = None, None
        if sys.platform == "darwin":
            try:
                lib = ctypes.CDLL("/usr/lib/libcompression.dylib")
                f = lib.compression_decode_buffer
                f.restype = ctypes.c_size_t
                f.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t,
                              ctypes.c_void_p, ctypes.c_int]
                self.name = "libcompression"
                self.fn = lambda dst, n, src, m: f(dst, n, src, m, None, 0x801)
            except OSError:
                pass
        if self.fn is None:
            try:
                lib = ctypes.CDLL(ctypes.util.find_library("lzfse") or "liblzfse.so.1")
                f = lib.lzfse_decode_buffer
                f.restype = ctypes.c_size_t
                f.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t,
                              ctypes.c_void_p]
                self.name = "liblzfse"
                self.fn = lambda dst, n, src, m: f(dst, n, src, m, None)
            except OSError:
                pass

    def decode(self, data, size):
        if self.fn is None:
            raise Gap("no LZFSE decoder here")
        cap = size + 64
        dst = ctypes.create_string_buffer(cap)
        src = ctypes.create_string_buffer(bytes(data) + b"\0" * PAD, len(data) + PAD)  # R5
        n = self.fn(dst, cap, src, len(data))
        if n != size:
            raise Gap("R5 the decoder gave %d bytes, the block has %d" % (n, size))
        return dst.raw[:n]

    def lzvn(self, data, size):
        return self.decode(b"bvxn" + struct.pack("<II", size, len(data)) + bytes(data) + b"bvx$", size)


LZ = Lzfse()


def decode_chunk(alg, chunk, size):
    if not chunk:
        raise Gap("empty compressed block")
    if alg in (3, 4):
        if chunk[0] & 0x0F == 0x0F:
            out = chunk[1:]
        else:
            d = zlib.decompressobj()
            try:
                out = d.decompress(chunk, size + 1)
            except zlib.error as e:
                raise Gap("zlib: %s" % e)
            if not d.eof or d.unconsumed_tail:
                raise Gap("R5 zlib block does not end where it should")
    elif alg in (7, 8):
        out = chunk[1:] if chunk[0] == 0x06 else LZ.lzvn(chunk, size)
    elif alg in (9, 10):
        out = chunk[1:]
    elif alg in (11, 12):
        out = chunk[1:] if chunk[0] == 0xFF else LZ.decode(chunk, size)
    elif alg in (13, 14):
        if chunk[0] & 0x0F != 0x0F:
            raise Gap("LZBITMAP: no open decoder")
        out = chunk[1:]
    else:
        raise Gap("decmpfs algorithm %d is not known" % alg)
    if len(out) != size:
        raise Gap("R5 a block decoded to %d bytes, not %d" % (len(out), size))
    return bytes(out)


# --- the file-system tree --------------------------------------------------------------------------

class Tree:
    def __init__(self, vol):
        if vol.gap:
            raise Gap("volume %s: %s" % (vol.name, vol.gap))
        if vol.sealed:
            raise Gap("volume %s is sealed: not read here" % vol.name)
        self.d = vol.d
        self.t = vol.fs_tree()
        self.limit = min(self.d.size, MAX_READ)  # R2: no file is larger than this

    def records(self, oid, typ=None):
        out = []

        def key_oid(k):
            return unpack("<Q", k, 0, "key")[0] & OID_MASK

        def walk(paddr, depth):
            if depth > 16:
                raise Gap("R6 tree deeper than 16")
            level, leaf, ents = self.t.node(paddr)
            if leaf:
                for k, v in ents:
                    ko = key_oid(k)
                    if ko == oid and (typ is None or unpack("<Q", k)[0] >> 60 == typ):
                        out.append((k, v))
                    elif ko > oid:
                        return False
                return True
            start = 0
            for i, (k, _) in enumerate(ents):
                if key_oid(k) < oid:
                    start = i
            for k, v in ents[start:]:
                if key_oid(k) > oid:
                    return False
                unpack("<Q", v, 0, "index value")
                if not walk(self.t.child(v)[0], depth + 1):
                    return False
            return True

        walk(self.t.root, 0)
        return out

    def inode(self, oid):
        r = self.records(oid, T_INODE)
        if not r:
            raise Gap("no inode %d" % oid)
        v = r[0][1]
        (parent, private, _c, _m, _ch, _a, _if, _n, _pc, _wg, bsd, _o, _g, mode, _p1,
         usize) = unpack("<QQQQQQQiIIIIIHHQ", v, 0, "inode")
        ino = {"private": private, "bsd_flags": bsd, "mode": mode, "size": None}
        xf = v[92:]
        if len(xf) >= 4:
            n, _used = unpack("<HH", xf, 0, "inode fields")
            pos = 4 + 4 * n
            for i in range(n):
                xt, _xfl, xs = unpack("<BBH", xf, 4 + 4 * i, "inode field")
                if xt == INO_EXT_TYPE_DSTREAM:
                    ino["size"] = unpack("<Q", xf, pos, "dstream")[0]
                pos += (xs + 7) & ~7
        return ino

    def children(self, oid):
        out = {}
        for k, v in self.records(oid, T_DREC):
            if len(k) < 12:
                raise Gap("R1 directory key of %d bytes" % len(k))
            nlen = unpack("<I", k, 8)[0] & 0x3FF
            if 12 + nlen != len(k):
                raise Gap("R1 directory key's name length %d in %d bytes" % (nlen, len(k)))
            name = k[12:12 + nlen].rstrip(b"\0")
            child, _added, flags = unpack("<QQH", v, 0, "directory entry")
            if not name or b"/" in name:
                raise Gap("R1 a name that is empty or holds /")
            out[name.decode("utf-8", "surrogateescape")] = (child, flags & 0xF)
        return out

    def lookup(self, path):
        oid = ROOT_DIR
        for part in [p for p in path.split("/") if p]:
            kids = self.children(oid)
            if part not in kids:
                raise Gap("%s: no %r" % (path, part))
            oid = kids[part][0]
        return oid

    def xattr(self, oid, want):
        for k, v in self.records(oid, T_XATTR):
            nlen = unpack("<H", k, 8, "xattr key")[0]
            if 10 + nlen > len(k):
                raise Gap("R1 xattr name past its key")
            if k[10:10 + nlen].rstrip(b"\0").decode("utf-8", "replace") != want:
                continue
            flags, dlen = unpack("<HH", v, 0, "xattr")
            xd = v[4:4 + dlen]
            if len(xd) != dlen:
                raise Gap("R1 xattr data past its value")
            if flags & XATTR_DATA_EMBEDDED:
                return bytes(xd)
            if flags & XATTR_DATA_STREAM:
                sid, size = unpack("<QQ", xd, 0, "xattr stream")
                return self.stream(sid, size)
            raise Gap("xattr %s of %d has neither data nor a stream" % (want, oid))
        return None

    def stream(self, oid, size):
        """R2, R3: the bytes of a data stream, never more than its size or the limit."""
        if size > self.limit:
            raise Gap("R2 a stream of %d bytes, over the limit of %d" % (size, self.limit))
        out = bytearray(size)  # holes stay zero; bounded by the check above
        at = 0
        for k, v in self.records(oid, T_FEXT):
            logical = unpack("<Q", k, 8, "extent key")[0]
            lf, phys, _crypto = unpack("<QQQ", v, 0, "extent")
            length = lf & ((1 << 56) - 1)
            if logical < at:
                raise Gap("R3 extents of %d overlap or are out of order at %d" % (oid, logical))
            if length == 0 or length % self.d.bs:
                raise Gap("R3 an extent of %d bytes" % length)
            if logical >= size:
                break
            n = min(length, size - logical)
            if phys:
                if phys * self.d.bs + n > self.d.size:
                    raise Gap("R3 an extent outside the container")
                self.d.f.seek(self.d.offset + phys * self.d.bs)
                b = self.d.f.read(n)
                if len(b) != n:
                    raise Gap("short read")
                out[logical:logical + n] = b
            at = logical + length
        return bytes(out)

    def read(self, oid):
        ino = self.inode(oid)
        if ino["mode"] & S_IFMT != S_IFREG:
            raise Gap("inode %d is not a regular file" % oid)
        if not ino["bsd_flags"] & UF_COMPRESSED:
            return self.stream(ino["private"], ino["size"] or 0), None
        hdr = self.xattr(oid, "com.apple.decmpfs")
        if hdr is None or len(hdr) < 16:
            raise Gap("inode %d is compressed and has no decmpfs header" % oid)
        magic, alg, usize = unpack("<IIQ", hdr, 0, "decmpfs header")
        if magic != 0x636D7066:
            raise Gap("decmpfs magic %#x" % magic)
        if alg not in ALGORITHMS:
            raise Gap("decmpfs algorithm %d is not known" % alg)
        if usize > self.limit:
            raise Gap("R2 a file of %d bytes, over the limit" % usize)
        if usize == 0:
            return b"", alg
        if alg % 2 == 1:
            if usize > BLOCK:
                raise Gap("%d bytes in the xattr, over one block" % usize)
            return decode_chunk(alg, hdr[16:], usize), alg
        fork = self.xattr(oid, "com.apple.ResourceFork")
        if fork is None:
            raise Gap("compressed in a resource fork it does not have")
        nblocks = (usize + BLOCK - 1) // BLOCK
        if alg in (4, 10):
            data_off = unpack(">I", fork, 0, "fork header")[0]
            count = unpack("<I", fork, data_off + 4, "fork table")[0]
            if count != nblocks:
                raise Gap("R4 %d blocks in the fork, %d from the size" % (count, nblocks))
            unpack("<%dI" % (2 * count), fork, data_off + 8, "fork table")
            entries = []
            for i in range(count):
                off, ln = unpack("<II", fork, data_off + 8 + 8 * i)
                entries.append((data_off + 4 + off, ln))
        else:
            offs = unpack("<%dI" % (nblocks + 1), fork, 0, "fork table")
            entries = [(offs[i], offs[i + 1] - offs[i]) for i in range(nblocks)]
        for i, (off, ln) in enumerate(entries):  # R4: the whole table, before any decoding
            if ln <= 0 or off + ln > len(fork):
                raise Gap("R4 block %d is empty or past the end of its fork" % i)
        out = [decode_chunk(alg, fork[off:off + ln], min(BLOCK, usize - i * BLOCK))
               for i, (off, ln) in enumerate(entries)]
        return b"".join(out), alg

    def walk(self, oid=ROOT_DIR, path="/"):
        """R6: (path, oid, type) of everything under a directory, each directory once."""
        seen, count, stack = {oid}, 0, [(oid, path, 0)]
        while stack:
            d, p, depth = stack.pop()
            if depth > 64:
                raise Gap("R6 directories deeper than 64")
            for name, (child, typ) in sorted(self.children(d).items()):
                count += 1
                if count > MAX_ENTRIES:
                    raise Gap("R6 more than %d entries" % MAX_ENTRIES)
                cp = p.rstrip("/") + "/" + name
                yield cp, child, typ
                if typ == 4:
                    if child in seen:
                        raise Gap("R6 directory %d reached twice" % child)
                    seen.add(child)
                    stack.append((child, cp, depth + 1))


def open_volume(img, want=None):
    f, cs = A.open_image(img)
    vols = [v for c in cs for v in c.get("_vols", [])]
    if want:
        pick = [v for v in vols if v.name == want or v.role_name() == want]
    else:
        pick = [v for v in vols if v.role_name() == "data"] or [v for v in vols if not v.sealed] or vols
    if not pick:
        raise Gap("no volume to read")
    return Tree(pick[0])


# apfsread's own refusals (container, object map, tree nodes) are gaps too.
GAPS = (Gap, A.Bad)


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    cmd, img = argv[0], argv[1]
    t = open_volume(img)
    if cmd == "ls":
        under = argv[argv.index("--under") + 1] if "--under" in argv else "/"
        base = t.lookup(under) if under != "/" else ROOT_DIR
        for p, oid, typ in t.walk(base, under):
            if typ != 8:
                continue
            rec = {"path": p}
            try:
                data, alg = t.read(oid)
                rec.update({"size": len(data), "compressed": ALGORITHMS.get(alg) if alg else None,
                            "sha256": hashlib.sha256(data).hexdigest()})
            except GAPS as e:
                rec["gap"] = str(e)
            print(json.dumps(rec), flush=True)
        return 0
    if cmd == "cat":
        data, _ = t.read(t.lookup(argv[2]))
        sys.stdout.buffer.write(data)
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except GAPS as e:
        print(json.dumps({"gap": str(e)}))
        sys.exit(1)
