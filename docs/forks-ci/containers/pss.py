#!/usr/bin/env python3
"""PSS (MiB) of every process in a container's cgroup, plus the cgroup's own memory figures and limits.

    sudo pss.py CGROUP_DIR

Reads /proc/PID/smaps_rollup like docs/forks-ci/checks/measure_footprint.py, so the number is comparable with
the microVM figures of the F1 evidence. Prints PSS_MIB, NPROC, CGROUP_MEMORY_CURRENT_MIB, and the limits.
"""
import os
import sys

root = sys.argv[1]
pids = []
for directory, _dirs, files in os.walk(root):
    if 'cgroup.procs' in files:
        with open(os.path.join(directory, 'cgroup.procs')) as handle:
            pids += [int(x) for x in handle.read().split()]
total_kb = 0
counted = 0
for pid in pids:
    try:
        with open(f'/proc/{pid}/smaps_rollup') as handle:
            for line in handle:
                if line.startswith('Pss:'):
                    total_kb += int(line.split()[1])
                    counted += 1
                    break
    except OSError:
        pass


def read(name):
    try:
        with open(os.path.join(root, name)) as handle:
            return handle.read().strip().replace('\n', ' ')
    except OSError:
        return 'unreadable'


current = read('memory.current')
print(f'PSS_MIB {total_kb / 1024:.1f}')
print(f'NPROC {counted} of {len(pids)}')
print(f'CGROUP_MEMORY_CURRENT_MIB {int(current) / 1048576:.1f}' if current.isdigit() else f'CGROUP_MEMORY_CURRENT_MIB {current}')
for name in ('memory.max', 'cpu.max', 'pids.max'):
    print(f'LIMIT {name} {read(name)}')
