"""Reads the contents of files from an APFS image, from outside: the second mode #370 gives Silo's
reader (finding 6), for `look` and `put-back`. Read only, no decryption, no mount.

  apfscontent.py ls IMG [--volume ROLE_OR_NAME] [--under PATH]
        one JSON line per file under PATH: path, size, compressed (the decmpfs algorithm or null),
        and the sha256 of its contents, or the reason it could not be read (a gap, never a guess)
  apfscontent.py cat IMG PATH [--volume ROLE_OR_NAME]
        the contents of one file on stdout
  apfscontent.py decoders
        which decoder each compression uses on this machine (JSON)

It reads what decmpfs, APFS's file compression, stores: the header in the xattr com.apple.decmpfs
and the data either after it or in the xattr com.apple.ResourceFork, in blocks of 64 KiB. zlib is
Python's own. LZVN and LZFSE go to Apple's code: libcompression on a Mac, liblzfse (Apple's
open-source lzfse) elsewhere. LZBITMAP has no open decoder: a file in it is a gap.

Written from the Apple File System Reference (2020-06-22) and Apple's decmpfs.h (xnu), on
apfsread.py. A probe for test 39, not Silo's reader.
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
S_IFMT, S_IFREG, S_IFDIR, S_IFLNK = 0o170000, 0o100000, 0o040000, 0o120000
XATTR_DATA_STREAM, XATTR_DATA_EMBEDDED = 1, 2
INO_EXT_TYPE_DSTREAM = 8
BLOCK = 0x10000
ALGORITHMS = {3: "zlib-xattr", 4: "zlib-fork", 7: "lzvn-xattr", 8: "lzvn-fork", 9: "plain-xattr",
              10: "plain-fork", 11: "lzfse-xattr", 12: "lzfse-fork", 13: "lzbitmap-xattr",
              14: "lzbitmap-fork"}


class Gap(A.Bad):
    """What the reader cannot read: said, never guessed."""


# --- decoders ------------------------------------------------------------------------------------

class Lzfse:
    """LZFSE and LZVN through Apple's code. LZVN blocks are wrapped in lzfse's own 'bvxn' block,
    which both libcompression's and liblzfse's LZFSE decoders read."""

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
                self.fn = lambda dst, n, src, m: f(dst, n, src, m, None, 0x801)  # COMPRESSION_LZFSE
            except OSError:
                pass
        if self.fn is None:
            path = ctypes.util.find_library("lzfse") or "liblzfse.so.1"
            try:
                lib = ctypes.CDLL(path)
                f = lib.lzfse_decode_buffer
                f.restype = ctypes.c_size_t
                f.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
                self.name = "liblzfse"
                self.fn = lambda dst, n, src, m: f(dst, n, src, m, None)
            except OSError:
                pass

    def decode(self, data, size):
        if self.fn is None:
            raise Gap("no LZFSE decoder here (libcompression or liblzfse)")
        cap = size + 64  # room past the end, so a decoder that would write more is seen
        dst = ctypes.create_string_buffer(cap)
        n = self.fn(dst, cap, data, len(data))
        if n != size:
            raise Gap("LZFSE decoder gave %d bytes, the header says %d" % (n, size))
        return dst.raw[:n]

    def lzvn(self, data, size):
        block = b"bvxn" + struct.pack("<II", size, len(data)) + data + b"bvx$"
        return self.decode(block, size)


LZ = Lzfse()


def decode_chunk(alg, chunk, size):
    """One block of decmpfs data, size bytes once decoded."""
    if not chunk:
        raise Gap("empty compressed block")
    if alg in (3, 4):
        if chunk[0] & 0x0F == 0x0F:
            out = chunk[1:]
        else:
            out = zlib.decompress(chunk)
    elif alg in (7, 8):
        out = chunk[1:] if chunk[0] == 0x06 else LZ.lzvn(chunk, size)
    elif alg in (9, 10):
        out = chunk[1:]
    elif alg in (11, 12):
        out = chunk[1:] if chunk[0] == 0xFF else LZ.decode(chunk, size)
    elif alg in (13, 14):
        if chunk[0] & 0x0F == 0x0F:
            out = chunk[1:]
        else:
            raise Gap("LZBITMAP: no open decoder")
    else:
        raise Gap("decmpfs algorithm %d is not known" % alg)
    if len(out) != size:
        raise Gap("a block of %s decoded to %d bytes, not %d" % (ALGORITHMS.get(alg, alg), len(out), size))
    return out


# --- the file-system tree --------------------------------------------------------------------------

class Tree:
    """The file-system tree of one volume, looked up by object id."""

    def __init__(self, vol):
        if vol.gap:
            raise Gap("volume %s: %s" % (vol.name, vol.gap))
        if vol.sealed:
            raise Gap("volume %s is sealed: its file extents are in the fext tree, which this probe does not read" % vol.name)
        self.vol = vol
        self.d = vol.d
        self.t = vol.fs_tree()

    def records(self, oid, typ=None):
        """[(key, value)] of every record of oid (of type typ), in key order."""
        out = []

        def key_oid(k):
            return struct.unpack_from("<Q", k)[0] & OID_MASK

        def walk(paddr, depth):
            if depth > 16:
                raise A.Bad("tree deeper than 16")
            level, leaf, ents = self.t.node(paddr)
            if leaf:
                for k, v in ents:
                    ko = key_oid(k)
                    if ko == oid and (typ is None or struct.unpack_from("<Q", k)[0] >> 60 == typ):
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
         usize) = struct.unpack_from("<QQQQQQQiIIIIIHHQ", v, 0)
        ino = {"oid": oid, "private": private, "bsd_flags": bsd, "mode": mode, "size": None,
               "uncompressed_size": usize}
        xf = v[92:]
        if len(xf) >= 4:
            n, _used = struct.unpack_from("<HH", xf, 0)
            pos = 4 + 4 * n
            for i in range(n):
                xt, _xfl, xs = struct.unpack_from("<BBH", xf, 4 + 4 * i)
                if xt == INO_EXT_TYPE_DSTREAM and pos + 8 <= len(xf):
                    ino["size"] = struct.unpack_from("<Q", xf, pos)[0]
                pos += (xs + 7) & ~7
        return ino

    def children(self, oid):
        """{name: (child oid, type)} of a directory."""
        out = {}
        for k, v in self.records(oid, T_DREC):
            if len(k) >= 12:
                nlh = struct.unpack_from("<I", k, 8)[0]
                nlen = nlh & 0x3FF
                if 12 + nlen == len(k):
                    name = k[12:12 + nlen]
                else:  # a key without the hash
                    nlen = struct.unpack_from("<H", k, 8)[0]
                    name = k[10:10 + nlen]
            else:
                continue
            child, _added, flags = struct.unpack_from("<QQH", v, 0)
            out[name.rstrip(b"\0").decode("utf-8", "replace")] = (child, flags & 0xF)
        return out

    def lookup(self, path):
        oid = ROOT_DIR
        for part in [p for p in path.split("/") if p]:
            kids = self.children(oid)
            if part not in kids:
                raise Gap("%s: no %r" % (path, part))
            oid = kids[part][0]
        return oid

    def xattrs(self, oid):
        """{name: (flags, data)}: data is the bytes when embedded, (stream oid, size) otherwise."""
        out = {}
        for k, v in self.records(oid, T_XATTR):
            nlen = struct.unpack_from("<H", k, 8)[0]
            name = k[10:10 + nlen].rstrip(b"\0").decode("utf-8", "replace")
            flags, dlen = struct.unpack_from("<HH", v, 0)
            xd = v[4:4 + dlen]
            if flags & XATTR_DATA_EMBEDDED:
                out[name] = (flags, bytes(xd))
            elif flags & XATTR_DATA_STREAM:
                sid, size = struct.unpack_from("<QQ", xd, 0)
                out[name] = (flags, (sid, size))
            else:
                out[name] = (flags, None)
        return out

    def stream(self, oid, size):
        """The bytes of a data stream (a file's, or an xattr's), from its file extents."""
        parts, at = [], 0
        for k, v in self.records(oid, T_FEXT):
            logical = struct.unpack_from("<Q", k, 8)[0]
            lf, phys, _crypto = struct.unpack_from("<QQQ", v, 0)
            length = lf & ((1 << 56) - 1)
            if logical != at:
                if logical < at:
                    raise A.Bad("stream %d: extents overlap at %d" % (oid, logical))
                parts.append(b"\0" * (logical - at))
                at = logical
            if phys == 0:
                parts.append(b"\0" * length)
            else:
                if (phys * self.d.bs + length) > self.d.size:
                    raise A.Bad("stream %d: extent outside the container" % oid)
                self.d.f.seek(self.d.offset + phys * self.d.bs)
                b = self.d.f.read(length)
                if len(b) != length:
                    raise A.Bad("stream %d: short read" % oid)
                parts.append(b)
            at += length
            if at >= size:
                break
        if at < size:
            parts.append(b"\0" * (size - at))
        return b"".join(parts)[:size]

    def xattr_bytes(self, oid, name):
        x = self.xattrs(oid).get(name)
        if x is None:
            return None
        flags, data = x
        if isinstance(data, bytes):
            return data
        if data is None:
            raise Gap("xattr %s of %d has neither data nor a stream" % (name, oid))
        return self.stream(data[0], data[1])

    def read(self, oid):
        """(contents, decmpfs algorithm or None) of a regular file."""
        ino = self.inode(oid)
        if ino["mode"] & S_IFMT != S_IFREG:
            raise Gap("inode %d is not a regular file" % oid)
        if not ino["bsd_flags"] & UF_COMPRESSED:
            return self.stream(ino["private"], ino["size"] or 0), None
        hdr = self.xattr_bytes(oid, "com.apple.decmpfs")
        if hdr is None or len(hdr) < 16:
            raise Gap("inode %d is compressed and has no decmpfs header" % oid)
        magic, alg, usize = struct.unpack_from("<IIQ", hdr, 0)
        if magic != 0x636D7066:  # "fpmc" on disk
            raise Gap("inode %d: decmpfs magic %#x" % (oid, magic))
        if alg not in ALGORITHMS:
            raise Gap("inode %d: decmpfs algorithm %d is not known" % (oid, alg))
        if usize == 0:
            return b"", alg
        if alg % 2 == 1:  # in the xattr, after the header: one block
            if usize > BLOCK:
                raise Gap("inode %d: %d bytes in the xattr, over one block" % (oid, usize))
            return decode_chunk(alg, hdr[16:], usize), alg
        fork = self.xattr_bytes(oid, "com.apple.ResourceFork")
        if fork is None:
            raise Gap("inode %d: compressed in a resource fork it does not have" % oid)
        nblocks = (usize + BLOCK - 1) // BLOCK
        entries = []
        if alg in (4, 10):  # a resource fork, as HFS+ wrote it
            data_off = struct.unpack_from(">I", fork, 0)[0]
            count = struct.unpack_from("<I", fork, data_off + 4)[0]
            if count != nblocks:
                raise Gap("inode %d: %d blocks in the fork, %d from the size" % (oid, count, nblocks))
            for i in range(count):
                off, ln = struct.unpack_from("<II", fork, data_off + 8 + 8 * i)
                entries.append((data_off + 4 + off, ln))
        else:  # a table of offsets, one more than the blocks
            offs = struct.unpack_from("<%dI" % (nblocks + 1), fork, 0)
            entries = [(offs[i], offs[i + 1] - offs[i]) for i in range(nblocks)]
        out = []
        for i, (off, ln) in enumerate(entries):
            if off + ln > len(fork):
                raise Gap("inode %d: block %d is past the end of its fork" % (oid, i))
            out.append(decode_chunk(alg, fork[off:off + ln], min(BLOCK, usize - i * BLOCK)))
        return b"".join(out), alg

    def walk(self, oid, path, depth=0):
        """Yields (path, oid, type) of everything under a directory."""
        if depth > 64:
            return
        for name, (child, typ) in sorted(self.children(oid).items()):
            p = path.rstrip("/") + "/" + name
            yield p, child, typ
            if typ == 4:  # DT_DIR
                yield from self.walk(child, p, depth + 1)


def open_volume(img, want=None):
    f, cs = A.open_image(img)
    vols = [v for c in cs for v in c.get("_vols", [])]
    if want:
        pick = [v for v in vols if v.name == want or v.role_name() == want]
    else:
        pick = [v for v in vols if v.role_name() == "data"] or [v for v in vols if not v.sealed] or vols
    if not pick:
        raise Gap("no volume %r in %s: %s" % (want, img, [(v.name, v.role_name()) for v in vols]))
    return Tree(pick[0])


def cmd_ls(img, want, under):
    t = open_volume(img, want)
    base = t.lookup(under) if under not in ("", "/") else ROOT_DIR
    for path, oid, typ in t.walk(base, under or "/"):
        if typ != 8:  # DT_REG
            continue
        rec = {"path": path}
        try:
            data, alg = t.read(oid)
            rec.update({"size": len(data), "compressed": ALGORITHMS.get(alg) if alg else None,
                        "sha256": hashlib.sha256(data).hexdigest()})
        except (Gap, A.Bad, zlib.error, struct.error) as e:
            rec.update({"gap": str(e)})
            try:
                hdr = t.xattr_bytes(oid, "com.apple.decmpfs")
                if hdr and len(hdr) >= 8:
                    rec["compressed"] = ALGORITHMS.get(struct.unpack_from("<I", hdr, 4)[0])
            except (A.Bad, struct.error):
                pass
        print(json.dumps(rec), flush=True)


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    cmd, rest = argv[0], argv[1:]
    want = None
    if "--volume" in rest:
        i = rest.index("--volume")
        want = rest[i + 1]
        del rest[i:i + 2]
    under = "/"
    if "--under" in rest:
        i = rest.index("--under")
        under = rest[i + 1]
        del rest[i:i + 2]
    if cmd == "decoders":
        print(json.dumps({"zlib": "python", "lzvn": LZ.name, "lzfse": LZ.name, "lzbitmap": None}))
        return 0
    if cmd == "ls":
        cmd_ls(rest[0], want, under)
        return 0
    if cmd == "cat":
        t = open_volume(rest[0], want)
        data, _ = t.read(t.lookup(rest[1]))
        sys.stdout.buffer.write(data)
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except A.Bad as e:
        print(json.dumps({"gap": str(e)}))
        sys.exit(1)
