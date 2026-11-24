"""Reading and answering a git receive-pack request (smart HTTP push).

Request body: pkt-lines "<old> <new> <ref>" (capabilities after NUL on the first one), a flush,
optional push-option pkt-lines and a flush (only if the client sent the push-options capability),
then a PACK (absent when every command is a delete). A large push first sends a probe body that
is only "0000".

Sources: https://git-scm.com/docs/gitprotocol-pack , https://git-scm.com/docs/gitprotocol-common ,
https://git-scm.com/docs/gitprotocol-capabilities , https://git-scm.com/docs/gitformat-pack
"""
import hashlib
import zlib

ZERO = "0" * 40


def pkt(data: bytes) -> bytes:
    return b"%04x" % (len(data) + 4) + data


class Push:
    def __init__(self):
        self.commands = []        # list of (old, new, ref)
        self.caps = []
        self.push_options = []
        self.pack_offset = None   # index of "PACK" in body, or None
        self.probe = False
        self.cmd_end = 0          # offset just after the command flush
        self.signed = False
        self.unreadable = None    # why Silo cannot read the commands (it then refuses)


def parse_request(body: bytes) -> Push:
    p = Push()
    if body == b"0000":
        p.probe = True
        return p
    i = 0
    first = True
    # commands
    while True:
        n = int(body[i:i + 4], 16)
        if n == 0:
            i += 4
            break
        line = body[i + 4:i + n]
        i += n
        if line.startswith(b"shallow "):
            continue
        if line.startswith(b"push-cert"):
            # signed push: the commands sit inside the certificate, which ends with push-cert-end
            p.signed = True
            if b"\0" in line:
                p.caps = line.split(b"\0", 1)[1].decode().strip().split()
            while True:
                n = int(body[i:i + 4], 16)
                if n == 0:
                    p.unreadable = "push certificate without an end"
                    return p
                l = body[i + 4:i + n].decode("utf-8", "replace").rstrip("\n")
                i += n
                if l == "push-cert-end":
                    break
                f = l.split(" ")
                if len(f) == 3 and all(len(x) == 40 and all(c in "0123456789abcdef" for c in x) for x in f[:2]):
                    p.commands.append(tuple(f))
            if body[i:i + 4] != b"0000":
                p.unreadable = "unexpected data after the push certificate"
                return p
            i += 4
            break
        if first:
            if b"\0" in line:
                line, caps = line.split(b"\0", 1)
                p.caps = caps.decode().strip().split()
            first = False
        old, new, ref = line.decode().rstrip("\n").split(" ", 2)
        p.commands.append((old, new, ref))
    p.cmd_end = i
    if "push-options" in p.caps:
        while True:
            n = int(body[i:i + 4], 16)
            if n == 0:
                i += 4
                break
            p.push_options.append(body[i + 4:i + n].decode().rstrip("\n"))
            i += n
    if body[i:i + 4] == b"PACK":
        p.pack_offset = i
    return p


def rebuild_request(body: bytes, push: Push, keep_refs) -> bytes:
    """The same request with only the commands for keep_refs. Capabilities move to the first
    kept command. Push options and pack stay as they were."""
    kept = [c for c in push.commands if c[2] in keep_refs]
    out = b""
    for k, (old, new, ref) in enumerate(kept):
        line = f"{old} {new} {ref}".encode()
        if k == 0:
            line += b"\0" + " ".join(push.caps).encode()
        out += pkt(line + b"\n")
    out += b"0000"
    return out + body[push.cmd_end:]


# ---------------------------------------------------------------- pack reading

TYPES = {1: b"commit", 2: b"tree", 3: b"blob", 4: b"tag"}


def _inflate(buf, pos):
    d = zlib.decompressobj()
    out = []
    step = 65536
    while not d.eof:
        chunk = buf[pos:pos + step]
        if not chunk:
            raise ValueError("truncated pack")
        out.append(d.decompress(chunk))
        pos += len(chunk)
    pos -= len(d.unused_data)
    return b"".join(out), pos


def _apply_delta(base, delta):
    def varint(i):
        r = s = 0
        while True:
            c = delta[i]; i += 1
            r |= (c & 0x7f) << s; s += 7
            if not c & 0x80:
                return r, i
    _, i = varint(0)
    size, i = varint(i)
    out = bytearray()
    while i < len(delta):
        op = delta[i]; i += 1
        if op & 0x80:
            off = ln = 0
            for b in range(4):
                if op & (1 << b):
                    off |= delta[i] << (8 * b); i += 1
            for b in range(3):
                if op & (0x10 << b):
                    ln |= delta[i] << (8 * b); i += 1
            ln = ln or 0x10000
            out += base[off:off + ln]
        else:
            out += delta[i:i + op]; i += op
    assert len(out) == size
    return bytes(out)


def read_pack(buf: bytes, start: int, commits_only=False):
    """Returns (commits: {sha: [parents]}, stats). Resolves OFS_DELTA and in-pack REF_DELTA.
    A REF_DELTA whose base is not in the pack (thin pack) stays unresolved.
    commits_only: stop at the first tree or blob. pack-objects writes commits first, so the
    walk needs only the start of the pack (measured in packorder.py)."""
    view = memoryview(buf)
    assert bytes(view[start:start + 4]) == b"PACK"
    n = int.from_bytes(view[start + 8:start + 12], "big")
    pos = start + 12
    raw = {}       # offset -> (type, data or (base_ref, delta))
    for _ in range(n):
        off = pos
        c = buf[pos]; pos += 1
        typ = (c >> 4) & 7
        if commits_only and typ in (2, 3):
            n = len(raw)
            break
        while c & 0x80:
            c = buf[pos]; pos += 1
        if typ == 6:
            c = buf[pos]; pos += 1
            rel = c & 0x7f
            while c & 0x80:
                c = buf[pos]; pos += 1
                rel = ((rel + 1) << 7) | (c & 0x7f)
            data, pos = _inflate(view, pos)
            raw[off] = (6, (off - rel, data))
        elif typ == 7:
            base = bytes(view[pos:pos + 20]).hex(); pos += 20
            data, pos = _inflate(view, pos)
            raw[off] = (7, (base, data))
        else:
            data, pos = _inflate(view, pos)
            raw[off] = (typ, data)
    resolved = {}  # offset -> (typename, data)
    by_sha = {}
    def resolve(off, depth=0):
        if off in resolved:
            return resolved[off]
        typ, v = raw[off]
        if typ in TYPES:
            r = (TYPES[typ], v)
        elif typ == 6:
            b = resolve(v[0], depth + 1)
            r = None if b is None else (b[0], _apply_delta(b[1], v[1]))
        else:
            boff = by_sha.get(v[0])
            if boff is None:
                return None
            b = resolve(boff, depth + 1)
            r = None if b is None else (b[0], _apply_delta(b[1], v[1]))
        if r is not None:
            resolved[off] = r
            sha = hashlib.sha1(r[0] + b" %d\0" % len(r[1]) + r[1]).hexdigest()
            by_sha[sha] = off
        return r
    # two passes so in-pack REF_DELTA bases found later get resolved
    for _ in range(2):
        for off in raw:
            resolve(off)
    commits = {}
    for sha, off in by_sha.items():
        t, data = resolved[off]
        if t != b"commit":
            continue
        parents = []
        for line in data.split(b"\n"):
            if not line:
                break
            if line.startswith(b"parent "):
                parents.append(line[7:47].decode())
        commits[sha] = parents
    stats = {
        "objects": n,
        "commits": len(commits),
        "deltified_commits": sum(1 for o, (t, _) in resolved.items()
                                 if t == b"commit" and raw[o][0] in (6, 7)),
        "unresolved": sum(1 for o in raw if o not in resolved),
        "pack_bytes": pos - start + 20,
    }
    return commits, stats


def walk(old, new, commits):
    """Ancestry from the pack alone. Returns ("ff", set()), ("force", set()) or
    ("unknown", boundary): boundary = parents outside the pack (commits the server has)."""
    if new not in commits:
        return "unknown", {new}
    seen, todo, boundary = {new}, [new], set()
    while todo:
        c = todo.pop()
        for p in commits[c]:
            if p == old:
                return "ff", set()
            if p in seen:
                continue
            seen.add(p)
            if p in commits:
                todo.append(p)
            else:
                boundary.add(p)
    if not boundary:
        return "force", set()   # history of new is entirely new and never meets old
    return "unknown", boundary


# ---------------------------------------------------------------- answers

def _wrap(inner: bytes, sideband: bool, message: str = None) -> bytes:
    if not sideband:
        return inner
    out = b""
    if message:
        out += pkt(b"\x02" + message.encode() + b"\n")
    for k in range(0, len(inner), 65515):
        out += pkt(b"\x01" + inner[k:k + 65515])
    return out + b"0000"


def refusal(push: Push, reasons: dict, default="refused") -> bytes:
    """A 200 receive-pack answer refusing every ref: report-status "ng <ref> <reason>"."""
    sb = "side-band-64k" in push.caps or "side-band" in push.caps
    inner = pkt(b"unpack ok\n")
    for _, _, ref in push.commands:
        inner += pkt(f"ng {ref} {reasons.get(ref, default)}\n".encode())
    inner += b"0000"
    msg = next(iter(reasons.values()), default)
    return _wrap(inner, sb, msg)


def demux(resp: bytes, sideband: bool):
    """Split a receive-pack answer into (report-status bytes, list of band-2 messages)."""
    if not sideband:
        return resp, []
    inner, msgs, i = b"", [], 0
    while i + 4 <= len(resp):
        n = int(resp[i:i + 4], 16)
        if n == 0:
            i += 4
            continue
        band, data = resp[i + 4], resp[i + 5:i + n]
        if band == 1:
            inner += data
        else:
            msgs.append(bytes(data))
        i += n
    return inner, msgs


def splice(resp: bytes, push: Push, refused: dict) -> bytes:
    """Add "ng" lines for refused refs into the server's own answer for the kept refs."""
    sb = "side-band-64k" in push.caps or "side-band" in push.caps
    inner, msgs = demux(resp, sb)
    lines, i = [], 0
    while i + 4 <= len(inner):
        n = int(inner[i:i + 4], 16)
        if n == 0:
            break
        lines.append(inner[i + 4:i + n]); i += n
    out = pkt(lines[0]) if lines else pkt(b"unpack ok\n")
    for ref, why in refused.items():
        out += pkt(f"ng {ref} {why}\n".encode())
    for l in lines[1:]:
        out += pkt(l)
    out += b"0000"
    if not sb:
        return out
    pre = b"".join(pkt(b"\x02" + m) for m in msgs)
    note = pkt(b"\x02" + (next(iter(refused.values())) + "\n").encode()) if refused else b""
    return pre + note + _wrap(out, True)
