import os, sys, stat, socket, hashlib, random
random.seed(7)
home='/home/silo/data'; ws='/workspace/repo'
os.makedirs(home,exist_ok=True); os.makedirs(ws,exist_ok=True)
n=0
for d in range(500):
    p=f'{home}/d{d:03d}'; os.makedirs(p,exist_ok=True)
    for f in range(100):
        open(f'{p}/f{f:03d}.bin','wb').write(os.urandom(20000))
        n+=1
print('files',n)
for i in range(2000):
    p=f'{ws}/src{i%50}'; os.makedirs(p,exist_ok=True)
    open(f'{p}/m{i}.txt','w').write(f'workspace file {i}\n'*200)
open('/workspace/big.bin','wb').write(os.urandom(100_000_000))
# specials in home
h='/home/silo'
os.symlink('data/d000/f000.bin',f'{h}/rel-link'); os.symlink('/nonexistent/target',f'{h}/dangling')
os.link(f'{h}/data/d001/f001.bin',f'{h}/hardlink-a'); 
os.link(f'{h}/hardlink-a',f'{h}/data/d002/hardlink-b')
os.mkfifo(f'{h}/fifo'); s=socket.socket(socket.AF_UNIX); s.bind(f'{h}/sock'); s.close()
open(f'{h}/setuid.sh','w').write('#!/bin/sh\necho hi\n'); os.chmod(f'{h}/setuid.sh',0o4755)
open(f'{h}/mode000','w').write('x'); os.chmod(f'{h}/mode000',0)
open(f'{h}/root-owned','w').write('r'); os.chown(f'{h}/root-owned',0,0)
open(f'{h}/other-uid','w').write('o'); os.chown(f'{h}/other-uid',1234,4321)
open(f'{h}/xattr-file','w').write('x'); os.setxattr(f'{h}/xattr-file','user.note',b'kept?'); os.setxattr(f'{h}/xattr-file','user.empty',b'')
os.mkdir(f'{h}/dir700',0o700); os.chmod(f'{h}/dir700',0o700); os.setxattr(f'{h}/dir700','user.dirnote',b'dir')
with open(f'{h}/sparse','wb') as f: f.seek(50_000_000); f.write(b'end')
open(f'{h}/üñí çödé ✓.txt','w').write('unicode')
open(f'{h}/.claude.json','w').write('{"login":"canary"}')
os.makedirs(f'{h}/.config/agent',exist_ok=True); open(f'{h}/.config/agent/history','w').write('history '*100)
os.utime(f'{h}/rel-link',(1_000_000_000,1_000_000_000),follow_symlinks=False)
os.utime(f'{h}/data/d000/f000.bin',(1_000_000_000,1_000_000_000))
# workspace specials
open('/workspace/rootfile','w').write('root'); os.chown('/workspace/rootfile',0,0)
os.symlink('repo/src0','/workspace/linkdir'); os.chmod('/workspace/repo',0o2775)
open('/workspace/xw','w').write('x'); os.setxattr('/workspace/xw','user.k',b'v')
