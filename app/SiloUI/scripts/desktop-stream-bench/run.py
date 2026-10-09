#!/usr/bin/env python3
"""Benchmark one open desktop viewer of a development build (never production).

Start the Dev app with `SILO_DESKTOP_BENCH_DIR=<dir>` (see README.md), open the
desktop of a throwaway computer, then run

    run.py --app "<path>/Silo Dev.app" --home ~/.silo-dev/<id> --computer e2e-stream \
           --bench-dir <dir> --label before

It starts `responder.py` on the guest display, asks the app for a report,
adds the guest's CPU use of the streaming processes and writes
`<bench-dir>/<label>.json`.
"""
import argparse
import base64
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
GUEST_SCRIPT = '/tmp/silo-stream-responder.py'
GUEST_PID = '/tmp/silo-stream-responder.pid'
DISPLAY_ENV = ['-e', 'DISPLAY=:1', '-e', 'XAUTHORITY=/run/silo-desktop/user/Xauthority']
# Guest processes whose CPU the report adds: capture/encode, X server, apps.
WATCHED = ('selkies', 'Xvfb', 'xfwm4', 'python3')


def msb(options, *command, user=None, check=True):
    contents = Path(options.app) / 'Contents'
    environment = dict(os.environ, MSB_HOME=str(Path(options.home).expanduser()),
                       MSB_PATH=str(contents / 'MacOS/msb'),
                       MSB_LIBKRUNFW_PATH=str(contents / 'Frameworks/libkrunfw.5.dylib'))
    argv = [str(contents / 'MacOS/msb'), 'exec', options.computer, '--no-start']
    if user:
        argv += ['-u', user]
    argv += [*(DISPLAY_ENV if user else []), '--', *command]
    result = subprocess.run(argv, env=environment, capture_output=True, text=True, timeout=120)
    if check and result.returncode != 0:
        sys.exit(f'msb exec failed ({result.returncode}): {result.stderr.strip()}')
    return result.stdout


def guest_cpu(options):
    """Jiffies per watched guest process, plus the total, from /proc."""
    script = ("import os\n"
              "c=list(map(int,open('/proc/stat').readline().split()[1:8]))\n"
              "out={'total':sum(c),'idle':c[3]+c[4]}\n"
              "for pid in filter(str.isdigit,os.listdir('/proc')):\n"
              "  try:\n"
              "    s=open(f'/proc/{pid}/stat').read();name=s[s.index('(')+1:s.rindex(')')]\n"
              "    f=s[s.rindex(')')+2:].split();out[pid]=[name,int(f[11])+int(f[12])]\n"
              "  except Exception: pass\n"
              "print(__import__('json').dumps(out))\n")
    return json.loads(msb(options, 'python3', '-c', script))


def cpu_delta(before, after, seconds, cores):
    # Guest jiffies: Linux USER_HZ is 100.
    used = {}
    for pid, value in after.items():
        if pid in ('total', 'idle') or not any(value[0].startswith(name) for name in WATCHED):
            continue
        spent = value[1] - (before.get(pid, [None, 0])[1])
        used[value[0]] = used.get(value[0], 0) + spent / 100 / seconds * 100
    total = after['total'] - before['total']
    busy = (total - (after['idle'] - before['idle'])) / total * 100 if total else 0
    return {'processesPercentOfOneCore': {k: round(v, 1) for k, v in used.items() if v > 0.1},
            'guestCpuPercentOfAllCores': round(busy, 1)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--home', required=True)
    parser.add_argument('--computer', required=True)
    parser.add_argument('--bench-dir', required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--probes', type=int, default=30)
    parser.add_argument('--idle', type=int, default=10)
    parser.add_argument('--motion', type=int, default=10)
    parser.add_argument('--keep-responder', action='store_true')
    options = parser.parse_args()
    bench = Path(options.bench_dir)

    encoded = base64.b64encode((HERE / 'responder.py').read_bytes()).decode()
    msb(options, 'sh', '-c', f'echo {encoded} | base64 -d > {GUEST_SCRIPT} && chmod 644 {GUEST_SCRIPT}')
    msb(options, 'sh', '-c',
        f'test -f {GUEST_PID} && kill $(cat {GUEST_PID}) 2>/dev/null; '
        f'setsid python3 {GUEST_SCRIPT} >/tmp/silo-stream-responder.log 2>&1 & echo $! > {GUEST_PID}',
        user='silo')
    for _ in range(30):
        time.sleep(0.5)
        if msb(options, 'cat', '/tmp/silo-stream-responder.log', check=False).startswith('ready'):
            break
    else:
        sys.exit('the guest responder did not start; see /tmp/silo-stream-responder.log in the guest')
    time.sleep(1)
    cores = int(msb(options, 'nproc').strip())

    def measure(probes, idle, motion):
        """One page report plus the guest CPU spent while it ran."""
        before_files = set(bench.glob('result-*.json'))
        before = guest_cpu(options)
        started = time.monotonic()
        # The app polls for request.json, so it appears complete or not at all.
        partial = bench / '.request.json.partial'
        partial.write_text(json.dumps({'probes': probes, 'idleSeconds': idle, 'motionSeconds': motion}))
        partial.replace(bench / 'request.json')
        deadline = time.monotonic() + 60 + probes * 4 + idle + motion
        result_file = None
        while time.monotonic() < deadline and not result_file:
            time.sleep(0.5)
            fresh = set(bench.glob('result-*.json')) - before_files
            result_file = min(fresh) if fresh else None
        elapsed = time.monotonic() - started
        after = guest_cpu(options)
        if not result_file:
            sys.exit('no benchmark result; is a viewer open and the app started with SILO_DESKTOP_BENCH_DIR?')
        return json.loads(result_file.read_text()), cpu_delta(before, after, elapsed, cores)

    # Probes and motion run over the fixture; the idle phase runs on the
    # ordinary desktop once the fixture is gone.
    report, fixture_cpu = measure(options.probes, 0, options.motion)
    log = msb(options, 'cat', '/tmp/silo-stream-responder.log', check=False)
    key_times = [float(line.split()[1]) for line in log.splitlines() if line.startswith('key ')]
    if not options.keep_responder:
        msb(options, 'sh', '-c', f'kill $(cat {GUEST_PID}) 2>/dev/null; rm -f {GUEST_PID}', user='silo', check=False)
        if options.idle > 0:
            time.sleep(2)
            idle, idle_cpu = measure(0, options.idle, 0)
            report.setdefault('page', {}).setdefault('phases', []).extend(
                phase for phase in idle.get('page', {}).get('phases', []) if phase['name'] == 'idle')
            report['idleHostProcesses'] = idle.get('hostProcesses')
            report['idleGuest'] = idle_cpu
    report['label'] = options.label
    report['guest'] = {'cores': cores, **fixture_cpu}
    # Guest clock times at which the fixture received each probe key, to split
    # input delay from picture delay (up to the guest/host clock offset).
    report['guest']['keyReceivedAt'] = key_times
    output = bench / f'{options.label}.json'
    output.write_text(json.dumps(report, indent=2))
    page = report.get('page', {})
    print(json.dumps({'label': options.label, 'inputToPicture': page.get('inputToPicture'),
                      'sink': page.get('sink'), 'errors': page.get('errors') or report.get('error'),
                      'guest': report['guest']}, indent=2))
    print(f'wrote {output}')


if __name__ == '__main__':
    main()
