#!/usr/bin/env python3
"""Checks ckptread.py against the kernel and against full walks.

Needs root, loop devices and e2fsprogs; needs no KVM. Run: sudo python3 -I test_ckptread.py
(the folder of this file is put on the path by hand, since -I leaves it off).

The fixture is main's disk in small: a GPT disk of 1 GiB whose partition `cloudimg-rootfs` holds
ext4 with Ubuntu 26.04's features (metadata_csum, 64bit, orphan_file; jbd2 checksums v3). It is
mounted from the disk file, and a night's work runs on it in phases. After each phase the disk file
is copied while still mounted, with no sync, as a checkpoint of a running computer is; then each
copy is also turned into a `delta` (#365's format) against the one before.
"""
import os, sys, random, shutil, struct, subprocess, tempfile, unittest, uuid, zlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ckptread as C

DISK = 1 << 30
PART_START = 2048            # in 512-byte sectors: 1 MiB
PART_END = DISK // 512 - 34  # the last sector of the partition, before the backup GPT
PART_OFF = PART_START * 512
PART_SIZE = (PART_END - PART_START + 1) * 512 // C.BS * C.BS
W = None                     # the fixture's folder


def sh(cmd, **kw):
    return subprocess.run(cmd, shell=True, check=True, capture_output=True, text=True, **kw).stdout


def make_gpt(path, label):
    """A disk with one GPT partition named `label`, as Ubuntu's image names its root."""
    with open(path, 'r+b') as f:
        mbr = bytearray(512)
        mbr[446 + 4] = 0xEE
        struct.pack_into('<II', mbr, 446 + 8, 1, min(DISK // 512 - 1, 0xFFFFFFFF))
        mbr[510:512] = b'\x55\xAA'
        f.write(mbr)
        entry = bytearray(128)
        entry[0:16] = uuid.UUID('b921b045-1df0-41c3-af44-4c6f280d3fae').bytes_le  # Linux arm64 root
        entry[16:32] = uuid.uuid4().bytes_le
        struct.pack_into('<QQ', entry, 32, PART_START, PART_END)
        name = label.encode('utf-16-le')
        entry[56:56 + len(name)] = name
        entries = bytes(entry) + bytes(127 * 128)
        hdr = bytearray(92)
        hdr[0:8] = b'EFI PART'
        struct.pack_into('<IIIIQQQQ', hdr, 8, 0x10000, 92, 0, 0, 1, DISK // 512 - 1, 34, DISK // 512 - 34)
        hdr[56:72] = uuid.uuid4().bytes_le
        struct.pack_into('<QIII', hdr, 72, 2, 128, 128, zlib.crc32(entries))
        struct.pack_into('<I', hdr, 16, zlib.crc32(hdr))
        f.seek(512)
        f.write(hdr)
        f.seek(1024)
        f.write(entries)


def kernel_walk(copy):
    """The copy as the kernel reads it after e2fsck replayed its journal: {path: (ino, kind, size,
    mode)}."""
    ref = os.path.join(W, 'ref.img')
    shutil.copyfile(copy, ref)
    loop = sh(f'losetup -f --show -o {PART_OFF} --sizelimit {PART_SIZE} {ref}').strip()
    try:
        subprocess.run(['e2fsck', '-fy', loop], capture_output=True)
        mnt = os.path.join(W, 'refmnt')
        os.makedirs(mnt, exist_ok=True)
        sh(f'mount -o ro {loop} {mnt}')
        try:
            out = sh(f"cd {mnt} && find . -mindepth 1 -printf '%i\\t%y\\t%s\\t%m\\t/%P\\n'")
        finally:
            sh(f'umount {mnt}')
    finally:
        sh(f'losetup -d {loop}')
        os.unlink(ref)
    res = {}
    for line in out.splitlines():
        ino, y, size, mode, path = line.split('\t', 4)
        if path == '/lost+found' or path.startswith('/lost+found/'):
            continue
        kind = {'f': 'file', 'd': 'dir', 'l': 'link'}.get(y, 'other')
        res[path.encode()] = (int(ino), kind, int(size), int(mode, 8))
    return res


def as_kernel(walk):
    return {p: (r[0], r[2], r[3], r[4]) for p, r in walk.items()
            if p != b'/lost+found' and not p.startswith(b'/lost+found/')}


def write_delta(prev, cur, out):
    """#365's delta of `cur` against `prev`: the blocks that differ, at their offsets, then the
    bitmap of them. Returns the set of disk blocks."""
    blocks = set()
    with open(prev, 'rb') as a, open(cur, 'rb') as b, open(out, 'wb') as d:
        d.truncate(DISK)
        for n in range(DISK // C.BS):
            x, y = a.read(C.BS), b.read(C.BS)
            if x != y:
                blocks.add(n)
                d.seek(n * C.BS)
                d.write(y)
        m = 0
        for n in blocks:
            m |= 1 << n
        d.seek(DISK)
        d.write(m.to_bytes((DISK // C.BS + 63) // 64 * 8, 'little'))
    return blocks


def fs_blocks(disk_blocks):
    """Disk blocks to blocks of the root filesystem, as the comparer gets them."""
    first = PART_OFF // C.BS
    return {b - first for b in disk_blocks if first <= b < first + PART_SIZE // C.BS}


COPIES = []        # paths of the copies, in order
DELTAS = []        # DELTAS[k] turns copy k-1 into copy k (DELTAS[0] is None)
WRITTEN = []       # WRITTEN[k]: the disk blocks of DELTAS[k]


def night(m):
    """The phases of a night, each a function of the mount point. The copy after a phase is taken
    `wait` seconds later, with `sync` first or not."""
    def setup():
        os.makedirs(f'{m}/home/p/work/app/src')
        os.makedirs(f'{m}/home/p/work/fixtures')
        os.makedirs(f'{m}/home/p/big')
        for i in range(300):
            open(f'{m}/home/p/work/fixtures/f{i}.json', 'w').write(f'fixture {i}\n')
        for i in range(3000):
            open(f'{m}/home/p/big/e{i}', 'w').close()
        for i in range(40):
            open(f'{m}/home/p/work/app/src/m{i}.ts', 'wb').write(os.urandom(4096))
        os.symlink('../app', f'{m}/home/p/work/link-to-app')
        os.link(f'{m}/home/p/work/app/src/m1.ts', f'{m}/home/p/work/hard-m1')
        with open(f'{m}/home/p/frag', 'wb') as f:
            for i in range(200):
                f.seek(i * 8192)
                f.write(os.urandom(4096))
    def edits():
        shutil.rmtree(f'{m}/home/p/work/fixtures')
        os.rename(f'{m}/home/p/work/app/src/m2.ts', f'{m}/home/p/work/app/src/renamed.ts')
        open(f'{m}/home/p/work/app/src/m3.ts', 'a').write('edit\n')
        os.chmod(f'{m}/home/p/work/app/src/m4.ts', 0o600)
        os.unlink(f'{m}/home/p/work/app/src/m5.ts')
        open(f'{m}/home/p/work/app/src/m5.ts', 'wb').write(os.urandom(100))
        os.makedirs(f'{m}/home/p/work/new')
        for i in range(150):
            open(f'{m}/home/p/work/new/n{i}', 'w').write('n\n')
        for i in range(1500):
            os.unlink(f'{m}/home/p/big/e{i}')
    def move_tree():
        os.rename(f'{m}/home/p/work/app', f'{m}/home/p/app-moved')
        os.makedirs(f'{m}/home/p/work/app/deep/er/still')
        for i in range(20):
            open(f'{m}/home/p/work/app/deep/er/still/x{i}', 'w').write('x' * i)
        with open(f'{m}/home/p/frag', 'r+b') as f:   # rewrite in place: same size, same extents
            f.seek(8192 * 7)
            f.write(os.urandom(4096))
            os.fsync(f.fileno())   # the data on the disk; a write not flushed is not, as at a power cut
    def reuse():
        shutil.rmtree(f'{m}/home/p/work/new')    # frees 151 inodes...
        os.makedirs(f'{m}/home/p/work/again')    # ...which the next files take, other generations
        for i in range(160):
            open(f'{m}/home/p/work/again/a{i}', 'w').write('a\n')
        os.makedirs(f'{m}/home/p/node_modules/pkg')
        for i in range(400):
            open(f'{m}/home/p/node_modules/pkg/f{i}.js', 'w').write('module.exports = %d\n' % i)
    def churn():
        r = random.Random(7)
        names = [f'{m}/home/p/big/e{i}' for i in range(1500, 3000)]
        for n in r.sample(names, 600):
            os.unlink(n)
        for i in range(600):
            open(f'{m}/home/p/big/z{i}', 'w').write('z')
        big = f'{m}/home/p/blob'
        with open(big, 'wb') as f:
            f.write(os.urandom(3 << 20))
    return [(setup, True, 6), (edits, False, 7), (move_tree, False, 2), (reuse, False, 7), (churn, False, 1)]


def setUpModule():
    global W
    if os.geteuid() != 0:
        raise unittest.SkipTest('needs root for loop devices')
    W = tempfile.mkdtemp(prefix='ckptread-')
    disk = os.path.join(W, 'disk.img')
    with open(disk, 'wb') as f:
        f.truncate(DISK)
    make_gpt(disk, 'cloudimg-rootfs')
    sh(f'mkfs.ext4 -q -F -O metadata_csum,64bit,orphan_file -E offset={PART_OFF} {disk} {PART_SIZE // 1024}k')
    m = os.path.join(W, 'm')
    os.makedirs(m)
    sh(f'mount -o loop,offset={PART_OFF},sizelimit={PART_SIZE} {disk} {m}')
    try:
        c0 = os.path.join(W, 'c0.img')
        sh(f'sync; cp --sparse=always {disk} {c0}')
        COPIES.append(c0)
        DELTAS.append(None)
        WRITTEN.append(set())
        import time
        for k, (phase, sync, wait) in enumerate(night(m), 1):
            phase()
            if sync:
                sh('sync')
            time.sleep(wait)
            ck = os.path.join(W, f'c{k}.img')
            sh(f'cp --sparse=always {disk} {ck}')
            COPIES.append(ck)
    finally:
        sh(f'umount {m}')
    for k in range(1, len(COPIES)):
        d = os.path.join(W, f'd{k}')
        WRITTEN.append(write_delta(COPIES[k - 1], COPIES[k], d))
        DELTAS.append(d)


def tearDownModule():
    if W:
        shutil.rmtree(W, ignore_errors=True)


def image(k, through_deltas=False):
    if through_deltas:
        src = C.DeltaChain(COPIES[0], DELTAS[1:k + 1], DISK)
    else:
        src = C.FileSource(COPIES[k])
    return C.Image(C.root_fs(src))


class Units(unittest.TestCase):
    def test_crc32c(self):
        self.assertEqual(C.crc32c(b'123456789'), 0xE3069283)
        self.assertEqual(C.crc32c(b''), 0)

    def test_partition_by_label(self):
        src = C.FileSource(COPIES[0])
        self.assertEqual(C.gpt_partition(src), (PART_OFF, (PART_END - PART_START + 1) * 512))
        with self.assertRaises(C.Refused):
            C.gpt_partition(src, 'another')


class AgainstTheKernel(unittest.TestCase):
    def test_every_copy_reads_as_the_kernel_reads_it_after_replay(self):
        replayed = 0
        for k in range(len(COPIES)):
            with self.subTest(copy=k):
                img = image(k)
                self.assertEqual(img.gaps, [])
                self.assertEqual(as_kernel(img.walk()), kernel_walk(COPIES[k]))
                replayed += len(img.replayed)
                if img.replayed:
                    self.assertGreater(img.checksums_verified, 0)
        self.assertGreater(replayed, 0, 'no copy needed its journal replayed: the test proves nothing')
        print(f'\n  journal blocks replayed over {len(COPIES)} copies: {replayed}', file=sys.stderr)

    def test_without_replay_some_copy_reads_wrong(self):
        wrong = 0
        for k in range(len(COPIES)):
            try:
                w = as_kernel(C.Image(C.root_fs(C.FileSource(COPIES[k])), replay=False).walk())
            except C.Refused:
                wrong += 1
                continue
            wrong += w != kernel_walk(COPIES[k])
        self.assertGreater(wrong, 0)


class ThroughDeltas(unittest.TestCase):
    def test_a_checkpoint_read_through_its_deltas_is_the_copy(self):
        for k in range(len(COPIES)):
            with self.subTest(copy=k):
                chain = C.DeltaChain(COPIES[0], DELTAS[1:k + 1], DISK)
                with open(COPIES[k], 'rb') as f:
                    for b in sorted(set().union(*WRITTEN[:k + 1]))[:3000] + [0, 1, 256, 257]:
                        f.seek(b * C.BS)
                        self.assertEqual(chain.read(b * C.BS, C.BS), f.read(C.BS), b)
                self.assertEqual(image(k, True).walk(), image(k).walk())


class Comparison(unittest.TestCase):
    def check_pair(self, i, j, ix_i, extra=0, seed=0):
        a, b = image(i, True), image(j, True)
        written = fs_blocks(set().union(*WRITTEN[i + 1:j + 1]))
        r = random.Random(seed)
        written |= {r.randrange(a.blocks_count) for _ in range(extra)}  # written, same bytes
        # Reads counted after each image is open: both ways read the superblock, the group
        # descriptors and the journal first.
        before = a.blocks_read + b.blocks_read
        ix_j, content = C.compare(ix_i, a, b, written)
        set_read = a.blocks_read + b.blocks_read - before
        full_a, full_b = image(i, True), image(j, True)
        before = full_a.blocks_read + full_b.blocks_read
        wa, wb = full_a.walk(), full_b.walk()
        full_read = full_a.blocks_read + full_b.blocks_read - before
        self.assertEqual(ix_j.paths(), wb)
        self.assertEqual(C.diff_lines(ix_i.paths(), ix_j.paths()), C.diff_lines(wa, wb))
        return ix_j, content, set_read, full_read, len(C.diff_lines(wa, wb))

    def test_from_the_write_log_equals_a_full_walk_pair_by_pair(self):
        ix = C.Index.build(image(0, True))
        rows = []
        for k in range(1, len(COPIES)):
            with self.subTest(pair=(k - 1, k)):
                ix, content, set_read, full_read, lines = self.check_pair(k - 1, k, ix, extra=200, seed=k)
                rows.append(f'  {k - 1}->{k}: {lines:5d} lines, {len(WRITTEN[k]):5d} blocks written, '
                            f'read {set_read:5d} blocks from the set, {full_read:6d} in a full walk of both')
        print('\n' + '\n'.join(rows), file=sys.stderr)

    def test_across_several_checkpoints_with_the_writes_of_each(self):
        ix0 = C.Index.build(image(0, True))
        self.check_pair(0, len(COPIES) - 1, ix0)
        ix1 = C.Index.build(image(1, True))
        self.check_pair(1, 3, ix1, extra=1000, seed=99)

    def test_content_written_in_place_is_seen(self):
        # Phase 3 rewrites one block of `frag` in place: same size, same extents, so a full walk
        # sees only its mtime; the write log names the file.
        ix2 = C.Index.build(image(2, True))
        ix3, content, *_ = self.check_pair(2, 3, ix2)
        frag = ix3.paths()[b'/home/p/frag'][0]
        self.assertIn(frag, content)


class JournalFaults(unittest.TestCase):
    """Faults planted in a copy whose journal must be replayed: the reader stops or names a gap,
    and never crashes."""

    @classmethod
    def setUpClass(cls):
        best = max(range(len(COPIES)), key=lambda k: len(image(k).replayed))
        cls.copy = COPIES[best]
        cls.img = image(best)
        if not cls.img.replayed:
            raise unittest.SkipTest('no copy with a journal to replay')

    def planted(self, jindex, byte, xor=0xFF):
        p = os.path.join(W, 'fault.img')
        shutil.copyfile(self.copy, p)
        off = PART_OFF + self.img.jmap[jindex] * C.BS + byte
        with open(p, 'r+b') as f:
            f.seek(off)
            v = f.read(1)[0]
            f.seek(off)
            f.write(bytes([v ^ xor]))
        return p

    def read(self, p):
        return C.Image(C.root_fs(C.FileSource(p)))

    def test_a_bad_commit_ends_the_log_before_its_transaction(self):
        commits = [(i, s) for kind, i, s in self.img.journal_blocks if kind == 2]
        i, s = commits[-1]
        img = self.read(self.planted(i, 100))
        self.assertIn(f'journal-commit-checksum {s}', img.gaps)
        self.assertLess(len(img.replayed), len(self.img.replayed) + 1)

    def test_a_bad_block_is_not_replayed_and_is_named(self):
        data = [(i, s) for kind, i, s in self.img.journal_blocks if kind == 0]
        i, s = data[len(data) // 2]
        img = self.read(self.planted(i, 2000))
        self.assertTrue(any(g.startswith(f'journal-block-checksum {s} ') for g in img.gaps), img.gaps)

    def test_a_bad_descriptor_ends_the_log(self):
        desc = [(i, s) for kind, i, s in self.img.journal_blocks if kind == 1]
        i, s = desc[-1]
        img = self.read(self.planted(i, 40))
        self.assertIn(f'journal-descriptor-checksum {s}', img.gaps)

    def test_random_flips_in_the_journal_never_crash(self):
        r = random.Random(1)
        used = [i for _, i, _ in self.img.journal_blocks]
        outcomes = {}
        for n in range(150):
            p = self.planted(r.choice(used), r.randrange(C.BS), 1 << r.randrange(8))
            try:
                img = self.read(p)
                img.walk()
                key = 'gap' if img.gaps else 'read'
            except C.Refused as e:
                key = f'refused {e}'
            outcomes[key] = outcomes.get(key, 0) + 1
        print(f'\n  150 flips: {outcomes}', file=sys.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2, warnings='ignore')
