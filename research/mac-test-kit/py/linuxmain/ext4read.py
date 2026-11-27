#!/usr/bin/env python3
"""A trial reader of an ext4 disk image, from outside, never a mount.

It reads the superblock, the group descriptors, the inode tables, extent trees and directories,
and replays the jbd2 journal in memory first, so a copy of a running disk reads as the disk will
after its next boot. It writes nothing. It is a trial for step 5 of option B: Silo's reader is
Zig, strict and fuzzed; this one shows what the reader has to do and checks it against e2fsprogs
and the kernel.

  ext4read.py walk IMAGE [--no-replay]          one line per path: ino gen type size mode path
  ext4read.py diff BEFORE AFTER [--no-replay]   F lines: created, deleted, changed
  ext4read.py journal IMAGE                     what replaying the journal would write
KIT_FS_OFFSET, in bytes, names where the filesystem starts inside a whole disk image.
"""
import struct, sys, hashlib, os

class Refused(Exception):
    """The image breaks a rule the reader depends on. Silo writes a gap naming it."""

def u16(b, o): return struct.unpack_from('<H', b, o)[0]
def u32(b, o): return struct.unpack_from('<I', b, o)[0]
def be32(b, o): return struct.unpack_from('>I', b, o)[0]

JBD2_MAGIC = 0xC03B3998
INCOMPAT_64BIT, INCOMPAT_EXTENTS, INCOMPAT_INLINE = 0x80, 0x40, 0x8000
COMPAT_HAS_JOURNAL = 0x4

class Image:
    def __init__(self, path, replay=True, offset=0):
        self.f = open(path, 'rb')
        self.base = offset
        self.overlay = {}          # block number -> bytes, from the journal
        self.blocks_read = 0
        sb = self.raw(1024, 1024)
        if u16(sb, 56) != 0xEF53:
            raise Refused('not-ext4')
        self.bs = 1024 << u32(sb, 24)
        if self.bs not in (1024, 2048, 4096, 65536):
            raise Refused('block-size')
        self.blocks_count = u32(sb, 4) | (u32(sb, 0x150) << 32 if u32(sb, 0x60) & INCOMPAT_64BIT else 0)
        self.ipg = u32(sb, 40)
        self.inodes_count = u32(sb, 0)
        self.isize = u16(sb, 88) if u32(sb, 76) >= 1 else 128
        self.first_data = u32(sb, 20)
        self.incompat = u32(sb, 0x60)
        self.compat = u32(sb, 0x5C)
        if self.incompat & INCOMPAT_INLINE:
            raise Refused('inline-data')
        self.desc_size = u16(sb, 0xFE) if self.incompat & INCOMPAT_64BIT else 32
        self.journal_ino = u32(sb, 0xE0)
        self.ngroups = (self.blocks_count - self.first_data + u32(sb, 32) - 1) // u32(sb, 32)
        gdt = self.first_data + 1
        self.itable = []
        for g in range(self.ngroups):
            off = g * self.desc_size
            blk = gdt + off // self.bs
            d = self.block(blk)[off % self.bs: off % self.bs + self.desc_size]
            lo = u32(d, 8)
            hi = u32(d, 0x28) if self.desc_size >= 64 else 0
            self.itable.append(lo | (hi << 32))
        self.replayed = {}
        if replay and self.compat & COMPAT_HAS_JOURNAL and self.journal_ino:
            self.replayed = self.replay_journal()
            self.overlay.update(self.replayed)

    def raw(self, off, n):
        self.f.seek(self.base + off)
        b = self.f.read(n)
        if len(b) != n:
            raise Refused('short-read')
        return b

    def block(self, n):
        if n in self.overlay:
            return self.overlay[n]
        if n >= self.blocks_count:
            raise Refused('block-past-end')
        self.blocks_read += 1
        return self.raw(n * self.bs, self.bs)

    def inode(self, ino):
        if not 1 <= ino <= self.inodes_count:
            raise Refused('inode-number')
        g, i = divmod(ino - 1, self.ipg)
        off = i * self.isize
        b = self.block(self.itable[g] + off // self.bs)
        return b[off % self.bs: off % self.bs + self.isize]

    def extents(self, ino_bytes, depth_budget=5):
        """(logical block, physical block, length) of a file, from its extent tree."""
        out = []
        def node(b, budget):
            if u16(b, 0) != 0xF30A:
                raise Refused('extent-magic')
            entries, depth = u16(b, 2), u16(b, 6)
            if budget == 0:
                raise Refused('extent-depth')
            for k in range(entries):
                e = 12 + 12 * k
                if depth == 0:
                    lblk, ln, hi, lo = u32(b, e), u16(b, e + 4), u16(b, e + 6), u32(b, e + 8)
                    if ln > 32768:
                        ln -= 32768          # an extent allocated, not written: reads as zeroes
                    out.append((lblk, lo | (hi << 32), ln))
                else:
                    lo, hi = u32(b, e + 4), u16(b, e + 8)
                    node(self.block(lo | (hi << 32)), budget - 1)
        flags = u32(ino_bytes, 32)
        if not flags & 0x80000:
            raise Refused('not-extents')     # block maps of ext2/3 are not read by this trial
        node(ino_bytes[40:100], depth_budget)
        return out

    def file_blocks(self, ino_bytes):
        for lblk, pblk, ln in self.extents(ino_bytes):
            for k in range(ln):
                yield pblk + k

    def dir_entries(self, ino_bytes):
        for pblk in self.file_blocks(ino_bytes):
            b = self.block(pblk)
            o = 0
            while o < self.bs:
                ino, rec_len, name_len, ftype = u32(b, o), u16(b, o + 4), b[o + 6], b[o + 7]
                if rec_len < 8 or o + rec_len > self.bs or name_len > rec_len - 8:
                    raise Refused('dirent')
                if ino and ftype != 0xDE:
                    yield ino, ftype, b[o + 8: o + 8 + name_len]
                o += rec_len

    # --- the journal -------------------------------------------------------------------------

    def replay_journal(self):
        """jbd2's three passes, in memory: find the last committed transaction, collect revokes,
        then the blocks to write. Returns {fs block: bytes}. Checksums are not verified here;
        Silo's reader verifies them and stops at the first that fails, as the kernel does."""
        ji = self.inode(self.journal_ino)
        jmap = list(self.file_blocks(ji))     # journal block i -> fs block
        def jblock(i):
            return self.block(jmap[i])
        sbj = jblock(0)
        if be32(sbj, 0) != JBD2_MAGIC or be32(sbj, 4) not in (3, 4):
            raise Refused('journal-superblock')
        maxlen, first, seq, start = be32(sbj, 16), be32(sbj, 20), be32(sbj, 24), be32(sbj, 28)
        if start == 0:
            return {}                          # clean: nothing to replay
        incompat = be32(sbj, 0x28) if be32(sbj, 4) == 4 else 0
        j64, csum3, csum2 = incompat & 0x2, incompat & 0x10, incompat & 0x8
        if csum3:
            tag_size = 16
        else:
            tag_size = 8 + (4 if j64 else 0)
        tail = 4 if (csum2 or csum3) else 0

        def nxt(i):
            i += 1
            return first if i >= maxlen else i

        def walk():
            """Yield (seq, kind, data) for each transaction, in order, stopping at the first
            block that is not the next expected one."""
            i, s = start, seq
            budget = maxlen
            while budget > 0:
                b = jblock(i)
                if be32(b, 0) != JBD2_MAGIC or be32(b, 8) != s:
                    return
                kind = be32(b, 4)
                if kind == 1:                  # descriptor: tags, then the data blocks
                    tags, o = [], 12
                    while o + tag_size <= self.bs - tail:
                        if csum3:
                            blk, flags = be32(b, o), be32(b, o + 4)
                            blk |= be32(b, o + 8) << 32 if j64 else 0
                        else:
                            blk, flags = be32(b, o), struct.unpack_from('>H', b, o + 6)[0]
                            if j64:
                                blk |= be32(b, o + 8) << 32
                        o += tag_size
                        if not flags & 2:      # not SAME_UUID: a 16-byte UUID follows
                            o += 16
                        i = nxt(i); budget -= 1
                        data = bytearray(jblock(i))
                        if flags & 1:          # ESCAPE: the block began with the magic
                            struct.pack_into('>I', data, 0, JBD2_MAGIC)
                        tags.append((blk, bytes(data)))
                        if flags & 8:          # LAST_TAG
                            break
                    yield s, 'data', tags
                elif kind == 2:                # commit
                    yield s, 'commit', None
                    s += 1
                elif kind == 5:                # revoke
                    count = be32(b, 12)
                    w = 8 if j64 else 4
                    if count > self.bs:
                        raise Refused('revoke-count')
                    blks = [int.from_bytes(b[o:o + w], 'big') for o in range(16, count, w)]
                    yield s, 'revoke', blks
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
                for blk, data in tags:
                    if revoked.get(blk, -1) >= s:
                        continue
                    if blk >= self.blocks_count:
                        raise Refused('journal-block-past-end')
                    out[blk] = data
        return out

    # --- the tree ----------------------------------------------------------------------------

    def walk(self, limit=10_000_000):
        """Every path, from the root, breadth first, with a budget and no cycles."""
        seen_dirs = set()
        queue = [(2, b'')]
        out = {}
        while queue:
            ino, path = queue.pop(0)
            if ino in seen_dirs:
                raise Refused('directory-cycle')
            seen_dirs.add(ino)
            ib = self.inode(ino)
            for child, ftype, name in self.dir_entries(ib):
                if name in (b'.', b'..'):
                    continue
                if b'/' in name or not name:
                    raise Refused('name')
                p = path + b'/' + name
                cb = self.inode(child)
                mode = u16(cb, 0)
                size = u32(cb, 4) | (u32(cb, 108) << 32)
                gen = u32(cb, 100)
                kind = {0x4000: 'dir', 0x8000: 'file', 0xA000: 'link'}.get(mode & 0xF000, 'other')
                if kind == 'file' and u32(cb, 32) & 0x80000:
                    ext = hashlib.sha256(repr(self.extents(cb)).encode()).hexdigest()[:12]
                else:
                    ext = '-'
                out[p] = (child, gen, kind, size, mode & 0o7777, u32(cb, 16), ext)
                if len(out) > limit:
                    raise Refused('budget')
                if kind == 'dir':
                    queue.append((child, p))
        return out


def show(p):
    return p.decode('utf-8', 'backslashreplace')

def main():
    cmd, args = sys.argv[1], [a for a in sys.argv[2:] if not a.startswith('--')]
    replay = '--no-replay' not in sys.argv
    off = int(os.environ.get('KIT_FS_OFFSET', '0'))   # where the filesystem starts in the image
    Img = lambda path, rep: Image(path, rep, off)
    if cmd == 'walk':
        img = Img(args[0], replay)
        for p, (ino, gen, kind, size, mode, mtime, ext) in sorted(img.walk().items()):
            print(f'{ino}\t{gen}\t{kind}\t{size}\t{mode:o}\t{show(p)}')
        print(f'# blocks read {img.blocks_read}, replayed {len(img.replayed)}', file=sys.stderr)
    elif cmd == 'journal':
        img = Img(args[0], True)
        print(f'{len(img.replayed)} blocks to replay: {sorted(img.replayed)[:20]}')
    elif cmd == 'diff':
        a, b = Img(args[0], replay), Img(args[1], replay)
        wa, wb = a.walk(), b.walk()
        n = 0
        for p in sorted(set(wa) | set(wb)):
            x, y = wa.get(p), wb.get(p)
            if x and not y:
                print(f'F\tdeleted\t{x[2]}\t{x[0]}\t{x[3]}\t0\t-\t{show(p)}')
            elif y and not x:
                print(f'F\tcreated\t{y[2]}\t{y[0]}\t0\t{y[3]}\t-\t{show(p)}')
            elif (x[0], x[1]) != (y[0], y[1]):
                # same path, another file: ext4 reuses inode numbers, so the generation tells
                print(f'F\tdeleted\t{x[2]}\t{x[0]}\t{x[3]}\t0\t-\t{show(p)}')
                print(f'F\tcreated\t{y[2]}\t{y[0]}\t0\t{y[3]}\t-\t{show(p)}')
            else:
                what = [w for w, i in (('content', 3), ('mode', 4), ('content', 6)) if x[i] != y[i]]
                what += ['mtime'] if x[5] != y[5] else []
                what = sorted(set(what))
                if what:
                    print(f'F\tchanged\t{y[2]}\t{y[0]}\t{x[3]}\t{y[3]}\t{",".join(what)}\t{show(p)}')
                else:
                    continue
            n += 1
        print(f'end\tfiles\t{n}\tblocks-read\t{a.blocks_read + b.blocks_read}')

if __name__ == '__main__':
    try:
        main()
    except Refused as e:
        print(f'gap\t{e}')
        sys.exit(2)
