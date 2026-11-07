"""Machine facts, one line each, for macOS (sysctl, sw_vers) and Linux (/proc, /etc/os-release).

    python3 measure_facts.py            # facts
    python3 measure_facts.py avail-mib  # memory available for new VMs, in MiB
"""
import os
import platform
import re
import subprocess
import sys


def run(*cmd):
    return subprocess.run(cmd, capture_output=True, text=True).stdout.strip()


def linux_meminfo():
    return {k: int(v.split()[0]) for k, v in (l.split(':', 1) for l in open('/proc/meminfo'))}


def avail_mib():
    if platform.system() == 'Linux':
        return linux_meminfo()['MemAvailable'] / 1024
    page = int(run('sysctl', '-n', 'hw.pagesize'))
    stats = {m[1]: int(m[2]) for m in re.finditer(r'(Pages [\w ]+?):\s+(\d+)\.', run('vm_stat'))}
    pages = sum(stats.get(k, 0) for k in ('Pages free', 'Pages inactive', 'Pages speculative', 'Pages purgeable'))
    return pages * page / 1048576


if len(sys.argv) > 1 and sys.argv[1] == 'avail-mib':
    print(f'{avail_mib():.0f}')
    sys.exit(0)

if platform.system() == 'Linux':
    cpu = next((l.split(':', 1)[1].strip() for l in open('/proc/cpuinfo') if l.startswith('model name')), 'unknown cpu')
    cores = sum(1 for l in open('/proc/cpuinfo') if l.startswith('processor'))
    os_release = dict(l.rstrip().split('=', 1) for l in open('/etc/os-release') if '=' in l)
    info = linux_meminfo()
    print(f"machine: {cpu}, {info['MemTotal'] / 1048576:.1f} GiB RAM ({info['MemAvailable'] / 1048576:.1f} GiB available), "
          f"{cores} cores, {os_release.get('PRETTY_NAME', 'Linux').strip(chr(34))}, kernel {platform.release()}, {platform.machine()}")
    print(f"kvm: {'present' if os.path.exists('/dev/kvm') else 'absent'}; swap {info.get('SwapTotal', 0) / 1048576:.1f} GiB")
else:
    print(f"machine: {run('sysctl', '-n', 'machdep.cpu.brand_string')}, {int(run('sysctl', '-n', 'hw.memsize')) // 1073741824} GiB RAM "
          f"({avail_mib() / 1024:.1f} GiB free+inactive+purgeable), {run('sysctl', '-n', 'hw.ncpu')} cores, "
          f"macOS {run('sw_vers', '-productVersion')} ({run('sw_vers', '-buildVersion')}), {platform.machine()}")
