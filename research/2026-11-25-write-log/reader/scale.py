#!/usr/bin/env python3
"""The comparison from the write log against a full walk, on Ubuntu 26.04's own disk grown to hold
about a million paths, with a night of 1,000 changed files between two checkpoints.

  sudo python3 -I scale.py DISK.raw WORKDIR

DISK.raw is Ubuntu's cloud image converted to raw (#381's proto/qcow2raw.py). It is copied into
WORKDIR, grown to 12 GiB, its root partition and filesystem grown to fill it, and filled. Needs
root, a loop device and e2fsprogs; needs no KVM.
"""
import os, sys, time, random, shutil, struct, subprocess, zlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ckptread as C

def sh(cmd):
    return subprocess.run(cmd, shell=True, check=True, capture_output=True, text=True).stdout

def grow_gpt(path, label, new_size):
    """Moves the end of partition `label`, the last one, to the end of a disk of `new_size`."""
    with open(path, 'r+b') as f:
        f.truncate(new_size)
        f.seek(512)
        hdr = bytearray(f.read(92))
        first_entry, count, esize = struct.unpack_from('<QII', hdr, 72)
        f.seek(first_entry * 512)
        entries = bytearray(f.read(count * esize))
        last = new_size // 512 - 34
        for i in range(count):
            e = i * esize
            if entries[e + 56:e + 128].decode('utf-16-le', 'replace').rstrip('\x00') == label:
                struct.pack_into('<Q', entries, e + 40, last)
        f.seek(first_entry * 512)
        f.write(entries)
        struct.pack_into('<Q', hdr, 48, last)                  # last usable sector
        struct.pack_into('<Q', hdr, 32, new_size // 512 - 1)   # where the backup header goes
        struct.pack_into('<I', hdr, 88, zlib.crc32(entries))
        struct.pack_into('<I', hdr, 16, 0)
        struct.pack_into('<I', hdr, 16, zlib.crc32(hdr))
        f.seek(512)
        f.write(hdr)

def block_diff(a, b, first, count):
    """The blocks of the filesystem whose bytes differ between two disk files."""
    out = set()
    chunk = 256
    with open(a, 'rb') as fa, open(b, 'rb') as fb:
        for start in range(0, count, chunk):
            fa.seek((first + start) * C.BS)
            fb.seek((first + start) * C.BS)
            n = min(chunk, count - start)
            x, y = fa.read(n * C.BS), fb.read(n * C.BS)
            if x == y:
                continue
            for k in range(n):
                if x[k * C.BS:(k + 1) * C.BS] != y[k * C.BS:(k + 1) * C.BS]:
                    out.add(start + k)
    return out

def main():
    src, w = sys.argv[1], sys.argv[2]
    os.makedirs(w, exist_ok=True)
    disk = os.path.join(w, 'disk.raw')
    shutil.copyfile(src, disk)
    grow_gpt(disk, 'cloudimg-rootfs', 12 << 30)
    off, size = C.gpt_partition(C.FileSource(disk))
    loop = sh(f'losetup -f --show -o {off} --sizelimit {size} {disk}').strip()
    m = os.path.join(w, 'm')
    os.makedirs(m, exist_ok=True)
    try:
        subprocess.run(['e2fsck', '-fy', loop], capture_output=True)
        sh(f'resize2fs {loop}')
        sh(f'mount {loop} {m}')
        try:
            t = time.time()
            r = random.Random(3)
            paths = []
            for d in range(900):     # a home folder of many repositories
                base = f'{m}/home/p/work/repo{d // 30}/pkg{d}'
                os.makedirs(base)
                for i in range(1000):
                    p = f'{base}/f{i}.ts'
                    with open(p, 'w') as f:
                        f.write('x' * r.randrange(1, 3000))
                    paths.append(p)
            sh('sync')
            print(f'filled in {time.time() - t:.0f} s', file=sys.stderr)
            time.sleep(40)           # the journal written back, as after an idle stretch
            sh('sync')
            c0 = os.path.join(w, 'c0.raw')
            sh(f'cp --sparse=always {disk} {c0}')
            # The night: 1,000 changed files, then a copy 7 s later with no sync.
            pick = r.sample(paths, 700)
            for p in pick[:400]:
                with open(p, 'a') as f:
                    f.write('edit\n')
            for p in pick[400:600]:
                os.unlink(p)
            for p in pick[600:700]:
                os.rename(p, p + '.renamed')
            os.makedirs(f'{m}/home/p/work/new')
            for i in range(300):
                with open(f'{m}/home/p/work/new/n{i}.ts', 'w') as f:
                    f.write('n' * i)
            time.sleep(7)
            c1 = os.path.join(w, 'c1.raw')
            sh(f'cp --sparse=always {disk} {c1}')
        finally:
            sh(f'umount {m}')
    finally:
        sh(f'losetup -d {loop}')

    first, count = off // C.BS, size // C.BS
    written = block_diff(c0, c1, first, count)
    img = lambda p: C.Image(C.FileSource(p).slice(off, size))

    a0 = img(c0)
    t = time.time()
    before = a0.blocks_read
    ix0 = C.Index.build(a0)
    print(f'index of c0: {len(ix0.rec)} inodes, {a0.blocks_read - before} blocks, {time.time() - t:.1f} s')

    a, b = img(c0), img(c1)
    print(f'opening both, superblocks and journals: {a.blocks_read + b.blocks_read} blocks read')
    print(f'journal: c0 replays {len(a.replayed)} blocks, c1 replays {len(b.replayed)}, '
          f'{a.checksums_verified + b.checksums_verified} checksums checked, gaps {a.gaps + b.gaps}')
    t = time.time()
    before = a.blocks_read + b.blocks_read
    ix1, content = C.compare(ix0, a, b, written)
    set_s, set_blocks = time.time() - t, a.blocks_read + b.blocks_read - before

    fa, fb = img(c0), img(c1)
    t = time.time()
    before = fa.blocks_read + fb.blocks_read
    wa, wb = fa.walk(), fb.walk()
    full_s, full_blocks = time.time() - t, fa.blocks_read + fb.blocks_read - before

    lines = C.diff_lines(wa, wb)
    same = C.diff_lines(ix0.paths(), ix1.paths()) == lines and ix1.paths() == wb
    kinds = {}
    for l in lines:
        kinds[l[0]] = kinds.get(l[0], 0) + 1
    print(f'paths: {len(wa)} before, {len(wb)} after; lines {len(lines)} {kinds}')
    print(f'blocks written between the two: {len(written)}')
    print(f'from the write log: read {set_blocks} blocks ({set_blocks * 4 // 1024} MiB), {set_s:.1f} s')
    print(f'full walk of both:  read {full_blocks} blocks ({full_blocks * 4 // 1024} MiB), {full_s:.1f} s')
    print(f'equal: {same}')
    sys.exit(0 if same else 1)

if __name__ == '__main__':
    main()
