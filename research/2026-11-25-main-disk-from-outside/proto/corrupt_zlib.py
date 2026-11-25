"""The same measure as corrupt.c, for decmpfs's zlib blocks: one bit flipped at a random place of
each encoded block of 64 KiB, decoded by Python's zlib (which checks the stream's Adler-32), and
by the probe's rule (exact size).

  python3 corrupt_zlib.py FILE...
"""
import random
import sys
import zlib

BLOCK, FLIPS, MAX = 65536, 200, 400
rng = random.Random(0x5110)
counts = {"refused": 0, "same": 0, "wrong": 0}
blocks = 0
per_file = MAX // len(sys.argv[1:]) + 1
for path in sys.argv[1:]:
    with open(path, "rb") as f:
        for _ in range(per_file):
            raw = f.read(BLOCK)
            if len(raw) < 4096:
                break
            enc = bytearray(zlib.compress(raw, 5))
            if len(enc) >= len(raw):
                continue
            blocks += 1
            for _ in range(FLIPS):
                at, bit = rng.randrange(len(enc)), 1 << rng.randrange(8)
                enc[at] ^= bit
                try:
                    d = zlib.decompressobj()
                    out = d.decompress(bytes(enc), len(raw) + 64)
                    ok = d.eof and len(out) == len(raw) and not d.unused_data
                except zlib.error:
                    ok = False
                if not ok:
                    counts["refused"] += 1
                elif out == raw:
                    counts["same"] += 1
                else:
                    counts["wrong"] += 1
                enc[at] ^= bit
t = sum(counts.values())
print("zlib: %d blocks, %d flips each" % (blocks, FLIPS))
print("  strict refused %5.1f%%  same %5.1f%%  wrong without an error %5.1f%%" % (
    100 * counts["refused"] / t, 100 * counts["same"] / t, 100 * counts["wrong"] / t))
