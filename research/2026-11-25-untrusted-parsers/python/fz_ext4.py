"""#381 proto/ext4read.py, coverage-guided: a real ext4 image, with bytes changed in the blocks
the reader reads. Property: the reader either answers or refuses (`Refused`); any other
exception is a crash of the night's comparison. Hangs and memory are libFuzzer's -timeout and
-rss_limit_mb. usage: SRC=ext4read.py SEED=image python3 fz_ext4.py [libFuzzer args]"""
import atheris, os, sys, struct, importlib.util
sys.setrecursionlimit(10000)
sys.path.insert(0, os.path.dirname(os.path.abspath(os.environ["SRC"])))
with atheris.instrument_imports(include=["ext4read"]):
    import ext4read as E
SEED = open(os.environ["SEED"], "rb").read()
REPLAY = os.environ.get("REPLAY", "1") == "1"

def _raw(s, off, n):
    b = s.data[s.base + off: s.base + off + n]
    if len(b) != n: raise E.Refused("short-read")
    return b

# Find the blocks a clean read touches, to aim the changes there.
touched = set()
E.Image.raw = lambda s, off, n: (touched.update(range(off // 1024, (off + n - 1) // 1024 + 1)), _raw(s, off, n))[1]
class Probe(E.Image):
    def __init__(self, data):
        self.data = data
        E.Image.__init__(self, os.environ["SEED"], REPLAY)
p = Probe(SEED); p.walk()
E.Image.raw = _raw
TOUCHED = sorted(touched)
print(f"# {len(TOUCHED)} blocks of 1 KiB touched by a clean read", file=sys.stderr)

class Fz(E.Image):
    def __init__(self, data):
        self.data = data
        E.Image.__init__(self, os.environ["SEED"], REPLAY)

def one(buf):
    fdp = atheris.FuzzedDataProvider(buf)
    data = bytearray(SEED)
    for _ in range(fdp.ConsumeIntInRange(1, 8)):
        blk = TOUCHED[fdp.ConsumeIntInRange(0, len(TOUCHED) - 1)]
        off = blk * 1024 + fdp.ConsumeIntInRange(0, 1023)
        chunk = fdp.ConsumeBytes(fdp.ConsumeIntInRange(1, 8))
        data[off: off + len(chunk)] = chunk
    try:
        img = Fz(bytes(data))
        img.walk()
    except E.Refused:
        pass
    except Exception as e:
        # One input per kind of crash, then go on: the run shows every kind, not the first.
        import traceback
        tb = traceback.extract_tb(e.__traceback__)[-1]
        sig = f"{type(e).__name__} at line {tb.lineno} ({tb.line.strip()[:60]})"
        if sig not in SEEN:
            SEEN.add(sig)
            os.makedirs(OUT, exist_ok=True)
            fn = f"{OUT}/crash-{len(SEEN):02d}.img"
            open(fn, "wb").write(bytes(data))
            print(f"\nCRASH {sig} -> {fn}", file=sys.stderr, flush=True)

SEEN = set(); OUT = os.environ.get("OUT", "crashes")
atheris.Setup(sys.argv, one)
atheris.Fuzz()
