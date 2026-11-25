#!/usr/bin/env python3
"""Power-cut test of the recorder's writer (#386), simulated on Linux.

A process killed at any point keeps everything it handed to the kernel. A power cut does not: it
keeps only what the disk had made durable, in whatever order the disk wrote it. This script traces
every call the writer makes to the disk (strace), then builds the states a power cut can leave
under four models of the disk, starts the writer on each and checks:

  1. the writer starts;
  2. `silo verify` passes after the start;
  3. every line acknowledged before the cut is there.

Usage: powercut.py <silorec> <scratch> [samples-per-point] [seed]

Each model runs with three rules for what the writer's start cuts from the last segment:
  as #386            only a last frame cut short, or one whose end is the end of the file;
  cut_any_bad_frame  the first bad frame wherever it is, as a database's write-ahead log does in
                     point-in-time recovery (#386 has it as a planted bug);
  after_head         the first bad frame, or a line that does not follow, after the line the
                     head on disk names; before it, a bad frame is damage, as in #386.
Whatever is cut is kept beside the segment, and a fact names it (#386).

The models (what a call promises against a power cut):

  linux   Linux, ext4 ordered: fdatasync and fsync make the file's data, or the folder's entries,
          durable before they return. Metadata reaches the disk in the order it was made (the
          journal). An append's new size is never durable before its data.
  mac-fsync
          A Mac with plain fsync(2), which is what Zig's std.posix.fsync, fdatasync and File.sync
          call on macOS. Apple's man page: fsync moves the data to the drive, which "may not
          physically write the data ... for quite some time and it may be written in an
          out-of-order sequence". So no call makes anything durable: any subset of the writes may
          be on disk, and a size may be durable with its data not, which reads as zeros. Metadata
          is still applied in order (APFS commits whole transactions).
  mac-full
          A Mac where every fsync and fdatasync of the writer is F_FULLFSYNC: the drive's whole
          cache is flushed, so everything issued before it is durable.
  mac-barrier
          A Mac where the flush before an acknowledgement is F_FULLFSYNC and every other flush is
          F_BARRIERFSYNC: writes before a barrier reach the disk before writes after it, with no
          promise that either did.
"""
import json, os, random, re, shutil, subprocess, sys

SECTOR = 512


def unhex(s):
    return bytes(int(h, 16) for h in re.findall(r'\\x([0-9a-f]{2})', s))


def trace(silorec, scratch):
    rec = os.path.join(scratch, 'traced')
    out = os.path.join(scratch, 'trace.txt')
    shutil.rmtree(rec, ignore_errors=True)
    subprocess.run(['strace', '-y', '-xx', '-s', '100000000', '-o', out, '-e',
                    'trace=openat,write,pwrite64,fsync,fdatasync,renameat,renameat2,unlinkat,ftruncate,mkdirat',
                    silorec, 'workload', rec, '4096'], check=True, stdout=subprocess.DEVNULL)
    root = os.path.realpath(rec).encode()
    ops = []  # (kind, ...) in issue order
    inode_of = {}  # path -> inode id, as of the trace
    next_ino = [0]
    fd_ino = {}

    def rel(p):
        if p == root:
            return b''
        if p.startswith(root + b'/'):
            return p[len(root) + 1:]
        return None

    def anno(tok):
        m = re.match(r'(-?\d+|AT_FDCWD)<((?:\\x[0-9a-f]{2})*)>', tok)
        return unhex(m.group(2)) if m else None

    for line in open(out, encoding='ascii'):
        m = re.match(r'(\w+)\((.*)\) += (.*)$', line.strip())
        if not m:
            continue
        call, args, ret = m.groups()
        a = [x.strip() for x in re.split(r', (?=(?:[^"]*"[^"]*")*[^"]*$)', args)]
        if call == 'write' and a[0].startswith('1<'):
            ops.append(('ack', int(unhex(a[1]).strip())))
        elif call == 'mkdirat':
            p = rel(anno(a[0]) + b'/' + unhex(a[1]))
            if p is not None:
                ops.append(('mkdir', p))
        elif call == 'openat':
            full = anno(ret)
            p = rel(full) if full else None
            if p is None:
                continue
            if 'O_DIRECTORY' in a[2]:
                continue
            if 'O_CREAT' in a[2]:
                next_ino[0] += 1
                ino = next_ino[0]
                inode_of[p] = ino
                ops.append(('create', p, ino))
            fd_ino[int(ret.split('<')[0])] = inode_of[p]
        elif call == 'pwrite64':
            fd = int(a[0].split('<')[0])
            data, off = unhex(a[1]), int(a[3])
            ops.append(('write', fd_ino[fd], off, data))
        elif call in ('fsync', 'fdatasync'):
            p = rel(anno(a[0]))
            if p is None:
                continue
            fd = int(a[0].split('<')[0])
            if fd in fd_ino and inode_of.get(p) == fd_ino[fd]:
                ops.append(('sync-file', fd_ino[fd]))
            else:
                ops.append(('sync-dir', p))
        elif call == 'renameat':
            src = rel(anno(a[0]) + b'/' + unhex(a[1]))
            dst = rel(anno(a[2]) + b'/' + unhex(a[3]))
            inode_of[dst] = inode_of.pop(src)
            ops.append(('rename', src, dst))
        elif call == 'unlinkat':
            p = rel(anno(a[0]) + b'/' + unhex(a[1])) if anno(a[0]) else None
            if p:
                ops.append(('unlink', p))
    return ops


def units(ops):
    """Splits the trace into units that reach the disk whole: a metadata change, or one sector of
    one write. Each unit keeps its issue index."""
    us = []
    for i, op in enumerate(ops):
        k = op[0]
        if k == 'write':
            _, ino, off, data = op
            end = off + len(data)
            us.append(dict(i=i, meta=True, kind='size', ino=ino, size=end))
            s = off
            while s < end:
                e = min(end, (s // SECTOR + 1) * SECTOR)
                us.append(dict(i=i, meta=False, kind='data', ino=ino, off=s, data=data[s - off:e - off]))
                s = e
        elif k in ('mkdir', 'create', 'rename', 'unlink'):
            us.append(dict(i=i, meta=True, kind=k, op=op))
    return us


def dir_of(p):
    return p.rsplit(b'/', 1)[0] if b'/' in p else b''


def crash_state(ops, us, k, model, rng):
    """The units on disk after a power cut that falls just before op k."""
    issued = [u for u in us if u['i'] < k]
    syncs = [(i, op) for i, op in enumerate(ops[:k]) if op[0] in ('sync-file', 'sync-dir')]
    # Which units each model promises are on disk.
    must = set()
    if model == 'linux':
        for si, op in syncs:
            for n, u in enumerate(issued):
                if u['i'] >= si:
                    break
                if op[0] == 'sync-file' and u.get('ino') == op[1]:
                    must.add(n)
                if op[0] == 'sync-dir' and u['kind'] in ('create', 'rename', 'unlink', 'mkdir') and \
                        any(dir_of(p) == op[1] for p in u['op'][1:] if isinstance(p, bytes)):
                    must.add(n)
    elif model == 'mac-full':
        last = syncs[-1][0] if syncs else -1
        must = {n for n, u in enumerate(issued) if u['i'] < last}
    elif model == 'mac-barrier':
        full = [si for si, op in syncs if is_ack_flush(ops, si)]
        last = full[-1] if full else -1
        must = {n for n, u in enumerate(issued) if u['i'] < last}
    # Metadata is applied in order: a prefix of the metadata units, at least up to the last one
    # that must be there.
    metas = [n for n, u in enumerate(issued) if u['meta'] and u['kind'] != 'size']
    lo = max([metas.index(n) + 1 for n in metas if n in must], default=0)
    cut = rng.randint(lo, len(metas))
    keep = set(metas[:cut]) | must
    if model == 'mac-barrier':
        # Epochs between barriers: all of the epochs before a random one, some of that one.
        bars = [si for si, _ in syncs if si > (full[-1] if full else -1)]
        edges = [full[-1] if full else -1] + bars + [k]
        j = rng.randrange(len(edges) - 1)
        lo_i, hi_i = edges[j], edges[j + 1]
        keep = set(must)
        for n, u in enumerate(issued):
            if u['i'] < lo_i:
                keep.add(n)
            elif u['i'] < hi_i and rng.random() < 0.5:
                keep.add(n)
        # Metadata within the kept set stays a prefix.
        km = [n for n in metas if n in keep]
        if km:
            last_meta = max(km)
            keep |= {n for n in metas if n <= last_meta}
    else:
        for n, u in enumerate(issued):
            if not u['meta'] and n not in keep and rng.random() < 0.5:
                keep.add(n)
    for n, u in enumerate(issued):
        if u['kind'] == 'size' and n not in keep:
            if model == 'linux':
                # Ordered mode: the size is durable once all its data is.
                datas = [m for m, v in enumerate(issued) if v['i'] == u['i'] and not v['meta']]
                if all(m in keep for m in datas):
                    keep.add(n)
            elif rng.random() < 0.5:
                keep.add(n)
    return [u for n, u in enumerate(issued) if n in keep]


def is_ack_flush(ops, si):
    for op in ops[si + 1:]:
        if op[0] == 'ack':
            return True
        if op[0] != 'sync-file':
            return False
    return False


def materialize(state, dest):
    shutil.rmtree(dest, ignore_errors=True)
    os.makedirs(dest)
    names = {}  # path -> inode
    dirs = {b''}
    size, data = {}, {}
    for u in state:
        k = u['kind']
        if k == 'mkdir':
            dirs.add(u['op'][1])
        elif k == 'create':
            names[u['op'][1]] = u['op'][2]
            size.setdefault(u['op'][2], 0)
        elif k == 'rename':
            _, s, d = u['op']
            if s in names:
                names[d] = names.pop(s)
        elif k == 'unlink':
            names.pop(u['op'][1], None)
        elif k == 'size':
            size[u['ino']] = max(size.get(u['ino'], 0), u['size'])
        elif k == 'data':
            data.setdefault(u['ino'], []).append(u)
    for d in sorted(dirs):
        os.makedirs(os.path.join(dest, d.decode()), exist_ok=True)
    for p, ino in names.items():
        if dir_of(p) not in dirs:
            continue
        buf = bytearray(size.get(ino, 0))
        for u in data.get(ino, []):
            end = min(len(buf), u['off'] + len(u['data']))
            if u['off'] < end:
                buf[u['off']:end] = u['data'][:end - u['off']]
        with open(os.path.join(dest, p.decode()), 'wb') as f:
            f.write(buf)


def check(silorec, dest, acked, start=None):
    r = subprocess.run([silorec, 'start', dest, '4096'] + ([start] if start else []), capture_output=True, text=True)
    if r.returncode != 0:
        return 'refused-to-start', r.stdout.strip() or r.stderr.strip()[-200:]
    r = subprocess.run([silorec, 'verify', dest, 'c1'], capture_output=True, text=True)
    if r.returncode != 0:
        return 'verify-fails', r.stdout.strip()
    r = subprocess.run([silorec, 'print', dest], capture_output=True)
    seen = set()
    sources = ['engine', 'helper:c1', 'boundary']
    for l in r.stdout.splitlines():
        j = json.loads(l)
        s = j['source']
        if s.get('id') in sources:
            seen.add((sources.index(s['id']), s['n']))
    if acked is not None:
        for i in range(acked + 1):
            if (i % 3, i // 3 + 1) not in seen:
                return 'acked-line-lost', f'line {i} acknowledged, not on disk'
    return 'ok', ''


def main():
    silorec, scratch = os.path.abspath(sys.argv[1]), sys.argv[2]
    samples = int(sys.argv[3]) if len(sys.argv) > 3 else 10
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    os.makedirs(scratch, exist_ok=True)
    ops = trace(silorec, scratch)
    us = units(ops)
    rng = random.Random(seed)
    dest = os.path.join(scratch, 'state')
    print(json.dumps({'calls': len(ops), 'units': len(us), 'acks': sum(1 for o in ops if o[0] == 'ack'),
                      'sector': SECTOR, 'samples_per_point': samples, 'seed': seed}))
    configs = [(m, r) for r in (None, 'cut_any_bad_frame', 'after_head')
               for m in ('linux', 'mac-fsync', 'mac-full', 'mac-barrier')]
    for model, start in configs:
        tally, first = {}, {}
        lost_units = 0
        points = 0
        for k in range(1, len(ops) + 1):
            if ops[k - 1][0] == 'ack':
                continue
            points += 1
            acked = None
            for op in ops[:k]:
                if op[0] == 'ack':
                    acked = op[1]
            for _ in range(samples):
                st = crash_state(ops, us, k, model, rng)
                if len(st) < sum(1 for u in us if u['i'] < k):
                    lost_units += 1
                materialize(st, dest)
                outcome, why = check(silorec, dest, acked, start)
                tally[outcome] = tally.get(outcome, 0) + 1
                if outcome != 'ok' and outcome not in first:
                    first[outcome] = {'point': k, 'acked': acked, 'what': why}
        print(json.dumps({'model': model, 'start': start or 'as #386', 'points': points, 'states': points * samples,
                          'states_that_lost_a_write': lost_units,
                          'outcomes': tally, 'first': first}), flush=True)


if __name__ == '__main__':
    main()
