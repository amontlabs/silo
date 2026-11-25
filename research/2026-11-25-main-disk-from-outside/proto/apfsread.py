"""A small APFS reader for the kit: metadata only, read only, no decryption.

(Copied from #390, mac-test-kit/py/apfs/apfsread.py at 15820da, with one change: child() refuses
an index value shorter than 8 bytes, which fuzz_reader.py found crashing the probe.)

Written from the Apple File System Reference (2020-06-22), after the 250-line prototype of
research 11 (`apfs/scripts/apfsdiff.py` of the research scripts folder). Pure Python 3.9, no
numpy. Every block it reads is counted and its Fletcher-64 checksum checked. It is a probe for
tests 17, 18 and 19, not Silo's reader: it checks checksums, types and bounds, not every rule
apfsck checks.

  apfsread.py info IMG                 containers, checkpoints, volumes (JSON)
  apfsread.py checkpoint IMG [--deep]  is the newest checkpoint of each container whole? (JSON)
  apfsread.py leafdiff A B [--volume ROLE_OR_NAME] [--full]
                                       diff of the file-system trees of A and B by leaf; with
                                       --full, also by a walk of every leaf, and whether both
                                       agree (JSON). A or B may be IMG@SNAPSHOT_NAME or
                                       IMG@#N (the Nth snapshot, from 0) to read a snapshot.

IMG is a raw disk image (GPT with APFS partitions, as a macOS computer's disk) or a bare APFS
container. An ASIF image is not raw: convert it first.

Leaves are compared by (object id, version xid, block), all three from the object map, never by
block alone. Two clones of a running disk are not two snapshots of one container: a block that
held a leaf in the older clone can have been freed and reused for another leaf by the newer one.
"""
import itertools
import json
import os
import struct
import sys
import time
import uuid
from array import array

M32 = 0xFFFFFFFF
APFS_GUIDS = {
    uuid.UUID("7C3457EF-0000-11AA-AA11-00306543ECAC"): "apfs",
    uuid.UUID("69646961-6700-11AA-AA11-00306543ECAC"): "apfs-isc",
    uuid.UUID("52637672-7900-11AA-AA11-00306543ECAC"): "apfs-recovery",
}
ROLES = {0: "none", 1: "system", 2: "user", 4: "recovery", 8: "vm", 0x10: "preboot",
         0x20: "installer", 0x40: "data", 0x80: "baseband", 0xC0: "update", 0x100: "xart",
         0x140: "hardware", 0x180: "backup", 0x1C0: "sidecar", 0x200: "enterprise",
         0x240: "reserved", 0x280: "prelogin"}
T_NXSB, T_BTREE, T_NODE, T_OMAP, T_CPMAP, T_FS = 1, 2, 3, 0xB, 0xC, 0xD
OMAP_VAL_DELETED, OMAP_VAL_ENCRYPTED = 1, 4
APFS_FS_UNENCRYPTED = 1
INCOMPAT_SEALED = 0x20
REC_TYPES = {1: "snap_meta", 2: "extent", 3: "inode", 4: "xattr", 5: "sibling_link",
             6: "dstream_id", 7: "crypto_state", 8: "file_extent", 9: "dir_rec",
             10: "dir_stats", 11: "snap_name", 12: "sibling_map", 13: "file_info"}
assert array("I").itemsize == 4


class Bad(Exception):
    """The disk breaks a rule the reader checks: a gap, never a guess."""


def fletcher64(block):
    w = array("I", block[8:])
    if sys.byteorder != "little":
        w.byteswap()
    s1 = sum(w) % M32
    s2 = sum(itertools.accumulate(w)) % M32
    c1 = M32 - ((s1 + s2) % M32)
    c2 = M32 - ((s1 + c1) % M32)
    return (c2 << 32) | c1


def hdr(b):
    cks, oid, xid, typ, sub = struct.unpack_from("<QQQII", b, 0)
    return {"oid": oid, "xid": xid, "type": typ & 0xFFFF, "flags": typ >> 16, "subtype": sub}


class Disk:
    """One APFS container inside an image file, at a byte offset."""

    def __init__(self, f, offset, size, budget=50_000_000):
        self.f, self.offset, self.size = f, offset, size
        self.bs = 4096
        self.reads = 0
        self.budget = budget

    def raw(self, paddr):
        if paddr < 0 or (paddr + 1) * self.bs > self.size:
            raise Bad("block %d is outside the container" % paddr)
        self.reads += 1
        if self.reads > self.budget:
            raise Bad("block budget spent")
        self.f.seek(self.offset + paddr * self.bs)
        b = self.f.read(self.bs)
        if len(b) != self.bs:
            raise Bad("short read at block %d" % paddr)
        return b

    def block(self, paddr, want_type=None):
        b = self.raw(paddr)
        c = struct.unpack_from("<Q", b, 0)[0]
        if c == 0 and not any(b):
            raise Bad("block %d is all zero" % paddr)
        if c != fletcher64(b):
            raise Bad("bad checksum at block %d" % paddr)
        if want_type is not None and hdr(b)["type"] != want_type:
            raise Bad("block %d has type %#x, not %#x" % (paddr, hdr(b)["type"], want_type))
        return b


def find_containers(path):
    """[(name, byte offset, byte size)] of the APFS containers of an image."""
    size = os.path.getsize(path)
    with open(path, "rb") as f:
        head = f.read(65536)
    if head[32:36] == b"NXSB":
        return [("container", 0, size)]
    for sector in (512, 4096):
        h = head[sector:sector + 92]
        if h[:8] != b"EFI PART":
            continue
        ent_lba, n, esz = struct.unpack_from("<QII", h, 72)
        if not (0 < n <= 1024 and 128 <= esz <= 4096):
            raise Bad("GPT header out of bounds")
        out = []
        with open(path, "rb") as f:
            f.seek(ent_lba * sector)
            tab = f.read(n * esz)
        for i in range(n):
            e = tab[i * esz:(i + 1) * esz]
            if len(e) < 56 or not any(e[:16]):
                continue
            t = uuid.UUID(bytes_le=bytes(e[:16]))
            first, last = struct.unpack_from("<QQ", e, 32)
            if t in APFS_GUIDS:
                name = e[56:128].decode("utf-16-le", "replace").split("\0")[0] or APFS_GUIDS[t]
                out.append(("%d:%s" % (i + 1, name), first * sector, (last - first + 1) * sector))
        return out
    return []


def nx_fields(b):
    f = {"xid": hdr(b)["xid"]}
    (f["block_size"], f["block_count"]) = struct.unpack_from("<IQ", b, 36)
    f["next_xid"] = struct.unpack_from("<Q", b, 96)[0]
    (f["desc_blocks"], f["data_blocks"], f["desc_base"], f["data_base"],
     f["desc_next"], f["data_next"], f["desc_index"], f["desc_len"],
     f["data_index"], f["data_len"]) = struct.unpack_from("<IIQQIIIIII", b, 104)
    f["omap_oid"] = struct.unpack_from("<Q", b, 160)[0]
    f["max_fs"] = struct.unpack_from("<I", b, 180)[0]
    f["fs_oids"] = [o for o in struct.unpack_from("<100Q", b, 184) if o]
    return f


class Container:
    def __init__(self, f, name, offset, size):
        self.name = name
        self.d = Disk(f, offset, size)
        b0 = self.d.raw(0)
        if b0[32:36] != b"NXSB":
            raise Bad("no NXSB magic in block 0")
        bs = struct.unpack_from("<I", b0, 36)[0]
        if bs not in (4096, 8192, 16384, 32768, 65536):
            raise Bad("block size %d" % bs)
        self.d.bs = bs
        self.b0 = nx_fields(b0)
        self.b0_checksum_ok = struct.unpack_from("<Q", b0, 0)[0] == fletcher64(b0)

    def superblocks(self):
        """Every superblock of the checkpoint descriptor area that checks, newest first."""
        f = self.b0
        if f["desc_blocks"] & 0x80000000:
            raise Bad("checkpoint descriptor area is a tree (not handled by this probe)")
        n = f["desc_blocks"]
        if not 0 < n <= 65536:
            raise Bad("descriptor area size %d" % n)
        found, bad = [], 0
        for i in range(n):
            try:
                b = self.d.block(f["desc_base"] + i)
            except Bad:
                if any(self.d.raw(f["desc_base"] + i)):
                    bad += 1
                continue
            h = hdr(b)
            if h["type"] == T_NXSB and b[32:36] == b"NXSB":
                found.append((h["xid"], i, b))
        found.sort(key=lambda x: -x[0])
        return found, bad

    def check(self, xid, ring_index, sb, deep=False):
        """Raises Bad when the checkpoint whose superblock is sb is not whole."""
        nx = nx_fields(sb)
        f = self.b0
        n = f["desc_blocks"]
        # the checkpoint's descriptor blocks: desc_len blocks ending with the superblock
        if not 0 < nx["desc_len"] <= n:
            raise Bad("checkpoint length %d" % nx["desc_len"])
        maps = 0
        for k in range(nx["desc_len"] - 1):
            i = (nx["desc_index"] + k) % n
            b = self.d.block(f["desc_base"] + i)
            h = hdr(b)
            if h["xid"] != xid:
                raise Bad("descriptor block %d has xid %d, not %d" % (i, h["xid"], xid))
            if h["type"] != T_CPMAP:
                raise Bad("descriptor block %d is type %#x, not a checkpoint map" % (i, h["type"]))
            maps += 1
            count = struct.unpack_from("<I", b, 36)[0]
            if count > (self.d.bs - 40) // 40:
                raise Bad("checkpoint map count %d" % count)
            for m in range(count):
                typ, sub, size, pad, fs_oid, oid, paddr = struct.unpack_from("<IIIIQQQ", b, 40 + m * 40)
                if size % self.d.bs or size == 0 or size > 64 * self.d.bs:
                    raise Bad("ephemeral object size %d" % size)
                whole = b"".join(self.d.raw(paddr + j) for j in range(size // self.d.bs))
                if struct.unpack_from("<Q", whole, 0)[0] != fletcher64(whole):
                    raise Bad("ephemeral object %d (type %#x) at %d has a bad checksum" % (oid, typ & 0xFFFF, paddr))
                eh = hdr(whole)
                if eh["oid"] != oid:
                    raise Bad("ephemeral object at %d has oid %d, not %d" % (paddr, eh["oid"], oid))
                if eh["xid"] > xid:
                    raise Bad("ephemeral object %d is newer (%d) than its checkpoint" % (oid, eh["xid"]))
        if maps == 0 and nx["desc_len"] > 1:
            raise Bad("no checkpoint map")
        com = OMap(self.d, nx["omap_oid"], xid)
        vols = []
        for i, fs_oid in enumerate(nx["fs_oids"][: nx["max_fs"] or 100]):
            vols.append(Volume(self.d, com, fs_oid, xid, deep=deep))
        return nx, vols

    def open(self, deep=False):
        """The newest checkpoint that is whole, as macOS opens it. Returns a dict."""
        found, bad_blocks = self.superblocks()
        if not found:
            raise Bad("no valid superblock in the descriptor area")
        tried = []
        for xid, idx, sb in found:
            try:
                nx, vols = self.check(xid, idx, sb, deep=deep)
                return {"newest_xid": found[0][0], "opened_xid": xid, "newest_ok": xid == found[0][0],
                        "skipped": tried, "superblocks": [x for x, _, _ in found][:8],
                        "descriptor_blocks_failing_checksum": bad_blocks,
                        "block0_xid": self.b0["xid"], "nx": nx, "volumes": vols}
            except Bad as e:
                tried.append({"xid": xid, "why": str(e)})
        raise Bad("no checkpoint opens: " + "; ".join("%d: %s" % (t["xid"], t["why"]) for t in tried[:4]))


class BTree:
    """A B-tree; resolve(oid) -> (paddr, version xid) for a virtual tree, None for physical."""

    def __init__(self, d, root_paddr, resolve=None, fixed=None):
        self.d, self.root, self.resolve, self.fixed = d, root_paddr, resolve, fixed

    def node(self, paddr):
        b = self.d.block(paddr, T_NODE if paddr != self.root else None)
        t = hdr(b)["type"]
        if t not in (T_BTREE, T_NODE):
            raise Bad("block %d is type %#x, not a B-tree node" % (paddr, t))
        flags, level, nkeys = struct.unpack_from("<HHI", b, 32)
        toff, tlen = struct.unpack_from("<HH", b, 40)
        is_root, is_leaf, is_fixed = flags & 1, flags & 2, flags & 4
        if bool(is_leaf) != (level == 0):
            raise Bad("node %d: leaf flag and level disagree" % paddr)
        kstart = 56 + toff + tlen
        vend = self.d.bs - (40 if is_root else 0)
        if is_root and self.fixed is None and is_fixed:
            ks, vs = struct.unpack_from("<II", b, self.d.bs - 40 + 8)
            self.fixed = (ks, vs)
        step = 4 if is_fixed else 8
        if nkeys * step > tlen or kstart > vend:
            raise Bad("node %d: %d keys do not fit their table" % (paddr, nkeys))
        ents = []
        for i in range(nkeys):
            if is_fixed:
                ko, vo = struct.unpack_from("<HH", b, 56 + toff + i * 4)
                if self.fixed is None:
                    raise Bad("fixed-size node without sizes")
                kl, vl = self.fixed if is_leaf else (self.fixed[0], 8)
            else:
                ko, kl, vo, vl = struct.unpack_from("<HHHH", b, 56 + toff + i * 8)
            if kstart + ko + kl > vend:
                raise Bad("node %d: key %d out of bounds" % (paddr, i))
            k = bytes(b[kstart + ko: kstart + ko + kl])
            if vo == 0xFFFF:
                v = b""
            else:
                if vo > vend - kstart or vo < vl:
                    raise Bad("node %d: value %d out of bounds" % (paddr, i))
                v = bytes(b[vend - vo: vend - vo + vl])
            ents.append((k, v))
        return level, bool(is_leaf), ents

    def child(self, v):
        if len(v) < 8:  # rule R1 of content_reader.py, found by fuzz_reader.py
            raise Bad("index value of %d bytes" % len(v))
        c = struct.unpack_from("<Q", v, 0)[0]
        return self.resolve(c) if self.resolve else (c, None, c)

    def leaves(self, on_index=None):
        """Yields (paddr, version xid, oid) of every leaf, reading index nodes only."""
        root = (self.root, None, None)
        seen = set()

        def walk(ref, depth, want_level):
            paddr = ref[0]
            if depth > 16:
                raise Bad("tree deeper than 16")
            if paddr in seen:
                raise Bad("block %d reached twice" % paddr)
            seen.add(paddr)
            level, leaf, ents = self.node(paddr)
            if want_level is not None and level != want_level:
                raise Bad("node %d at level %d, its parent says %d" % (paddr, level, want_level))
            if leaf:
                yield ref
                return
            if on_index:
                on_index(paddr)
            for k, v in ents:
                c = self.child(v)
                if level == 1:
                    if c[0] in seen:
                        raise Bad("leaf %d listed twice" % c[0])
                    seen.add(c[0])
                    yield c
                else:
                    yield from walk(c, depth + 1, level - 1)

        yield from walk(root, 0, None)

    def records(self, leaf_paddr):
        level, leaf, ents = self.node(leaf_paddr)
        if not leaf:
            raise Bad("block %d is not a leaf" % leaf_paddr)
        return ents


class OMap:
    """An object map: oid -> [(xid, paddr, flags)], every version."""

    def __init__(self, d, omap_paddr, max_xid=None):
        b = d.block(omap_paddr, T_OMAP)
        tree_paddr = struct.unpack_from("<Q", b, 48)[0]
        self.snap_tree = struct.unpack_from("<Q", b, 56)[0]
        self.tree = BTree(d, tree_paddr, fixed=(16, 16))
        self.entries = {}
        for leaf, _, _ in self.tree.leaves():
            for k, v in self.tree.records(leaf):
                oid, xid = struct.unpack("<QQ", k)
                fl, size, paddr = struct.unpack("<IIQ", v)
                if max_xid is not None and xid > max_xid:
                    raise Bad("object map entry %d xid %d is newer than its checkpoint %d" % (oid, xid, max_xid))
                self.entries.setdefault(oid, []).append((xid, paddr, fl))
        for lst in self.entries.values():
            lst.sort()
        self.leaves = len(self.entries)

    def lookup(self, oid, xid):
        best = None
        for x, p, fl in self.entries.get(oid, ()):
            if x <= xid:
                best = (x, p, fl)
        if best is None or best[2] & OMAP_VAL_DELETED:
            raise Bad("object %d not in the object map at xid %d" % (oid, xid))
        if best[2] & OMAP_VAL_ENCRYPTED:
            raise Encrypted("object %d is encrypted" % oid)
        return best[1], best[0], oid


class Encrypted(Bad):
    pass


class Volume:
    def __init__(self, d, com, fs_oid, xid, deep=False):
        self.d = d
        paddr, vx, _ = com.lookup(fs_oid, xid)
        vb = d.block(paddr, T_FS)
        if vb[32:36] != b"APSB":
            raise Bad("volume %d: no APSB magic" % fs_oid)
        self.fs_oid = fs_oid
        self.xid = hdr(vb)["xid"]
        self.incompat = struct.unpack_from("<Q", vb, 56)[0]
        (self.omap_oid, self.root_tree_oid, self.extentref_oid,
         self.snap_meta_oid) = struct.unpack_from("<QQQQ", vb, 128)
        self.num_files, self.num_dirs, self.num_symlinks, self.num_other, self.num_snapshots = \
            struct.unpack_from("<QQQQQ", vb, 184)
        self.fs_flags = struct.unpack_from("<Q", vb, 264)[0]
        self.name = vb[704:960].split(b"\0")[0].decode("utf-8", "replace")
        self.role = struct.unpack_from("<H", vb, 964)[0]
        self.encrypted = not (self.fs_flags & APFS_FS_UNENCRYPTED)
        self.sealed = bool(self.incompat & INCOMPAT_SEALED)
        self.omap = OMap(d, self.omap_oid, xid)
        self.gap = None
        self.root = None
        try:
            self.root = self.omap.lookup(self.root_tree_oid, self.xid)
            d.block(self.root[0], T_BTREE)
        except Encrypted as e:
            self.gap = "encrypted: " + str(e)
        except Bad as e:
            if self.encrypted:
                self.gap = "encrypted volume, its tree does not read: " + str(e)
            else:
                raise
        if deep and self.gap is None:
            for _ in self.fs_tree().leaves():
                pass

    def role_name(self):
        return ROLES.get(self.role, hex(self.role))

    def fs_tree(self, xid=None, root_oid=None):
        x = self.xid if xid is None else xid
        res = lambda oid: self.omap.lookup(oid, x)
        r = res(self.root_tree_oid if root_oid is None else root_oid)
        return BTree(self.d, r[0], resolve=res)

    def snapshots(self):
        """[(xid, name, volume superblock paddr)], from the physical snapshot metadata tree."""
        if not self.snap_meta_oid:
            return []
        t = BTree(self.d, self.snap_meta_oid)
        out = []
        for leaf, _, _ in t.leaves():
            for k, val in t.records(leaf):
                oit = struct.unpack_from("<Q", k)[0]
                if oit >> 60 == 1 and len(val) >= 50:
                    ext_oid, sb_oid, ct, cht, inum, et, flags, nlen = struct.unpack_from("<QQQQQIIH", val)
                    name = val[50:50 + nlen].split(b"\0")[0].decode("utf-8", "replace")
                    out.append((oit & ((1 << 60) - 1), name, sb_oid))
        return out

    def snapshot_tree(self, sb_paddr):
        vb = self.d.block(sb_paddr, T_FS)
        root_oid = struct.unpack_from("<Q", vb, 136)[0]
        return self.fs_tree(xid=hdr(vb)["xid"], root_oid=root_oid)

    def summary(self):
        s = {"fs_oid": self.fs_oid, "name": self.name, "role": self.role_name(), "xid": self.xid,
             "files": self.num_files, "dirs": self.num_dirs, "snapshots": self.num_snapshots,
             "encrypted": self.encrypted, "sealed": self.sealed, "object_map_oids": self.omap.leaves}
        try:
            s["snapshots_listed"] = len(self.snapshots())
        except Bad as e:
            s["snapshots_gap"] = str(e)
        if self.gap:
            s["gap"] = self.gap
        return s


def open_image(path, deep=False):
    f = open(path, "rb")
    out = []
    for name, off, size in find_containers(path):
        rec = {"partition": name, "offset": off, "bytes": size}
        try:
            c = Container(f, name, off, size)
            o = c.open(deep=deep)
            rec.update({k: o[k] for k in ("newest_xid", "opened_xid", "newest_ok", "skipped", "superblocks",
                                         "descriptor_blocks_failing_checksum", "block0_xid")})
            rec["volumes"] = [v.summary() for v in o["volumes"]]
            rec["_vols"] = o["volumes"]
            rec["blocks_read"] = c.d.reads
        except Bad as e:
            rec["gap"] = str(e)
        out.append(rec)
    return f, out


def public(cs):
    return [{k: v for k, v in c.items() if not k.startswith("_")} for c in cs]


def cmd_info(path, deep=False):
    t = time.time()
    f, cs = open_image(path, deep=deep)
    print(json.dumps({"image": path, "containers": public(cs), "seconds": round(time.time() - t, 3)}))


def pick(cs, want):
    """The volume to diff: by role or name; default the data volume, else the first volume."""
    vols = [v for c in cs for v in c.get("_vols", [])]
    if not vols:
        raise Bad("no volume opens")
    for v in vols:
        if want and (v.role_name() == want or v.name == want):
            return v
    if want:
        raise Bad("no volume %s" % want)
    for v in vols:
        if v.role_name() == "data":
            return v
    return vols[0]


def tree_of(spec, want):
    path, _, snap = spec.partition("@")
    f, cs = open_image(path)
    v = pick(cs, want)
    if v.gap:
        raise Bad("volume %s: %s" % (v.name, v.gap))
    if not snap:
        return v, v.fs_tree()
    snaps = v.snapshots()
    for i, (x, name, sb) in enumerate(snaps):
        if name == snap or snap == "#%d" % i:
            return v, v.snapshot_tree(sb)
    raise Bad("no snapshot %s in %s" % (snap, path))


def cmd_leafdiff(a, b, want=None, full=False):
    out = {"a": a, "b": b}
    try:
        leafdiff(out, a, b, want, full)
    except Bad as e:
        out["gap"] = str(e)
    print(json.dumps(out))


def leafdiff(out, a, b, want, full):
    va, ta = tree_of(a, want)
    vb, tb = tree_of(b, want)
    out["volume"] = va.name
    ra0, rb0 = va.d.reads, vb.d.reads
    t = time.time()
    la, lb = set(ta.leaves()), set(tb.leaves())
    only_a, only_b = la - lb, lb - la
    recs_a = {}
    for p in only_a:
        recs_a.update(ta.records(p[0]))
    recs_b = {}
    for p in only_b:
        recs_b.update(tb.records(p[0]))
    fast = set(recs_a.items()) ^ set(recs_b.items())
    out.update({"leaves_a": len(la), "leaves_b": len(lb), "changed_leaves_a": len(only_a),
                "changed_leaves_b": len(only_b), "records_differ": len(fast),
                "leaf_seconds": round(time.time() - t, 3),
                "leaf_blocks_read": va.d.reads - ra0 + (vb.d.reads - rb0 if vb.d is not va.d else 0),
                "same_block_other_version": len({p[0] for p in la} & {p[0] for p in lb}) - len(la & lb)})
    keys = {}
    for k, _ in fast:
        typ = struct.unpack_from("<Q", k)[0] >> 60
        name = REC_TYPES.get(typ, str(typ))
        keys[name] = keys.get(name, 0) + 1
    out["records_differ_by_type"] = keys
    if full:
        ra0, rb0 = va.d.reads, vb.d.reads
        t = time.time()
        fa, fb = {}, {}
        for p in la:
            fa.update(ta.records(p[0]))
        for p in lb:
            fb.update(tb.records(p[0]))
        whole = set(fa.items()) ^ set(fb.items())
        out.update({"full_seconds": round(time.time() - t, 3),
                    "full_blocks_read": va.d.reads - ra0 + (vb.d.reads - rb0 if vb.d is not va.d else 0),
                    "records_a": len(fa), "records_b": len(fb), "same_as_full": whole == fast})


def main(argv):
    if len(argv) >= 2 and argv[0] == "info":
        cmd_info(argv[1], deep="--deep" in argv)
    elif len(argv) >= 2 and argv[0] == "checkpoint":
        t = time.time()
        f, cs = open_image(argv[1], deep="--deep" in argv)
        res = []
        for c in public(cs):
            res.append({k: c.get(k) for k in ("partition", "newest_xid", "opened_xid", "newest_ok", "skipped", "gap",
                                               "blocks_read")})
        print(json.dumps({"image": argv[1], "containers": res, "seconds": round(time.time() - t, 3)}))
    elif len(argv) >= 3 and argv[0] == "leafdiff":
        want = None
        if "--volume" in argv:
            want = argv[argv.index("--volume") + 1]
        cmd_leafdiff(argv[1], argv[2], want, full="--full" in argv)
    else:
        sys.stderr.write(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
