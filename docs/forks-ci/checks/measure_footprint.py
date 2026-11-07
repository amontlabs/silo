"""RAM (MiB) of a running computer's VM process tree, read-only.

    python3 measure_footprint.py COMPUTER_NAME

Finds the processes whose command line names the computer (the `msb` sandbox process), adds their
descendants, and sums memory per process: macOS `footprint -p PID` (physical footprint), Linux
Pss from /proc/PID/smaps_rollup (shares counted proportionally). Prints one line per process and
`TOTAL <MiB>`; the method is printed as `method: ...`.
"""
import os
import platform
import re
import subprocess
import sys

needle = re.compile(r'(?<![\w.-])' + re.escape(sys.argv[1]) + r'(?![\w.-])')
me = os.getpid()
LINUX = platform.system() == 'Linux'


def processes():
    rows = []
    if LINUX:
        for entry in os.listdir('/proc'):
            if not entry.isdigit():
                continue
            try:
                with open(f'/proc/{entry}/cmdline', 'rb') as f:
                    command = f.read().replace(b'\0', b' ').decode(errors='replace').strip()
                with open(f'/proc/{entry}/stat') as f:
                    ppid = int(f.read().rsplit(')', 1)[1].split()[1])
            except (OSError, ValueError, IndexError):
                continue
            rows.append((int(entry), ppid, command))
    else:
        out = subprocess.run(['ps', '-axo', 'pid=,ppid=,command='], capture_output=True, text=True).stdout
        for line in out.splitlines():
            m = re.match(r'\s*(\d+)\s+(\d+)\s+(.*)', line)
            if m:
                rows.append((int(m[1]), int(m[2]), m[3]))
    return rows


def mib(pid):
    if LINUX:
        try:
            with open(f'/proc/{pid}/smaps_rollup') as f:
                for line in f:
                    if line.startswith('Pss:'):
                        return int(line.split()[1]) / 1024
        except OSError as e:
            return str(e)
        return 'no Pss line'
    r = subprocess.run(['footprint', '-p', str(pid), '--sort', 'dirty'], capture_output=True, text=True)
    s = re.search(r'Footprint:\s+([\d.]+)\s*(KB|MB|GB)', r.stdout)
    if not s:
        return (r.stdout.strip() or r.stderr.strip())[:80] or 'no footprint'
    return float(s[1]) * {'KB': 1 / 1024, 'MB': 1, 'GB': 1024}[s[2]]


rows = processes()
skip = ('measure_footprint', 'footprint -p', 'measure_forks')
hits = [r for r in rows if r[0] != me and needle.search(r[2]) and not any(s in r[2] for s in skip)]
if not hits:
    print(f'no process names {sys.argv[1]}')
    print('TOTAL 0.0')
    sys.exit(1)
pids = {r[0] for r in hits}
grew = True
while grew:
    grew = False
    for pid, ppid, _ in rows:
        if ppid in pids and pid not in pids:
            pids.add(pid)
            grew = True
print('method: ' + ('sum of Pss, /proc/PID/smaps_rollup' if LINUX else 'footprint -p PID (physical footprint)'))
total = 0.0
for pid, _, command in sorted(r for r in rows if r[0] in pids):
    v = mib(pid)
    if isinstance(v, str):
        print(f'pid {pid}: no value ({v})  {command[:90]}')
        continue
    total += v
    print(f'pid {pid}: {v:.1f} MiB  {command[:110]}')
print(f'TOTAL {total:.1f}')
