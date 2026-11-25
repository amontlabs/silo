#!/usr/bin/env python3
"""Capture checkpoint deltas of a loop-mounted ext4 disk image during a workload.

snap <dir> <img> <mnt> <tag>: freeze the file system, record for every 4 KiB block of the
image a hash of its content (0 for a block of zeros or a hole) and the category of the file
that owns it, keep a copy of the image, and write the delta since the previous snap: the old
content of every block whose content changed and was not zero before (what a checkpoint holds
once the block is rewritten).
"""
import hashlib, json, os, subprocess, sys, struct, fcntl, array, time
import numpy as np

BS = 4096
CHUNK = 64 << 20

def data_ranges(fd, size):
    off = 0
    while off < size:
        try:
            d = os.lseek(fd, off, os.SEEK_DATA)
        except OSError:
            return
        h = os.lseek(fd, d, os.SEEK_HOLE)
        yield d, h
        off = h

def hash_image(path):
    size = os.path.getsize(path)
    n = size // BS
    H = np.zeros(n, dtype=np.uint64)
    zero = bytes(BS)
    fd = os.open(path, os.O_RDONLY)
    for d, h in data_ranges(fd, size):
        d -= d % BS
        off = d
        while off < h:
            ln = min(CHUNK, h - off)
            buf = os.pread(fd, ln, off)
            for i in range(len(buf) // BS):
                b = buf[i*BS:(i+1)*BS]
                if b != zero:
                    H[off // BS + i] = int.from_bytes(hashlib.blake2b(b, digest_size=8).digest(), "little") | 1
            off += ln
    os.close(fd)
    return H

CATS = ["meta", "system", "source", "git", "node_modules", "npm-cache", "target",
        "cargo-home", "venv", "pip-cache", "other-cache", "free"]

def category(rel):
    p = "/" + rel
    if p.startswith("/system/"): return "system"
    if "/node_modules/" in p: return "node_modules"
    if "/.npm/" in p: return "npm-cache"
    if "/target/" in p: return "target"
    if "/.cargo/" in p: return "cargo-home"
    if "/.venv/" in p or "/venv/" in p: return "venv"
    if "/.cache/pip/" in p: return "pip-cache"
    if "/.cache/" in p: return "other-cache"
    if "/.git/" in p: return "git"
    return "source"

FS_IOC_FIEMAP = 0xC020660B
def extents(fd):
    out = []
    start = 0
    while True:
        cnt = 256
        req = bytearray(32 + cnt * 56)
        struct.pack_into("QQIIII", req, 0, start, 0xFFFFFFFFFFFFFFFF, 1, 0, cnt, 0)  # FIEMAP_FLAG_SYNC
        fcntl.ioctl(fd, FS_IOC_FIEMAP, req, True)
        mapped = struct.unpack_from("I", req, 20)[0]
        if mapped == 0:
            return out
        last = False
        for i in range(mapped):
            lo, ph, ln, _, _, fl = struct.unpack_from("QQQQQI", req, 32 + i * 56)
            out.append((ph, ln))
            if fl & 1: last = True
            start = lo + ln
        if last:
            return out

def block_categories(mnt, nblocks):
    C = np.full(nblocks, CATS.index("meta"), dtype=np.uint8)
    for root, dirs, files in os.walk(mnt):
        for f in files:
            p = os.path.join(root, f)
            try:
                if os.path.islink(p) or not os.path.isfile(p): continue
                fd = os.open(p, os.O_RDONLY | os.O_NOFOLLOW)
            except OSError:
                continue
            try:
                c = CATS.index(category(os.path.relpath(p, mnt)))
                for ph, ln in extents(fd):
                    C[ph // BS:(ph + ln + BS - 1) // BS] = c
            except OSError:
                pass
            finally:
                os.close(fd)
    return C

def snap(d, img, mnt, tag):
    os.makedirs(d, exist_ok=True)
    state = json.load(open(f"{d}/state.json")) if os.path.exists(f"{d}/state.json") else {"tags": []}
    subprocess.run(["sync"], check=True)
    subprocess.run(["fstrim", mnt], check=False)
    nblocks = os.path.getsize(img) // BS
    C = block_categories(mnt, nblocks)
    subprocess.run(["fsfreeze", "-f", mnt], check=True)
    try:
        t0 = time.time()
        H = hash_image(img)
        if state["tags"]:
            prev = state["tags"][-1]
            Hp = np.load(f"{d}/{prev}.hash.npy")
            Cp = np.load(f"{d}/{prev}.cat.npy")
            ch = np.nonzero((Hp != H) & (Hp != 0))[0]
            fd = os.open(f"{d}/prev.img", os.O_RDONLY)
            with open(f"{d}/{tag}.delta", "wb") as out:
                for b in ch:
                    out.write(os.pread(fd, BS, int(b) * BS))
            os.close(fd)
            np.savez(f"{d}/{tag}.delta.npz", blocks=ch, oldhash=Hp[ch], cat=Cp[ch])
            print(json.dumps({"tag": tag, "changed_held_mib": len(ch) * BS / 2**20,
                              "new_or_changed_mib": int(((Hp != H) & (H != 0)).sum()) * BS / 2**20,
                              "live_mib": int((H != 0).sum()) * BS / 2**20}), flush=True)
        subprocess.run(["cp", "--sparse=always", img, f"{d}/prev.img.tmp"], check=True)
        os.replace(f"{d}/prev.img.tmp", f"{d}/prev.img")
    finally:
        subprocess.run(["fsfreeze", "-u", mnt], check=True)
    np.save(f"{d}/{tag}.hash.npy", H)
    np.save(f"{d}/{tag}.cat.npy", C)
    state["tags"].append(tag)
    json.dump(state, open(f"{d}/state.json", "w"))

if __name__ == "__main__":
    snap(*sys.argv[1:5])
