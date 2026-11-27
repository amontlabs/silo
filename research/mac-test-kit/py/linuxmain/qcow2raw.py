#!/usr/bin/env python3
"""Converts a qcow2 image to a raw disk, as the helper's `convert` would, and refuses every shape
it does not expect: a version other than 3, a backing file, encryption, snapshots, external data,
extended L2 entries, a compression other than deflate. A trial: Silo's converter is Zig.

  qcow2raw.py IN.qcow2 OUT.raw      prints the clusters read, compressed and zero
"""
import struct, sys, zlib, os

class Refused(Exception):
    pass

def main(src, dst):
    f = open(src, 'rb')
    h = f.read(112)
    if h[:4] != b'QFI\xfb':
        raise Refused('magic')
    (version, backing_off, backing_len, cluster_bits, size, crypt, l1_size, l1_off, _rt_off,
     _rt_clusters, nsnap, _snap_off) = struct.unpack('>IQIIQIIQQIIQ', h[4:72])
    incompat, _compat, _autoclear, _refbits, hdr_len = struct.unpack('>QQQII', h[72:104])
    if version != 3: raise Refused('version')
    if backing_off or backing_len: raise Refused('backing-file')
    if crypt: raise Refused('encrypted')
    if nsnap: raise Refused('snapshots')
    if incompat & ~0x1:                       # only "dirty" (bit 0) is allowed; it changes nothing to read
        raise Refused(f'incompatible-features {incompat:#x}')
    if hdr_len > 104 and h[104] != 0:
        raise Refused('compression-type')     # 0 is deflate
    if not 9 <= cluster_bits <= 21: raise Refused('cluster-bits')
    cs = 1 << cluster_bits
    if l1_size * (cs // 8) * cs < size: raise Refused('l1-too-small')
    f.seek(l1_off)
    l1 = struct.unpack(f'>{l1_size}Q', f.read(8 * l1_size))
    out = open(dst, 'wb')
    out.truncate(size)
    n_data = n_comp = n_zero = 0
    per_l2 = cs // 8
    flen = os.fstat(f.fileno()).st_size
    for i, e1 in enumerate(l1):
        l2_off = e1 & 0x00FFFFFFFFFFFE00
        if not l2_off:
            continue
        if l2_off + cs > flen: raise Refused('l2-past-end')
        f.seek(l2_off)
        l2 = struct.unpack(f'>{per_l2}Q', f.read(cs))
        for j, e2 in enumerate(l2):
            vaddr = (i * per_l2 + j) * cs
            if vaddr >= size:
                break
            if e2 & (1 << 62):                # compressed
                x = 62 - (cluster_bits - 8)
                off = e2 & ((1 << x) - 1)
                nsec = ((e2 >> x) & ((1 << (62 - x)) - 1)) + 1
                f.seek(off)
                comp = f.read(nsec * 512 - (off & 511))
                d = zlib.decompressobj(-12)
                data = d.decompress(comp, cs)
                if len(data) != cs: raise Refused('compressed-cluster-size')
                out.seek(vaddr); out.write(data)
                n_comp += 1
            else:
                if e2 & 1:                    # reads as zeroes
                    n_zero += 1
                    continue
                off = e2 & 0x00FFFFFFFFFFFE00
                if not off:
                    continue
                if off + cs > flen: raise Refused('cluster-past-end')
                f.seek(off); data = f.read(cs)
                out.seek(vaddr); out.write(data)
                n_data += 1
    out.close()
    print(f'size {size} cluster {cs} data {n_data} compressed {n_comp} zero {n_zero}')

if __name__ == '__main__':
    try:
        main(sys.argv[1], sys.argv[2])
    except Refused as e:
        print(f'refused\t{e}'); sys.exit(2)
