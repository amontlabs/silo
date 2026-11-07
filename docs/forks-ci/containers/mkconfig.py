#!/usr/bin/env python3
"""Write an OCI config.json for crun: the hardening baseline of docs/forks-ci/evidence/c1-containers.

    mkconfig.py OUT.json --name N --rootfs DIR --cgroup /silo-c1/N --mode MODE [--uid U --gid G]
        [--caps none|relaxed] [--net IDX] [--connect PORTS] [--bind PORTS] [--env K=V ...]
        [--memory BYTES] [--cpus N] [--pids N]

crun runs inside the outer user namespace of userns.py, so the ids below are that namespace's: container
0..65535 maps to 0..65535 there (the caller's uid, then its sub-id range).
"""
import argparse
import json

BASELINE_CAPS = []
# Not in the baseline: what apt and dpkg need as container root. Used only where the run says so.
RELAXED_CAPS = ['CAP_CHOWN', 'CAP_DAC_OVERRIDE', 'CAP_FOWNER', 'CAP_FSETID', 'CAP_KILL', 'CAP_SETGID', 'CAP_SETUID']

CLONE_NEWUSER = 0x10000000
DENIED = ['io_uring_setup', 'io_uring_enter', 'io_uring_register', 'keyctl', 'add_key', 'request_key', 'bpf',
          'perf_event_open', 'userfaultfd']


def newuser_rule(names, index):
    return {'names': names, 'action': 'SCMP_ACT_ERRNO', 'errnoRet': 1,
            'args': [{'index': index, 'value': CLONE_NEWUSER, 'valueTwo': CLONE_NEWUSER, 'op': 'SCMP_CMP_MASKED_EQ'}]}


def seccomp():
    return {
        'defaultAction': 'SCMP_ACT_ALLOW',
        'architectures': ['SCMP_ARCH_X86_64', 'SCMP_ARCH_X86', 'SCMP_ARCH_X32'],
        'syscalls': [
            {'names': DENIED, 'action': 'SCMP_ACT_ERRNO', 'errnoRet': 1},
            newuser_rule(['clone', 'unshare'], 0),
            newuser_rule(['setns'], 1),
            # clone3 takes its flags in a struct seccomp cannot read; ENOSYS makes libc fall back to clone.
            {'names': ['clone3'], 'action': 'SCMP_ACT_ERRNO', 'errnoRet': 38},
        ],
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument('out')
    p.add_argument('--name', required=True)
    p.add_argument('--rootfs', required=True)
    p.add_argument('--cgroup', required=True)
    p.add_argument('--mode', required=True)
    p.add_argument('--uid', type=int, default=0)
    p.add_argument('--gid', type=int, default=0)
    p.add_argument('--home', default=None)
    p.add_argument('--caps', choices=['none', 'relaxed'], default='none')
    p.add_argument('--net', type=int, default=0)
    p.add_argument('--connect', default='')
    p.add_argument('--bind', default='')
    p.add_argument('--env', action='append', default=[])
    p.add_argument('--memory', type=int, default=4 << 30)
    p.add_argument('--cpus', type=float, default=2)
    p.add_argument('--pids', type=int, default=4096)
    a = p.parse_args()

    caps = RELAXED_CAPS if a.caps == 'relaxed' and a.uid == 0 else BASELINE_CAPS
    home = a.home or ('/root' if a.uid == 0 else '/home/ubuntu')
    env = ['PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin', f'HOME={home}', 'TERM=xterm',
           'LANG=C.UTF-8', 'no_proxy=127.0.0.1,localhost', f'C1_NAME={a.name}']
    if a.net:
        proxy = f'http://10.200.{a.net}.1:3128'
        env += ['C1_NET=1', f'C1_HOSTIP=10.200.{a.net}.1', f'http_proxy={proxy}', f'https_proxy={proxy}',
                f'HTTP_PROXY={proxy}', f'HTTPS_PROXY={proxy}']
    env += a.env
    # silo-init applies Landlock to itself and so to everything it starts.
    args = ['/silo/init', f'--connect={a.connect}', f'--bind={a.bind}', '--', '/bin/bash', '/silo/entry.sh', a.mode]
    config = {
        'ociVersion': '1.0.2-dev',
        'process': {
            'terminal': False,
            'user': {'uid': a.uid, 'gid': a.gid},
            'args': args,
            'env': env,
            'cwd': '/',
            'capabilities': {k: caps for k in ('bounding', 'effective', 'inheritable', 'permitted', 'ambient')},
            'rlimits': [{'type': 'RLIMIT_NOFILE', 'hard': 1048576, 'soft': 1048576}],
            'noNewPrivileges': True,
        },
        'root': {'path': a.rootfs, 'readonly': False},
        'hostname': a.name,
        'mounts': [
            {'destination': '/proc', 'type': 'proc', 'source': 'proc', 'options': ['nosuid', 'noexec', 'nodev']},
            {'destination': '/dev', 'type': 'tmpfs', 'source': 'tmpfs',
             'options': ['nosuid', 'strictatime', 'mode=755', 'size=65536k']},
            {'destination': '/dev/pts', 'type': 'devpts', 'source': 'devpts',
             'options': ['nosuid', 'noexec', 'newinstance', 'ptmxmode=0666', 'mode=0620', 'gid=5']},
            {'destination': '/dev/shm', 'type': 'tmpfs', 'source': 'shm',
             'options': ['nosuid', 'noexec', 'nodev', 'mode=1777', 'size=1073741824']},
            {'destination': '/dev/mqueue', 'type': 'mqueue', 'source': 'mqueue', 'options': ['nosuid', 'noexec', 'nodev']},
            {'destination': '/sys', 'type': 'sysfs', 'source': 'sysfs', 'options': ['nosuid', 'noexec', 'nodev', 'ro']},
            {'destination': '/sys/fs/cgroup', 'type': 'cgroup', 'source': 'cgroup',
             'options': ['nosuid', 'noexec', 'nodev', 'relatime', 'ro']},
        ],
        'linux': {
            'namespaces': [{'type': t} for t in ('pid', 'network', 'ipc', 'uts', 'mount', 'user', 'cgroup')],
            'uidMappings': [{'containerID': 0, 'hostID': 0, 'size': 65536}],
            'gidMappings': [{'containerID': 0, 'hostID': 0, 'size': 65536}],
            'cgroupsPath': a.cgroup,
            'resources': {'memory': {'limit': a.memory},
                          'cpu': {'quota': int(a.cpus * 100000), 'period': 100000},
                          'pids': {'limit': a.pids}},
            'seccomp': seccomp(),
            'maskedPaths': ['/proc/acpi', '/proc/asound', '/proc/kcore', '/proc/keys', '/proc/latency_stats',
                            '/proc/timer_list', '/proc/timer_stats', '/proc/sched_debug', '/proc/scsi',
                            '/sys/firmware', '/sys/devices/virtual/powercap'],
            'readonlyPaths': ['/proc/bus', '/proc/fs', '/proc/irq', '/proc/sys', '/proc/sysrq-trigger'],
        },
    }
    with open(a.out, 'w') as handle:
        json.dump(config, handle, indent=2)


if __name__ == '__main__':
    main()
