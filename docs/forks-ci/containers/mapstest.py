import os, sys
def attempt(label, setgroups, gid_map):
    r, w = os.pipe(); r2, w2 = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.unshare(0x10000000)
        os.write(w, b'1'); os.read(r2, 1); os._exit(0)
    os.read(r, 1)
    res = []
    def put(path, data):
        try:
            with open(f'/proc/{pid}/{path}', 'w') as f: f.write(data)
            res.append(f'{path}=ok')
        except OSError as e:
            res.append(f'{path}={e.strerror}')
    put('uid_map', '0 0 65536')
    if setgroups: put('setgroups', setgroups)
    put('gid_map', gid_map)
    print(f'MAPS {label}: ' + ' '.join(res))
    os.write(w2, b'1'); os.waitpid(pid, 0)
print('MAPS euid', os.geteuid(), 'egid', os.getegid(), 'groups', os.getgroups())
attempt('plain', None, '0 0 65536')
attempt('setgroups-deny', 'deny', '0 0 65536')
attempt('one-id', None, '0 0 1')
