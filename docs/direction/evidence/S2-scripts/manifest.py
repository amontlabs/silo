import os,sys,hashlib,stat
root=sys.argv[1]
def x(p):
    try: return ';'.join(f'{k}={os.getxattr(p,k,follow_symlinks=False)!r}' for k in sorted(os.listxattr(p,follow_symlinks=False)))
    except OSError as e: return 'ERR'
out=[]
def rec(p):
    st=os.lstat(p); m=st.st_mode
    h=''
    if stat.S_ISREG(m) and (m&0o444 or os.geteuid()==0) and st.st_size<200_000_000:
        h=hashlib.md5(open(p,'rb').read()).hexdigest() if st.st_size<3_000_000 else 'big'
    link=os.readlink(p) if stat.S_ISLNK(m) else ''
    out.append(f'{os.path.relpath(p,root)}|{oct(m)}|{st.st_uid}:{st.st_gid}|{st.st_size}|{int(st.st_mtime)}|nl{st.st_nlink}|{h}|{link}|{x(p)}|blocks{st.st_blocks if stat.S_ISREG(m) and st.st_size>10_000_000 else ""}')
for d,ds,fs in os.walk(root):
    ds.sort()
    rec(d)
    for f in sorted(fs): rec(os.path.join(d,f))
print('\n'.join(out))
