#!/usr/bin/env python3
"""Each change #386's `silo verify` cannot see, and what an anchor off the Mac says of it.

Usage: test_anchor.py <silorec> <scratch>   prints the table of results/anchor.md
"""
import json, os, shutil, subprocess, sys, threading

import anchor, recfmt

S, T = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
shutil.rmtree(T, ignore_errors=True)
os.makedirs(T)
base = os.path.join(T, 'base')
subprocess.run([S, 'workload', base, '4096'], check=True, capture_output=True)
LAST = len(recfmt.lines(base))

key, other = anchor.new_key(), anchor.new_key()
pub = key.public_key()


def verify(d):
    r = subprocess.run([S, 'verify', d, 'c1'], capture_output=True, text=True)
    return 'holds' if r.returncode == 0 else 'found: ' + r.stdout.strip().split(': ', 1)[-1]


def fresh(name):
    d = os.path.join(T, name)
    shutil.copytree(base, d)
    return d


def anchors(name, upto=LAST, every=4):
    """A Git anchor and a ledger that received the head after each flush, up to line `upto`."""
    g = anchor.GitAnchor(os.path.join(T, name + '.git'), os.path.join(T, name + '.work'))
    led = anchor.Ledger(os.path.join(T, name + '.ledger'), {'c1': pub})
    threading.Thread(target=led.serve_forever, daemon=True).start()
    la = anchor.LedgerAnchor(f'http://127.0.0.1:{led.server_address[1]}')
    for s in range(every, upto + 1, every):
        hd = anchor.sign(key, anchor.head_at(base, s))
        g.push(hd)
        la.push(hd)
    return g, la, led


rows = []


def row(case, d, g, la, note=''):
    ok_g, msg_g = anchor.check(d, g.heads('c1'), pub)
    ok_l, msg_l = anchor.check(d, la.heads('c1'), pub)
    rows.append((case, verify(d), 'holds' if ok_g else 'found', 'holds' if ok_l else 'found', msg_g if not ok_g else msg_l if not ok_l else msg_g, note))


# 1. Nothing changed.
d = fresh('untouched'); g, la, led = anchors('untouched')
row('Nothing changed', d, g, la)

# 2. The last lines cut at a line's end, head deleted (#386: not found).
d = fresh('cut'); g, la, led = anchors('cut')
recfmt.cut_tail(d, 30)
row('The last 18 lines cut at a line\'s end, head deleted', d, g, la)

# 3. A line changed, every line after it chained again, head rewritten (#386: not found).
d = fresh('rewrite'); g, la, led = anchors('rewrite')
recfmt.rewrite(d, 10, b'"pid":4009', b'"pid":4999')
row('Line 10 changed, every line after it chained again, head rewritten', d, g, la)

# 4. The same rewrite, and the attacker also rewrites the Git anchor's history.
d = fresh('rewrite-git'); g, la, led = anchors('rewrite-git')
recfmt.rewrite(d, 10, b'"pid":4009', b'"pid":4999')
w = g.work
forged = [anchor.sign(key, anchor.head_at(d, s)) for s in range(4, LAST + 1, 4)]
anchor.run('git', '-C', w, 'checkout', '-q', '--orphan', 'forged')
anchor.run('git', '-C', w, 'rm', '-q', '-rf', '--cached', '.')
open(os.path.join(w, 'c1.heads'), 'w').write(''.join(json.dumps(h, separators=(',', ':')) + '\n' for h in forged))
anchor.run('git', '-C', w, 'add', 'c1.heads')
anchor.run('git', '-C', w, 'commit', '-q', '-m', 'forged')
force = anchor.git_push(w, '--force')
delete = subprocess.run(['git', '-C', w, 'push', '-q', 'origin', ':main'], capture_output=True, text=True)
# A fast-forward commit that replaces the file's lines is accepted, and changes nothing for check.
anchor.run('git', '-C', w, 'checkout', '-q', 'main')
open(os.path.join(w, 'c1.heads'), 'w').write(''.join(json.dumps(h, separators=(',', ':')) + '\n' for h in forged))
anchor.run('git', '-C', w, 'commit', '-q', '-am', 'replace')
ff = anchor.git_push(w)
row('The same, and the Git anchor attacked: force push, branch deleted, file replaced in a new commit', d, g, la,
    f"force push: {'refused' if force[0] else 'accepted'}; delete: {'refused' if delete.returncode else 'accepted'}; new commit: {'refused' if ff[0] else 'accepted'}")

# 5. The same rewrite, and the attacker posts forged heads to the ledger.
d = fresh('rewrite-ledger'); g, la, led = anchors('rewrite-ledger')
recfmt.rewrite(d, 10, b'"pid":4009', b'"pid":4999')
own = la.push(anchor.sign(other, anchor.head_of(d)))
old = la.push(anchor.sign(key, anchor.head_at(d, 8)))
same = la.push(anchor.sign(key, anchor.head_of(d)))
row('The same, and forged heads posted to the ledger', d, g, la,
    f"signed with another key: {own[0]} {own[1]}; an older line: {old[0]} {old[1]}; signed with the Mac's key: {same[0]} {same[1]}")

# 6. Lines after the newest anchor cut: the window an anchor cannot cover.
d = fresh('window'); g, la, led = anchors('window', upto=40)
recfmt.cut_tail(d, 42)
row('Anchored up to line 40, then lines 43 to 48 cut and the head deleted', d, g, la,
    'the window: lines written since the last head sent')

# 7. Signed heads kept on the Mac only, then deleted with the rewrite.
d = fresh('local-signed')
local = [anchor.sign(key, anchor.head_at(base, s)) for s in range(4, LAST + 1, 4)]
recfmt.rewrite(d, 10, b'"pid":4009', b'"pid":4999')
ok_before, msg_before = anchor.check(d, local, pub)
local = []
ok_after, msg_after = anchor.check(d, local, pub)
rows.append(('Signed heads kept on the Mac, deleted with the rewrite', verify(d), '', '',
             msg_after, f"before they were deleted: {'holds' if ok_before else 'found'}"))

print('| Change | #386 `silo verify` | Git anchor | Ledger | What the anchor check says | Note |')
print('|---|---|---|---|---|---|')
for r in rows:
    print('| ' + ' | '.join(x.replace('|', '/') for x in r) + ' |')
