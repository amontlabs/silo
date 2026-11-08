#!/usr/bin/env python3
"""A file manifest of what a container changed: the entries of its overlay upper layer.

    sudo diff_upper.py UPPER [BASE_UPPER] --lower DIR [--lower DIR ...] [--tsv FILE]

Without BASE_UPPER the manifest is the upper layer against the lower layers (the image): A added, M modified,
D deleted (an overlay whiteout). With BASE_UPPER, the upper layer of a fork against the upper layer it was
copied from, which is how a fork's own changes would be listed. A whiteout is a 0/0 character device. Directories only copied up as
the parent of a change are not listed. Prints the counts and the time it took.
"""
import argparse
import hashlib
import os
import stat
import sys
import time

p = argparse.ArgumentParser()
p.add_argument('upper')
p.add_argument('base_upper', nargs='?')
p.add_argument('--lower', action='append', default=[])
p.add_argument('--tsv')
a = p.parse_args()


def is_whiteout(path):
    """An overlay whiteout is a 0/0 character device (also when the overlay is mounted in a user namespace)."""
    try:
        st = os.lstat(path)
    except OSError:
        return False
    return stat.S_ISCHR(st.st_mode) and st.st_rdev == 0


def digest(path):
    h = hashlib.sha256()
    with open(path, 'rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def same(left, right):
    sl, sr = os.lstat(left), os.lstat(right)
    if stat.S_IFMT(sl.st_mode) != stat.S_IFMT(sr.st_mode):
        return False
    if stat.S_ISLNK(sl.st_mode):
        return os.readlink(left) == os.readlink(right)
    if stat.S_ISREG(sl.st_mode):
        return sl.st_size == sr.st_size and digest(left) == digest(right)
    return True


def find_in(roots, rel):
    for root in roots:
        candidate = os.path.join(root, rel)
        if os.path.lexists(candidate):
            return candidate
    return None


start = time.monotonic()
rows = []
for directory, dirs, files in os.walk(a.upper):
    rel_dir = os.path.relpath(directory, a.upper)
    for name in sorted(dirs + files):
        full = os.path.join(directory, name)
        rel = os.path.normpath(os.path.join(rel_dir, name))
        st = os.lstat(full)
        whiteout = is_whiteout(full)
        previous = find_in([a.base_upper] if a.base_upper else a.lower, rel)
        previous_whiteout = previous is not None and is_whiteout(previous)
        if whiteout:
            if not previous_whiteout:
                rows.append(('D', rel, 0))
            continue
        if previous_whiteout:
            previous = None
        if stat.S_ISDIR(st.st_mode):
            if previous is None:
                rows.append(('A', rel + '/', 0))
            continue
        if previous is None:
            rows.append(('A', rel, st.st_size))
        elif not same(full, previous):
            rows.append(('M', rel, st.st_size))
if a.base_upper:
    # A fork that removes a file only its base's upper layer holds leaves no whiteout (nothing below hides):
    # the file is simply missing from its upper layer. Compare the trees both ways.
    for directory, dirs, files in os.walk(a.base_upper):
        rel_dir = os.path.relpath(directory, a.base_upper)
        for name in sorted(files):
            rel = os.path.normpath(os.path.join(rel_dir, name))
            base_entry = os.path.join(directory, name)
            if is_whiteout(base_entry):
                continue
            if not os.path.lexists(os.path.join(a.upper, rel)):
                rows.append(('D', rel, 0))
elapsed = time.monotonic() - start
counts = {k: sum(1 for r in rows if r[0] == k) for k in 'AMD'}
if a.tsv:
    with open(a.tsv, 'w') as handle:
        for kind, rel, size in sorted(rows, key=lambda r: r[1]):
            handle.write(f'{kind}\t{rel}\t{size}\n')
print(f'MANIFEST entries={len(rows)} added={counts["A"]} modified={counts["M"]} deleted={counts["D"]} seconds={elapsed:.3f}')
