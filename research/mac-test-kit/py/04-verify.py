"""Checks a copy of what guest/seqwriter.pl wrote. Prints one JSON object.

  04-verify.py raw IMAGE MAGIC FIRSTBLOCK NBLOCKS N
      reads the blocks of writes 0..N-1. prefix: how many writes, from 0, are all there;
      after_prefix: writes found after the first missing one (a copy that is a moment in time
      has none).
  04-verify.py file PATH MAGIC
      reads the records. prefix: whole records in order from 0; bad: records that are torn,
      out of order or of another magic after the prefix; tail_bytes: bytes after the last
      whole record (a cut last record, or zeros the file was extended with).
"""
import json
import struct
import sys


def raw(path, magic, first, nblk, n):
    prefix, after, missing = None, 0, 0
    with open(path, "rb") as f:
        for i in range(n):
            pos = first + ((i * 2654435761 + magic) % nblk)
            f.seek(pos * 4096)
            b = f.read(16)
            ok = len(b) == 16 and struct.unpack("<QQ", b) == (magic, i)
            if not ok:
                missing += 1
                if prefix is None:
                    prefix = i
            elif prefix is not None:
                after += 1
    return {"prefix": n if prefix is None else prefix, "after_prefix": after, "checked": n}


def records(path, magic):
    data = open(path, "rb").read()
    n = len(data) // 64
    prefix, bad, zero = None, 0, 0
    for i in range(n):
        r = data[i * 64:(i + 1) * 64]
        ok = False
        try:
            t = r.decode("ascii")
            p = t.split()
            body = "SEQ %s %s" % (p[1], p[2])
            ok = (r[-1:] == b"\n" and p[0] == "SEQ" and int(p[1], 16) == magic and int(p[2]) == i
                  and int(p[3][:8], 16) == sum(body.encode()) % (1 << 32))
        except (UnicodeDecodeError, ValueError, IndexError):
            ok = False
        if not ok:
            if not any(r):
                zero += 1
            if prefix is None:
                prefix = i
            bad += 1
    return {"prefix": n if prefix is None else prefix, "bad_after_prefix": bad, "zero_records": zero,
            "tail_bytes": len(data) - n * 64, "bytes": len(data)}


if __name__ == "__main__":
    a = sys.argv[1:]
    if a and a[0] == "raw" and len(a) == 6:
        print(json.dumps(raw(a[1], int(a[2], 16), int(a[3]), int(a[4]), int(a[5]))))
    elif a and a[0] == "file" and len(a) == 3:
        print(json.dumps(records(a[1], int(a[2], 16))))
    else:
        sys.stderr.write(__doc__)
        sys.exit(2)
