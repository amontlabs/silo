"""Physical footprint (MiB) of a running computer's VM, read-only.

    python3 final_footprint.py COMPUTER_ID_OR_NAME

Finds the `msb` sandbox process whose command line names the computer (read from `ps`), then
runs `footprint -p PID` on it and on its children. Prints one line per process and the total.
"""
import re
import subprocess
import sys

needle = sys.argv[1]
processes = subprocess.run(['ps', '-axo', 'pid=,ppid=,command='], capture_output=True, text=True).stdout.splitlines()
rows = []
for line in processes:
    match = re.match(r'\s*(\d+)\s+(\d+)\s+(.*)', line)
    if match:
        rows.append((int(match[1]), int(match[2]), match[3]))
hits = [r for r in rows if needle in r[2] and 'final_footprint' not in r[2] and 'footprint -p' not in r[2]]
if not hits:
    print(f'no process names {needle}')
    sys.exit(1)
pids = {r[0] for r in hits}
changed = True
while changed:  # add descendants (the VM may run in a child of the sandbox process)
    changed = False
    for pid, ppid, _ in rows:
        if ppid in pids and pid not in pids:
            pids.add(pid)
            changed = True
total = 0.0
for pid, _, command in sorted(r for r in rows if r[0] in pids):
    result = subprocess.run(['footprint', '-p', str(pid), '--sort', 'dirty'], capture_output=True, text=True)
    summary = re.search(r'Footprint:\s+([\d.]+)\s*(KB|MB|GB)', result.stdout)
    if not summary:
        print(f'pid {pid}: no footprint ({result.stdout.strip()[:80] or result.stderr.strip()[:80]})  {command[:90]}')
        continue
    mib = float(summary[1]) * {'KB': 1 / 1024, 'MB': 1, 'GB': 1024}[summary[2]]
    total += mib
    print(f'pid {pid}: {mib:.1f} MiB  {command[:110]}')
print(f'TOTAL {total:.1f}')
