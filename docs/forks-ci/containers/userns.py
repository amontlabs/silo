#!/usr/bin/env python3
"""Run a command as root in a new user and mount namespace, like rootless podman's outer namespace.

    userns.py [--probe] -- CMD ARGS...

Container id 0 is the caller's own uid, ids 1..65535 are the caller's /etc/subuid and /etc/subgid range
(set by the setuid newuidmap and newgidmap). Mounts made by CMD (the overlay root, the container) stay in
its own mount namespace and vanish with it. crun then runs inside this namespace, so its own user namespace
maps container ids to ids of this one.
"""
import os
import signal
import subprocess
import sys

CLONE_NEWUSER = 0x10000000
CLONE_NEWNS = 0x00020000


def subid_range(path, user, ident):
    with open(path) as handle:
        for line in handle:
            name, start, count = line.strip().split(':')
            if name in (user, str(ident)) and int(count) >= 65535:
                return int(start)
    raise SystemExit(f'no sub-id range of 65535 for {user} in {path}')


def main():
    args = sys.argv[1:]
    if not args or '--' not in args:
        raise SystemExit(__doc__)
    command = args[args.index('--') + 1:]
    uid, gid = os.getuid(), os.getgid()
    user = os.environ.get('USER') or subprocess.check_output(['id', '-un'], text=True).strip()
    sub_uid = subid_range('/etc/subuid', user, uid)
    sub_gid = subid_range('/etc/subgid', user, gid)
    ready_r, ready_w = os.pipe()
    go_r, go_w = os.pipe()
    child = os.fork()
    if child == 0:
        os.close(ready_r)
        os.close(go_w)
        os.unshare(CLONE_NEWUSER | CLONE_NEWNS)
        os.write(ready_w, b'1')
        os.read(go_r, 1)
        os.close(ready_w)
        os.close(go_r)
        os.execvp(command[0], command)
    os.close(ready_w)
    os.close(go_r)
    os.read(ready_r, 1)
    subprocess.check_call(['newuidmap', str(child), '0', str(uid), '1', '1', str(sub_uid), '65535'])
    subprocess.check_call(['newgidmap', str(child), '0', str(gid), '1', '1', str(sub_gid), '65535'])
    os.write(go_w, b'1')
    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(sig, lambda number, _frame: os.kill(child, number))
    while True:
        try:
            _, status = os.waitpid(child, 0)
            break
        except InterruptedError:
            continue
    sys.exit(os.waitstatus_to_exitcode(status))


if __name__ == '__main__':
    main()
