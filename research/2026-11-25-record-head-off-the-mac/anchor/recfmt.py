"""Just enough of #386's record format to read a chain and to rewrite it the way an attacker
running as you would: change a line, then compute every hash after it again, headers and head
included. Offsets are those of format.md."""
import hashlib, json, os, re, struct

HEADER = 128


def _crc32c_table():
    t = []
    for i in range(256):
        c = i
        for _ in range(8):
            c = (c >> 1) ^ 0x82F63B78 if c & 1 else c >> 1
        t.append(c)
    return t


_T = _crc32c_table()


def crc32c(b):
    c = 0xFFFFFFFF
    for x in b:
        c = _T[(c ^ x) & 0xFF] ^ (c >> 8)
    return c ^ 0xFFFFFFFF


def segments(d):
    return sorted(f for f in os.listdir(d) if re.fullmatch(r'\d{6}\.silorec', f))


def read(d):
    """[(segment file, [payload bytes, ...])]"""
    out = []
    for name in segments(d):
        b = open(os.path.join(d, name), 'rb').read()
        off, lines = HEADER, []
        while off + 8 <= len(b):
            n, _ = struct.unpack_from('<II', b, off)
            if off + 8 + n > len(b):
                break
            lines.append(b[off + 8:off + 8 + n])
            off += 8 + n
        out.append((name, lines))
    return out


def lines(d):
    return [l for _, ls in read(d) for l in ls]


def h(b):
    return hashlib.sha256(b).digest()


def write(d, segs, head=True):
    """Writes segments back, every prev, header and checksum computed again."""
    prev = b'\0' * 32
    seq = 1
    for name, ls in segs:
        p = os.path.join(d, name)
        hdr = bytearray(open(p, 'rb').read(HEADER))
        struct.pack_into('<Q', hdr, 32, seq)
        hdr[40:72] = prev
        hdr[96:128] = h(bytes(hdr[:96]))
        body = bytearray(hdr)
        for l in ls:
            l = re.sub(rb'"prev":"[0-9a-f]{64}"', b'"prev":"' + prev.hex().encode() + b'"', l, count=1)
            body += struct.pack('<II', len(l), crc32c(l)) + l
            prev = h(l)
            seq += 1
        open(p, 'wb').write(body)
    if head:
        last = json.loads([l for _, ls in segs for l in ls][-1])
        old = json.load(open(os.path.join(d, 'head')))
        old.update(seq=last['seq'], hash=prev.hex())
        open(os.path.join(d, 'head'), 'w').write(json.dumps(old, separators=(',', ':')) + '\n')


def rewrite(d, seq, old, new):
    """Changes text in line `seq` and chains every line after it again, head included."""
    segs = read(d)
    for _, ls in segs:
        for i, l in enumerate(ls):
            if json.loads(l)['seq'] == seq:
                assert old in l
                ls[i] = l.replace(old, new)
    write(d, segs)


def cut_tail(d, keep):
    """Keeps the first `keep` lines, cuts the rest at a line's end, and deletes the head."""
    segs, n, out = read(d), 0, []
    for name, ls in segs:
        if n >= keep:
            os.remove(os.path.join(d, name))
            continue
        out.append((name, ls[:keep - n]))
        n += len(ls)
    write(d, out, head=False)
    os.remove(os.path.join(d, 'head'))
