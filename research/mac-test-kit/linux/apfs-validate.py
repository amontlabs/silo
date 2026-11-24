"""Checks py/apfs/apfsread.py against disks Apple's APFS wrote: dissect.apfs's test images.

  python3 linux/apfs-validate.py DIR

DIR holds snapshot.bin, corrupt.bin, encrypted.bin (gunzipped from dissect.apfs's tests/_data,
fetched with linux/test-apfs.sh). Prints one line per check and exits 1 on a failure.
"""
import os
import random
import shutil
import struct
import subprocess
import sys
import tempfile

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(here, "..", "py", "apfs"))
import apfsread as A  # noqa: E402

d = sys.argv[1]
fails = 0


def check(name, ok, detail=""):
    global fails
    print("%s %s %s" % ("ok  " if ok else "FAIL", name, detail))
    fails += 0 if ok else 1


# 1. The leaf diff equals the full walk on every neighbouring pair of 512 snapshots and 200 random pairs.
f, cs = A.open_image(os.path.join(d, "snapshot.bin"))
v = A.pick(cs, None)
snaps = v.snapshots()
check("512 snapshots listed", len(snaps) == 512, str(len(snaps)))
trees = [v.snapshot_tree(sb) for _, _, sb in snaps] + [v.fs_tree()]
leaves = [set(t.leaves()) for t in trees]
recs = []
for t, ls in zip(trees, leaves):
    r = {}
    for p in ls:
        r.update(t.records(p[0]))
    recs.append(set(r.items()))
random.seed(1)
pairs = [(i, i + 1) for i in range(len(trees) - 1)]
pairs += [tuple(sorted(random.sample(range(len(trees)), 2))) for _ in range(200)]
same = 0
for i, j in pairs:
    oa, ob = leaves[i] - leaves[j], leaves[j] - leaves[i]
    ra, rb = {}, {}
    for p in oa:
        ra.update(trees[i].records(p[0]))
    for p in ob:
        rb.update(trees[j].records(p[0]))
    same += (set(ra.items()) ^ set(rb.items())) == (recs[i] ^ recs[j])
check("leaf diff equals full walk", same == len(pairs), "%d of %d pairs" % (same, len(pairs)))

# 2. A broken newest checkpoint: opens the one before and says which it skipped.
f, cs = A.open_image(os.path.join(d, "corrupt.bin"))
c = cs[0]
check("corrupt.bin opens checkpoint 302, skips 304",
      c.get("opened_xid") == 302 and c.get("newest_xid") == 304 and not c.get("newest_ok"),
      str({k: c.get(k) for k in ("newest_xid", "opened_xid", "skipped")}))

# 3. An encrypted volume is a gap, not an empty tree.
f, cs = A.open_image(os.path.join(d, "encrypted.bin"))
vs = cs[0].get("volumes", [])
check("encrypted.bin: encrypted volume is a gap", bool(vs) and vs[0].get("encrypted") and "gap" in vs[0], str(vs))

# 4. Faults put in on purpose: each must give a gap or a fallback, never a silent diff.
tmp = tempfile.mkdtemp()
try:
    src = os.path.join(d, "snapshot.bin")
    f, cs = A.open_image(src)
    v = A.pick(cs, None)
    leaf = sorted(v.fs_tree().leaves())[3][0]
    bs = v.d.bs
    cases = {
        "a byte of a leaf changed": (leaf * bs + 200, b"\x5a"),
        "block 0 superblock magic broken: a gap, not a diff": (32, b"XXXX"),
    }
    for name, (off, data) in cases.items():
        m = os.path.join(tmp, "m.bin")
        shutil.copy(src, m)
        with open(m, "r+b") as fh:
            fh.seek(off)
            fh.write(data)
        out = subprocess.run([sys.executable, os.path.join(here, "..", "py", "apfs", "apfsread.py"),
                              "leafdiff", src, m, "--full"], capture_output=True, text=True).stdout
        if name.startswith("a byte"):
            # a write past APFS keeps the leaf's address: only a read of the leaf (the full walk,
            # or the Mac's block map of test 05) sees it
            check("fault: " + name + ": the full walk reports it", "bad checksum" in out, out[:160])
        else:
            check("fault: " + name, '"gap"' in out, out[:160])
    # a leaf moved to another block with its bytes: same records, other address, counts as changed
finally:
    shutil.rmtree(tmp)

sys.exit(1 if fails else 0)
