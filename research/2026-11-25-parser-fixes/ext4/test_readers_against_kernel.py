"""Both ext4 readers of the drafts against the kernel's own ext4, on images a computer could leave
behind: #381's proto/ext4read.py and #398's reader/ckptread.py. Each case changes a clean image,
mounts it read-only through a loop device (the kernel's reading) and walks it with each reader
(Silo's reading). Paths and kinds must be the kernel's, or the reader refuses with a gap where the
case allows it. On the crash cases the kernel is not asked: a reader either answers or refuses,
and any other exception fails. Cases 7, 8 and 9 of #401's README, and the htree cases beside them.

Root, loop devices, e2fsprogs. Run: python3 -m unittest test_readers_against_kernel -v
"""
import importlib.util, os, shutil, struct, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
RESEARCH = os.path.dirname(os.path.dirname(HERE))

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m

E = load('ext4read', os.path.join(RESEARCH, '2026-11-25-linux-main/proto/ext4read.py'))
C = load('ckptread', os.path.join(RESEARCH, '2026-11-25-write-log/reader/ckptread.py'))

W = tempfile.mkdtemp(prefix='ext4cases-')
MNT = os.path.join(W, 'mnt'); os.makedirs(MNT)
sh = lambda c: subprocess.run(c, shell=True, capture_output=True, text=True)
KIND = {'d': 'dir', 'f': 'file', 'l': 'link'}

def mkimg(name, feats='^has_journal,^metadata_csum', n=40):
    src = os.path.join(W, 'src'); shutil.rmtree(src, ignore_errors=True)
    os.makedirs(src + '/c'); os.makedirs(src + '/a')
    for i in range(1, n + 1):
        open(f'{src}/c/file_with_a_long_name_{i:02d}', 'w').write(str(i))
    open(src + '/a/f1', 'w').write('hello')
    img = os.path.join(W, name + '.img')
    if os.path.exists(img): os.remove(img)
    r = sh(f'truncate -s 4M {img} && mkfs.ext4 -q -b 1024 -O {feats} -d {src} {img}')
    assert r.returncode == 0, r.stderr
    return img

def htree_img(name, feats):
    """A directory the kernel itself grew past one block, so it carries an htree index."""
    img = mkimg(name, feats, n=0)
    assert sh(f'mount -o loop {img} {MNT}').returncode == 0
    os.makedirs(MNT + '/h')
    for i in range(300):
        open(f'{MNT}/h/an_entry_with_a_long_name_{i:03d}', 'w').close()
    sh(f'umount {MNT}')
    flags = sh(f"debugfs -R 'stat /h' {img}").stdout.split('Flags: ')[1].split()[0]
    assert int(flags, 16) & 0x1000                                   # the INDEX flag is set
    return img

def kernel(img):
    sh(f'umount {MNT}')
    r = sh(f'mount -o loop,ro,noload {img} {MNT}')
    assert r.returncode == 0, r.stderr
    r = sh(f"cd {MNT} && find . -mindepth 1 -not -path './lost+found*' -printf '%y %p\\n'")
    sh(f'umount {MNT}')
    out = {}
    for line in r.stdout.splitlines():
        t, p = line.split(' ', 1)
        out[p[1:].encode()] = KIND.get(t, 'other')
    return out

def ext4read(img):
    return {p: v[2] for p, v in E.Image(img, True).walk().items()}

def ckptread(img):
    return {p: v[2] for p, v in C.Image(C.FileSource(img), True).walk().items()}

READERS = (('ext4read', ext4read, E.Refused), ('ckptread', ckptread, C.Refused))

def read(reader, refused, img):
    try:
        return {p: k for p, k in reader(img).items() if not p.startswith(b'/lost+found')}
    except refused as e:
        return f'refused {e}'

def bmap(img, path, lblk):
    return int(sh(f"debugfs -R 'bmap {path} {lblk}' {img}").stdout)

def inode_offset(img, path):
    ino = int(sh(f"debugfs -R 'stat {path}' {img}").stdout.split('Inode: ')[1].split()[0])
    r = sh(f"debugfs -R 'imap <{ino}>' {img}").stdout
    return int(r.split('located at block ')[1].split(',')[0]) * 1024 + int(r.split('offset ')[1].strip(), 16)

def patch(img, off, b):
    with open(img, 'r+b') as f: f.seek(off); f.write(b)

def at(img, needle, delta=0):
    return open(img, 'rb').read().find(needle) + delta


class SameAsKernel(unittest.TestCase):
    """Where the kernel reads, each reader reads the same, or refuses where `may_refuse`."""

    def check(self, img, may_refuse=False):
        k = kernel(img)
        for name, reader, refused in READERS:
            with self.subTest(reader=name):
                got = read(reader, refused, img)
                print(f'{self.id().split(".")[-1]:48} {name:9} '
                      f'{got if isinstance(got, str) else "as Linux, %d paths" % len(got)}',
                      file=sys.stderr)
                if isinstance(got, str):
                    self.assertTrue(may_refuse, f'{name} {got}, the kernel reads {len(k)} paths')
                else:
                    self.assertEqual(sorted(set(got) - set(k)), [], f'{name} sees what Linux does not')
                    self.assertEqual(sorted(set(k) - set(got)), [], f'{name} misses what Linux lists')
                    self.assertEqual({p: got[p] for p in k}, k, f'{name} kinds')

    def test_clean(self):
        self.check(mkimg('clean'))

    def test_clean_with_checksums(self):
        self.check(mkimg('clean-csum', '^has_journal,metadata_csum'))

    def test_htree(self):
        self.check(htree_img('htree', '^has_journal,^metadata_csum'))

    def test_htree_with_checksums(self):
        self.check(htree_img('htree-csum', '^has_journal,metadata_csum'))

    def test_dirent_type_0xde_is_listed_by_linux(self):
        """Finding 7: type byte 0xDE, the checksum tail's, on a file system without checksums."""
        img = mkimg('de'); patch(img, at(img, b'file_with_a_long_name_07', -1), b'\xde')
        self.check(img)

    def test_dir_size_below_its_blocks(self):
        """Finding 8: /c has two blocks and a size of one; the kernel reads up to the size."""
        img = mkimg('dirsize'); sh(f"debugfs -w -R 'sif /c size 1024' {img}")
        self.check(img)

    def test_dir_block_whose_checksum_fails(self):
        """Finding 8: the kernel refuses a directory block whose checksum fails."""
        img = mkimg('csum', '^has_journal,metadata_csum')
        patch(img, at(img, b'file_with_a_long_name_07'), b'X')
        self.check(img, may_refuse=True)

    def test_name_with_nul(self):
        """Finding 8: a name with a NUL byte is `file` to Linux. No path holds a NUL."""
        img = mkimg('nul'); patch(img, at(img, b'file_with_a_long_name_07', 4), b'\0')
        self.check(img, may_refuse=True)

    def test_dir_extent_unwritten(self):
        img = mkimg('unwritten'); o = inode_offset(img, '/c') + 40 + 12 + 4
        ln = struct.unpack('<H', open(img, 'rb').read()[o:o + 2])[0]
        patch(img, o, struct.pack('<H', ln + 32768))
        self.check(img, may_refuse=True)

    def test_htree_leaf_the_index_does_not_name(self):
        """The index's count is lowered by one: its last leaf is still a block of the directory,
        and the kernel, which lists an indexed directory through its index, does not list it."""
        img = htree_img('htree-unnamed', '^has_journal,^metadata_csum')
        root = bmap(img, '/h', 0) * 1024
        count = struct.unpack('<H', open(img, 'rb').read()[root + 0x22: root + 0x24])[0]
        patch(img, root + 0x22, struct.pack('<H', count - 1))
        self.check(img, may_refuse=True)


class NeverCrashes(unittest.TestCase):
    """Finding 9: what the fuzzer found, each made by hand. A refusal is the only other answer."""

    def answers_or_refuses(self, img):
        for name, reader, refused in READERS:
            with self.subTest(reader=name):
                try:
                    reader(img)
                except refused:
                    pass

    def sb(self, name, off, value, fmt='<I'):
        img = mkimg(name); patch(img, 1024 + off, struct.pack(fmt, value)); return img

    def test_zero_inodes_per_group(self):
        self.answers_or_refuses(self.sb('ipg0', 40, 0))

    def test_zero_blocks_per_group(self):
        self.answers_or_refuses(self.sb('bpg0', 32, 0))

    def test_inode_in_a_group_past_the_end(self):
        img = self.sb('ino-past', 0, 0xFFFFFF00)
        o = at(img, b'file_with_a_long_name_07', -8)
        patch(img, o, struct.pack('<I', 0xFFFFFF00))
        self.answers_or_refuses(img)

    def test_inode_size_past_a_block(self):
        self.answers_or_refuses(self.sb('isize', 88, 4096, '<H'))

    def test_inode_size_too_small(self):
        self.answers_or_refuses(self.sb('isize8', 88, 8, '<H'))

    def test_journal_with_no_blocks(self):
        """Found by fz_readers.py on the fixed ext4read: the journal inode's extent tree is emptied,
        so the journal has no block 0."""
        img = os.path.join(W, 'journal.img')
        if os.path.exists(img): os.remove(img)
        assert sh(f'truncate -s 16M {img} && mkfs.ext4 -q -b 1024 -J size=1 {img}').returncode == 0
        r = sh(f"debugfs -R 'imap <8>' {img}").stdout
        o = int(r.split('located at block ')[1].split(',')[0]) * 1024 + int(r.split('offset ')[1].strip(), 16)
        patch(img, o + 40 + 2, struct.pack('<H', 0))
        self.answers_or_refuses(img)

    def test_dirent_header_past_the_block(self):
        """The last entry's length is shortened by 4: the next header would start 4 bytes before
        the block ends."""
        img = mkimg('dirent-past')
        blk = bmap(img, '/a', 0) * 1024
        data = open(img, 'rb').read()
        o = 0
        while True:
            rec_len = struct.unpack('<H', data[blk + o + 4: blk + o + 6])[0]
            if o + rec_len >= 1024: break
            o += rec_len
        patch(img, blk + o + 4, struct.pack('<H', rec_len - 4))
        self.answers_or_refuses(img)


def tearDownModule():
    sh(f'umount {MNT}')
    shutil.rmtree(W, ignore_errors=True)

if __name__ == '__main__':
    unittest.main()
