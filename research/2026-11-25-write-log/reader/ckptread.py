#!/usr/bin/env python3
"""The ext4 reader of #381's proto/ext4read.py, made into what step 5 reads checkpoints with.

What it adds to the trial:

- **Sources.** It reads a disk through a source: a raw file, or a `delta` checkpoint read in place,
  the first full copy with the deltas up to it laid over it (#365's format: the copied blocks at
  their offsets, then the bitmap of them at offset `size`). Nothing is materialized.
- **The partition.** It finds main's root filesystem in a GPT disk by its label
  (`cloudimg-rootfs`, as in Ubuntu's image) and reads inside it.
- **The journal's checksums.** jbd2 v2 and v3 checksums of descriptor, revoke and commit blocks and
  of each block a transaction writes are checked. A bad descriptor, revoke or commit block ends the
  log there: that transaction and the ones after it are not replayed. A block whose own checksum is
  bad is not replayed and is named in a gap. The trial checked none.
- **The comparison from the write log.** `compare` takes the index of the earlier checkpoint and the
  set of blocks written between the two (the `writes` bitmap of each checkpoint in between) and
  reads only: the inode-table blocks in the set, the directories that own a block in the set or
  whose inode changed, and what is new under them. It gives the same lines as a full walk of both,
  and the index of the later checkpoint for the next comparison.

It writes nothing and mounts nothing. Silo's reader is Zig; this one is a trial that the tests
check against the kernel and against full walks.

  ckptread.py walk DISK                       one line per path, from the root filesystem
  ckptread.py walk-delta BASE SIZE DELTA...   the same, of a checkpoint read through its deltas
  ckptread.py journal DISK                    what replaying the journal would write, and gaps
"""
import struct, sys, hashlib, os, bisect

class Refused(Exception):
    """The image breaks a rule the reader depends on. Silo writes a gap naming it."""

def u16(b, o): return struct.unpack_from('<H', b, o)[0]
def u32(b, o): return struct.unpack_from('<I', b, o)[0]
def be32(b, o): return struct.unpack_from('>I', b, o)[0]

BS = 4096

# --- crc32c, as jbd2 uses it: the raw update, with no inversion before or after ----------------

def _table():
    t = []
    for i in range(256):
        c = i
        for _ in range(8):
            c = (c >> 1) ^ 0x82F63B78 if c & 1 else c >> 1
        t.append(c)
    return t
_T = _table()

def crc32c_update(crc, data):
    t = _T
    for byte in data:
        crc = t[(crc ^ byte) & 0xFF] ^ (crc >> 8)
    return crc

def crc32c(data):
    """The usual crc32c, with ~0 in and out: crc32c(b'123456789') == 0xE3069283."""
    return crc32c_update(0xFFFFFFFF, data) ^ 0xFFFFFFFF

# --- sources -------------------------------------------------------------------------------------

class FileSource:
    def __init__(self, path, offset=0, size=None):
        self.f = open(path, 'rb')
        self.offset = offset
        self.size = size if size is not None else os.fstat(self.f.fileno()).st_size - offset
        self.bytes_read = 0

    def read(self, off, n):
        if off < 0 or off + n > self.size:
            raise Refused('read-past-end')
        self.f.seek(self.offset + off)
        b = self.f.read(n)
        if len(b) != n:
            raise Refused('short-read')
        self.bytes_read += n
        return b

    def slice(self, offset, size):
        s = FileSource.__new__(FileSource)
        s.f, s.offset, s.size, s.bytes_read = self.f, self.offset + offset, size, 0
        return s


def read_map(path, size):
    """The bitmap after a delta's blocks: which 4 KiB blocks it holds."""
    words = (size // BS + 63) // 64
    with open(path, 'rb') as f:
        f.seek(size)
        raw = f.read(words * 8)
    if len(raw) != words * 8:
        raise Refused('delta-map')
    return int.from_bytes(raw, 'little')


def map_blocks(m):
    out = []
    while m:
        low = m & -m
        out.append(low.bit_length() - 1)
        m ^= low
    return out


class DeltaChain:
    """Checkpoint k of a disk under `delta`: the base with deltas 1..k laid over it, read in
    place. A block comes from the last delta that holds it, or from the base."""
    def __init__(self, base, deltas, size):
        if size % BS:
            raise Refused('delta-size')
        self.size = size
        self.base = open(base, 'rb')
        self.deltas = [(open(d, 'rb'), read_map(d, size)) for d in deltas]
        self.bytes_read = 0
        self.offset = 0

    def _block(self, b):
        for f, m in reversed(self.deltas):
            if m >> b & 1:
                f.seek(b * BS)
                return f.read(BS)
        self.base.seek(b * BS)
        return self.base.read(BS)

    def read(self, off, n):
        if off < 0 or off + n > self.size:
            raise Refused('read-past-end')
        out = bytearray()
        b = off // BS
        while len(out) < n + (off % BS):
            blk = self._block(b)
            if len(blk) != BS:
                raise Refused('short-read')
            out += blk
            b += 1
        self.bytes_read += n
        return bytes(out[off % BS: off % BS + n])

    def slice(self, offset, size):
        return Slice(self, offset, size)


class Slice:
    def __init__(self, src, offset, size):
        self.src, self.offset, self.size = src, offset, size

    def read(self, off, n):
        if off < 0 or off + n > self.size:
            raise Refused('read-past-end')
        return self.src.read(self.offset + off, n)

    @property
    def bytes_read(self):
        return self.src.bytes_read


def gpt_partition(src, label='cloudimg-rootfs'):
    """(offset, size) in bytes of the GPT partition with this label. 512-byte sectors."""
    hdr = src.read(512, 92)
    if hdr[:8] != b'EFI PART':
        raise Refused('no-gpt')
    first_entry, count, esize = struct.unpack_from('<QII', hdr, 72)
    if esize < 128 or count > 1024:
        raise Refused('gpt-entries')
    found = []
    for i in range(count):
        e = src.read(first_entry * 512 + i * esize, esize)
        if e[:16] == bytes(16):
            continue
        start, end = struct.unpack_from('<QQ', e, 32)
        name = e[56:128].decode('utf-16-le', 'replace').rstrip('\x00')
        if name == label:
            found.append((start * 512, (end - start + 1) * 512))
    if len(found) != 1:
        raise Refused('partition-label')
    return found[0]

# --- the filesystem ------------------------------------------------------------------------------

JBD2_MAGIC = 0xC03B3998
INCOMPAT_64BIT, INCOMPAT_INLINE = 0x80, 0x8000
COMPAT_HAS_JOURNAL = 0x4
JBD2_FEATURE_INCOMPAT_64BIT, JBD2_CSUM_V2, JBD2_CSUM_V3 = 0x2, 0x8, 0x10


class Image:
    def __init__(self, src, replay=True):
        self.src = src
        self.overlay = {}
        self.touched = set()
        self.gaps = []
        self.checksums_verified = 0
        sb = self.src.read(1024, 1024)
        if u16(sb, 56) != 0xEF53:
            raise Refused('not-ext4')
        self.bs = 1024 << u32(sb, 24)
        if self.bs not in (1024, 2048, 4096, 65536):
            raise Refused('block-size')
        self.incompat = u32(sb, 0x60)
        self.compat = u32(sb, 0x5C)
        self.blocks_count = u32(sb, 4) | (u32(sb, 0x150) << 32 if self.incompat & INCOMPAT_64BIT else 0)
        self.ipg = u32(sb, 40)
        self.inodes_count = u32(sb, 0)
        self.isize = u16(sb, 88) if u32(sb, 76) >= 1 else 128
        self.first_data = u32(sb, 20)
        if self.incompat & INCOMPAT_INLINE:
            raise Refused('inline-data')
        self.desc_size = u16(sb, 0xFE) if self.incompat & INCOMPAT_64BIT else 32
        self.journal_ino = u32(sb, 0xE0)
        bpg = u32(sb, 32)
        if not self.ipg or not bpg:
            raise Refused('superblock')
        self.ngroups = (self.blocks_count - self.first_data + bpg - 1) // bpg
        gdt = self.first_data + 1
        self.itable = []
        for g in range(self.ngroups):
            off = g * self.desc_size
            d = self.block(gdt + off // self.bs)[off % self.bs: off % self.bs + self.desc_size]
            self.itable.append(u32(d, 8) | ((u32(d, 0x28) << 32) if self.desc_size >= 64 else 0))
        self.itable_blocks = self.ipg * self.isize // self.bs
        self.replayed = {}
        if replay and self.compat & COMPAT_HAS_JOURNAL and self.journal_ino:
            self.replayed = self.replay_journal()
            self.overlay.update(self.replayed)

    @property
    def blocks_read(self):
        """Distinct blocks read from the disk so far. A block the journal replay laid over the
        disk is in memory and is not counted again."""
        return len(self.touched)

    def block(self, n):
        if n in self.overlay:
            return self.overlay[n]
        self.touched.add(n)
        if n >= self.blocks_count:
            raise Refused('block-past-end')
        return self.src.read(n * self.bs, self.bs)

    def inode(self, ino):
        if not 1 <= ino <= self.inodes_count:
            raise Refused('inode-number')
        g, i = divmod(ino - 1, self.ipg)
        off = i * self.isize
        b = self.block(self.itable[g] + off // self.bs)
        return b[off % self.bs: off % self.bs + self.isize]

    def itable_inodes(self, blk):
        """The inode numbers an inode-table block holds, or an empty range."""
        for g, start in enumerate(self.itable):
            if start <= blk < start + self.itable_blocks:
                per = self.bs // self.isize
                first = g * self.ipg + (blk - start) * per + 1
                return range(first, first + per)
        return range(0)

    def extents(self, ino_bytes, nodes=None, depth_budget=5):
        """(logical block, physical block, length) of a file; `nodes` gets the tree's blocks."""
        out = []
        def node(b, budget):
            if u16(b, 0) != 0xF30A:
                raise Refused('extent-magic')
            entries, depth = u16(b, 2), u16(b, 6)
            if budget == 0:
                raise Refused('extent-depth')
            if 12 + 12 * entries > len(b):
                raise Refused('extent-entries')
            for k in range(entries):
                e = 12 + 12 * k
                if depth == 0:
                    lblk, ln, hi, lo = u32(b, e), u16(b, e + 4), u16(b, e + 6), u32(b, e + 8)
                    if ln > 32768:
                        ln -= 32768
                    out.append((lblk, lo | (hi << 32), ln))
                else:
                    child = u32(b, e + 4) | (u16(b, e + 8) << 32)
                    if nodes is not None:
                        nodes.append(child)
                    node(self.block(child), budget - 1)
        if not u32(ino_bytes, 32) & 0x80000:
            raise Refused('not-extents')
        node(ino_bytes[40:100], depth_budget)
        return out

    def owned_blocks(self, ino_bytes):
        """Every block of a file: its extents' blocks and its extent tree's own blocks."""
        if not u32(ino_bytes, 32) & 0x80000:
            return []
        nodes = []
        ext = self.extents(ino_bytes, nodes)
        out = nodes
        for _, p, ln in ext:
            out.extend(range(p, p + ln))
        return out

    def dir_entries(self, ino_bytes):
        for _, pblk, ln in self.extents(ino_bytes):
            for k in range(ln):
                b = self.block(pblk + k)
                o = 0
                while o < self.bs:
                    ino, rec_len, name_len, ftype = u32(b, o), u16(b, o + 4), b[o + 6], b[o + 7]
                    if rec_len < 8 or o + rec_len > self.bs or name_len > rec_len - 8:
                        raise Refused('dirent')
                    if ino and ftype != 0xDE:
                        yield ino, ftype, b[o + 8: o + 8 + name_len]
                    o += rec_len

    # --- the journal ---------------------------------------------------------------------------

    def replay_journal(self):
        """jbd2's passes, in memory, with its checksums checked. Returns {fs block: bytes}."""
        ji = self.inode(self.journal_ino)
        jmap = []
        for lblk, p, ln in sorted(self.extents(ji)):
            jmap.extend(range(p, p + ln))
        def jblock(i):
            if i >= len(jmap):
                raise Refused('journal-block')
            return self.block(jmap[i])
        self.jmap = jmap
        self.journal_blocks = []   # (kind, journal index, sequence) of each block the log walk met
        sbj = jblock(0)
        if be32(sbj, 0) != JBD2_MAGIC or be32(sbj, 4) not in (3, 4):
            raise Refused('journal-superblock')
        maxlen, first, seq, start = be32(sbj, 16), be32(sbj, 20), be32(sbj, 24), be32(sbj, 28)
        if start == 0:
            return {}
        if maxlen > len(jmap) or not 0 < first < maxlen or not first <= start < maxlen:
            raise Refused('journal-superblock')
        incompat = be32(sbj, 0x28) if be32(sbj, 4) == 4 else 0
        j64 = incompat & JBD2_FEATURE_INCOMPAT_64BIT
        csum3, csum2 = incompat & JBD2_CSUM_V3, incompat & JBD2_CSUM_V2
        csum = csum2 or csum3
        seed = crc32c_update(0xFFFFFFFF, sbj[0x30:0x40]) if csum else None
        tag_size = 16 if csum3 else 8 + (4 if j64 else 0)
        tail = 4 if csum else 0
        bs = self.bs

        def tail_ok(b):
            if not csum:
                return True
            self.checksums_verified += 1
            z = bytearray(b)
            z[bs - 4:] = bytes(4)
            return crc32c_update(seed, z) == be32(b, bs - 4)

        def commit_ok(b):
            if not csum:
                return True
            self.checksums_verified += 1
            z = bytearray(b)
            z[16:20] = bytes(4)
            return crc32c_update(seed, z) == be32(b, 16)

        def data_ok(s, data, want):
            if not csum:
                return True
            self.checksums_verified += 1
            c = crc32c_update(crc32c_update(seed, struct.pack('>I', s)), data)
            return (c if csum3 else c & 0xFFFF) == want

        def nxt(i):
            i += 1
            return first if i >= maxlen else i

        def walk():
            i, s = start, seq
            budget = maxlen
            while budget > 0:
                b = jblock(i)
                if be32(b, 0) != JBD2_MAGIC or be32(b, 8) != s & 0xFFFFFFFF:
                    return
                kind = be32(b, 4)
                self.journal_blocks.append((kind, i, s))
                if kind == 1:
                    if not tail_ok(b):
                        self.gaps.append(f'journal-descriptor-checksum {s}')
                        return
                    tags, o = [], 12
                    while o + tag_size <= bs - tail:
                        if csum3:
                            blk, flags = be32(b, o), be32(b, o + 4)
                            blk |= be32(b, o + 8) << 32 if j64 else 0
                            want = be32(b, o + 12)
                        else:
                            blk, want, flags = be32(b, o), struct.unpack_from('>H', b, o + 4)[0], struct.unpack_from('>H', b, o + 6)[0]
                            if j64:
                                blk |= be32(b, o + 8) << 32
                        o += tag_size
                        if not flags & 2:
                            o += 16
                        i = nxt(i); budget -= 1
                        raw = jblock(i)
                        self.journal_blocks.append((0, i, s))
                        ok = data_ok(s, raw, want)
                        data = bytearray(raw)
                        if flags & 1:
                            struct.pack_into('>I', data, 0, JBD2_MAGIC)
                        tags.append((blk, bytes(data), ok))
                        if flags & 8:
                            break
                    yield s, 'data', tags
                elif kind == 2:
                    if not commit_ok(b):
                        self.gaps.append(f'journal-commit-checksum {s}')
                        return
                    yield s, 'commit', None
                    s += 1
                elif kind == 5:
                    if not tail_ok(b):
                        self.gaps.append(f'journal-revoke-checksum {s}')
                        return
                    count = be32(b, 12)
                    w = 8 if j64 else 4
                    if count > bs - tail or count < 16:
                        raise Refused('revoke-count')
                    yield s, 'revoke', [int.from_bytes(b[o:o + w], 'big') for o in range(16, count, w)]
                else:
                    return
                i = nxt(i); budget -= 1

        events = list(walk())
        committed = {s for s, kind, _ in events if kind == 'commit'}
        revoked = {}
        for s, kind, blks in events:
            if kind == 'revoke' and s in committed:
                for blk in blks:
                    revoked[blk] = max(revoked.get(blk, -1), s)
        out = {}
        for s, kind, tags in events:
            if kind == 'data' and s in committed:
                for blk, data, ok in tags:
                    if revoked.get(blk, -1) >= s:
                        continue
                    if blk >= self.blocks_count:
                        raise Refused('journal-block-past-end')
                    if not ok:
                        self.gaps.append(f'journal-block-checksum {s} {blk}')
                        continue
                    out[blk] = data
        return out

    # --- the tree ------------------------------------------------------------------------------

    def record(self, ino, ib=None):
        """What a path's line carries about its inode: gen, kind, size, mode, mtime, extents."""
        cb = ib if ib is not None else self.inode(ino)
        mode = u16(cb, 0)
        kind = {0x4000: 'dir', 0x8000: 'file', 0xA000: 'link'}.get(mode & 0xF000, 'other')
        ext = '-'
        if kind == 'file' and u32(cb, 32) & 0x80000:
            ext = hashlib.sha256(repr(self.extents(cb)).encode()).hexdigest()[:12]
        return (u32(cb, 100), kind, u32(cb, 4) | (u32(cb, 108) << 32), mode & 0o7777, u32(cb, 16), ext)

    def children(self, ino):
        out = {}
        for child, ftype, name in self.dir_entries(self.inode(ino)):
            if name in (b'.', b'..'):
                continue
            if b'/' in name or not name:
                raise Refused('name')
            out[name] = child
        return out

    def walk(self):
        """Every path, as #381's trial walks it: {path: (ino, gen, kind, size, mode, mtime, ext)}."""
        return Index.build(self).paths()

# --- the index of a checkpoint, and the comparison from the write log ----------------------------

LIMIT = 10_000_000


class Index:
    """What the comparer keeps of a checkpoint (`ckpt/<id>/names` in 5-the-record.md): each
    directory's entries, each inode's record, and which inode owns each block."""

    def __init__(self):
        self.children = {}   # dir ino -> {name: ino}
        self.rec = {}        # ino -> record
        self.blocks = {}     # ino -> [blocks it owns]
        self.owner = {}      # block -> ino

    @staticmethod
    def build(img, limit=10_000_000):
        ix = Index()
        ix._read_tree(img, 2, set(), limit)
        return ix

    def _own(self, ino, blocks):
        for b in self.blocks.pop(ino, ()):
            if self.owner.get(b) == ino:
                del self.owner[b]
        self.blocks[ino] = blocks
        for b in blocks:
            self.owner[b] = ino

    def _read_inode(self, img, ino):
        ib = img.inode(ino)
        self.rec[ino] = img.record(ino, ib)
        self._own(ino, img.owned_blocks(ib))
        return self.rec[ino]

    def _read_tree(self, img, top, seen, limit):
        """Reads directory `top` and everything under it from `img`."""
        queue = [top]
        if top not in self.rec:
            self._read_inode(img, top)
        while queue:
            d = queue.pop(0)
            if d in seen:
                raise Refused('directory-cycle')
            seen.add(d)
            kids = img.children(d)
            self.children[d] = kids
            for child in kids.values():
                r = self._read_inode(img, child)
                if len(self.rec) > limit:
                    raise Refused('budget')
                if r[1] == 'dir':
                    queue.append(child)

    def paths(self):
        out = {}
        queue = [(2, b'')]
        seen = set()
        while queue:
            ino, path = queue.pop(0)
            if ino in seen:
                raise Refused('directory-cycle')
            seen.add(ino)
            for name, child in self.children.get(ino, {}).items():
                p = path + b'/' + name
                r = self.rec[child]
                out[p] = (child,) + r
                if r[1] == 'dir':
                    queue.append((child, p))
        return out

    def prune(self):
        """Drops what no path reaches any more, so a freed inode is never read again."""
        live = {2}
        for rec in self.paths().values():
            live.add(rec[0])
        for ino in [i for i in self.rec if i not in live]:
            del self.rec[ino]
            self.children.pop(ino, None)
            self._own(ino, [])
            del self.blocks[ino]

    def copy(self):
        ix = Index()
        ix.children = dict(self.children)
        ix.rec = dict(self.rec)
        ix.blocks = dict(self.blocks)
        ix.owner = dict(self.owner)
        return ix


def compare(ix_a, a, b, written):
    """The index of checkpoint b, and the set of inodes whose data blocks were written, from the
    index of a, both images, and the blocks written between them (any superset of the blocks that
    changed). Reads from b only what the set and the journal point at."""
    s = set(written)
    for blk in set(a.replayed) | set(b.replayed):
        # a block the journal lays over the disk is changed only if the two views of it differ
        if blk not in s and a.block(blk) != b.block(blk):
            s.add(blk)
    changed = set()
    for blk in s:
        for ino in a.itable_inodes(blk):
            if ino <= a.inodes_count and a.inode(ino) != b.inode(ino):
                changed.add(ino)
    owners = {ix_a.owner[blk] for blk in s if blk in ix_a.owner}
    ix = ix_a.copy()
    unreadable = {}
    for ino in changed | owners:
        if ino in ix.rec:
            try:
                ix._read_inode(b, ino)
            except Refused as e:
                unreadable[ino] = e   # freed, most likely: fine if no path reaches it any more
    dirs = {i for i in changed | owners if i in ix_a.children or ix.rec.get(i, (0, ''))[1] == 'dir'}
    for d in sorted(dirs):
        r = ix.rec.get(d)
        if r is None:
            continue
        if r[1] != 'dir':
            ix.children.pop(d, None)
            continue
        if d not in ix_a.children or ix_a.rec[d][0] != r[0]:
            # a directory new at this number: read it whole
            ix.children.pop(d, None)
            ix._read_tree(b, d, set(), LIMIT)
            continue
        new = b.children(d)
        ix.children[d] = new
        for child in new.values():
            if child in ix_a.rec and child not in changed:
                continue  # moved or linked here, unchanged: its record and subtree hold
            rr = ix._read_inode(b, child)
            if rr[1] == 'dir' and (child not in ix_a.children or ix_a.rec[child][0] != rr[0]):
                ix._read_tree(b, child, set(), LIMIT)
    ix.prune()
    for ino, e in unreadable.items():
        if ino in ix.rec:
            raise e
    content = {ix_a.owner[blk] for blk in written if blk in ix_a.owner}
    return ix, content


def diff_lines(pa, pb):
    """The F lines of #381's trial, from two {path: record} maps."""
    out = []
    for p in sorted(set(pa) | set(pb)):
        x, y = pa.get(p), pb.get(p)
        if x and not y:
            out.append(('deleted', x[2], x[0], p))
        elif y and not x:
            out.append(('created', y[2], y[0], p))
        elif (x[0], x[1]) != (y[0], y[1]):
            out.append(('deleted', x[2], x[0], p))
            out.append(('created', y[2], y[0], p))
        else:
            what = {w for w, i in (('content', 3), ('mode', 4), ('mtime', 5), ('content', 6)) if x[i] != y[i]}
            if what:
                out.append(('changed', y[2], y[0], p, ','.join(sorted(what))))
    return out


def root_fs(src):
    """The root filesystem of a whole disk: its GPT partition, or the disk itself if it starts
    with ext4."""
    try:
        off, size = gpt_partition(src)
        return src.slice(off, size)
    except Refused as e:
        if str(e) != 'no-gpt':
            raise
        return src

# --- command line ----------------------------------------------------------------------------------

def show(p):
    return p.decode('utf-8', 'backslashreplace')

def print_walk(img):
    for p, (ino, gen, kind, size, mode, mtime, ext) in sorted(img.walk().items()):
        print(f'{ino}\t{gen}\t{kind}\t{size}\t{mode:o}\t{show(p)}')
    for g in img.gaps:
        print(f'gap\t{g}')

def main():
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == 'walk':
        print_walk(Image(root_fs(FileSource(args[0]))))
    elif cmd == 'walk-delta':
        print_walk(Image(root_fs(DeltaChain(args[0], args[2:], int(args[1])))))
    elif cmd == 'journal':
        img = Image(root_fs(FileSource(args[0])))
        print(f'{len(img.replayed)} blocks to replay, {img.checksums_verified} checksums checked')
        for g in img.gaps:
            print(f'gap\t{g}')

if __name__ == '__main__':
    try:
        main()
    except Refused as e:
        print(f'gap\t{e}')
        sys.exit(2)
