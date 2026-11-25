#!/usr/bin/env python3
"""What an anchor costs: the size of one signed head, the size of a Git anchor after N heads
committed one by one (pushed every 60 commits), and the time `heads` takes to read them all back.

Usage: cost.py <scratch> <heads>"""
import json, os, shutil, subprocess, sys, time
import anchor

T, N = os.path.abspath(sys.argv[1]), int(sys.argv[2])
shutil.rmtree(T, ignore_errors=True)
os.makedirs(T)
key = anchor.new_key()
g = anchor.GitAnchor(os.path.join(T, 'a.git'), os.path.join(T, 'work'))
p = os.path.join(g.work, 'c1.heads')
for i in range(1, N + 1):
    hd = anchor.sign(key, {'v': 1, 'computer': 'c1', 'seq': i * 1000, 'hash': os.urandom(32).hex(),
                           'time': '2026-11-25T02:41:07.123456789Z'})
    with open(p, 'a') as f:
        f.write(json.dumps(hd, separators=(',', ':')) + '\n')
    anchor.run('git', '-C', g.work, 'add', 'c1.heads')
    anchor.run('git', '-C', g.work, 'commit', '-q', '-m', f'c1 {i}')
    if i % 60 == 0:
        anchor.git_push(g.work)
anchor.git_push(g.work)
anchor.run('git', '-C', g.bare, 'gc', '-q')
du = int(subprocess.run(['du', '-sb', g.bare], capture_output=True, text=True).stdout.split()[0])
t = time.time()
heads = g.heads('c1')
print(json.dumps({'heads': N, 'bytes_per_signed_head': len(json.dumps(hd, separators=(',', ':'))) + 1,
                  'git_anchor_bytes_after_gc': du, 'bytes_per_head_in_git': round(du / N),
                  'read_back_s': round(time.time() - t, 2), 'heads_read': len(heads)}))
