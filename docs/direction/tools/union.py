#!/usr/bin/env python3
"""Resolves conflict hunks by keeping both sides (ours, then theirs). Hunks whose lines hold
`assert_eq!` are left for a person and listed."""
import re, sys
for path in sys.argv[1:]:
    text = open(path).read()
    left = []
    def fix(m):
        ours, theirs = m.group(1), m.group(2)
        if 'assert' in ours or 'assert' in theirs:
            left.append((ours.strip(), theirs.strip()))
            return m.group(0)
        return ours + theirs
    out = re.sub(r'<<<<<<< [^\n]*\n(.*?)=======\n(.*?)>>>>>>> [^\n]*\n', fix, text, flags=re.S)
    open(path, 'w').write(out)
    print(path, 'left:', left)
