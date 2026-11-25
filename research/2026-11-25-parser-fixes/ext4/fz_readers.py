"""#401's python/fz_ext4.py, for both readers: #381's ext4read.py (READER=ext4read) and #398's
ckptread.py (READER=ckptread). Changes of 1 to 8 bytes in the blocks a clean read touches; the
reader either answers or refuses, and any other exception is written to OUT, one per kind.
usage: READER=ext4read SEED=image OUT=dir python3 fz_readers.py [libFuzzer args]"""
import atheris, importlib, os, sys, traceback
sys.setrecursionlimit(10000)
HERE = os.path.dirname(os.path.abspath(__file__))
RESEARCH = os.path.dirname(os.path.dirname(HERE))
PATHS = {'ext4read': '2026-11-25-linux-main/proto/ext4read.py',
         'ckptread': '2026-11-25-write-log/reader/ckptread.py'}
NAME = os.environ['READER']
sys.path.insert(0, os.path.dirname(os.path.join(RESEARCH, PATHS[NAME])))
with atheris.instrument_imports(include=[NAME]):
    R = importlib.import_module(NAME)
SEED = open(os.environ['SEED'], 'rb').read()
OUT = os.environ.get('OUT', 'crashes')

class Bytes:
    """A source over bytes, recording which 1 KiB blocks are read."""
    def __init__(self, data, touched=None):
        self.data, self.touched, self.size = data, touched, len(data)
    def read(self, off, n):
        if self.touched is not None:
            self.touched.update(range(off // 1024, (off + n - 1) // 1024 + 1))
        b = self.data[off: off + n]
        if off < 0 or len(b) != n:
            raise R.Refused('short-read')
        return b

def walk(data, touched=None):
    if NAME == 'ckptread':
        return R.Image(Bytes(data, touched), True).walk()
    img = R.Image.__new__(R.Image)
    src = Bytes(data, touched)
    R.Image.raw = lambda s, off, n: src.read(s.base + off, n)
    R.Image.__init__(img, os.devnull, True)
    return img.walk()

touched = set(); walk(SEED, touched); TOUCHED = sorted(touched)
print(f'# {NAME}: {len(TOUCHED)} blocks of 1 KiB touched by a clean read', file=sys.stderr)
SEEN = set()

def one(buf):
    fdp = atheris.FuzzedDataProvider(buf)
    data = bytearray(SEED)
    for _ in range(fdp.ConsumeIntInRange(1, 8)):
        blk = TOUCHED[fdp.ConsumeIntInRange(0, len(TOUCHED) - 1)]
        off = blk * 1024 + fdp.ConsumeIntInRange(0, 1023)
        chunk = fdp.ConsumeBytes(fdp.ConsumeIntInRange(1, 8))
        data[off: off + len(chunk)] = chunk
    try:
        walk(bytes(data))
    except R.Refused:
        pass
    except Exception as e:
        tb = traceback.extract_tb(e.__traceback__)[-1]
        sig = f'{type(e).__name__} at line {tb.lineno} ({tb.line.strip()[:60]})'
        if sig not in SEEN:
            SEEN.add(sig)
            os.makedirs(OUT, exist_ok=True)
            fn = f'{OUT}/crash-{len(SEEN):02d}.img'
            open(fn, 'wb').write(bytes(data))
            print(f'\nCRASH {sig} -> {fn}', file=sys.stderr, flush=True)

atheris.Setup(sys.argv, one)
atheris.Fuzz()
