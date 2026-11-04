"""Run every scripts/test_*.py module as its own unittest process, in parallel.

One module (the desktop recipe tests) spends most of the suite's time waiting
on shell subprocesses, so a serial `unittest discover` is bounded by it. Each
module's output is printed as soon as it finishes; a module that fails, finds
no tests (unittest exits 5) or exceeds its time limit fails the run.
"""
from concurrent.futures import ThreadPoolExecutor, as_completed
import os
from pathlib import Path
import subprocess
import sys

SCRIPTS = Path(__file__).resolve().parent
MODULE_TIMEOUT = 300  # seconds; the slowest module takes under a minute on CI


def run(module):
    command = [sys.executable, '-m', 'unittest', 'discover', '-s', str(SCRIPTS), '-p', module.name]
    try:
        result = subprocess.run(command, cwd=SCRIPTS.parent, capture_output=True, text=True, timeout=MODULE_TIMEOUT)
        return module.name, result.returncode, result.stdout + result.stderr
    except subprocess.TimeoutExpired as error:
        output = (error.stdout or b'').decode(errors='replace') + (error.stderr or b'').decode(errors='replace')
        return module.name, None, output + f'\nTIMED OUT after {MODULE_TIMEOUT} s'


def main():
    modules = sorted(SCRIPTS.glob('test_*.py'))
    if not modules:
        print('no script test modules found', file=sys.stderr)
        return 1
    workers = max(len(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else (os.cpu_count() or 2), 2)
    failed = []
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for future in as_completed([pool.submit(run, module) for module in modules]):
            name, code, output = future.result()
            print(f'::group::{name} ({"ok" if code == 0 else "FAILED"})', flush=True)
            print(output.rstrip(), flush=True)
            print('::endgroup::', flush=True)
            if code != 0:
                failed.append(name)
    for name in sorted(failed):
        print(f'FAILED: {name}', file=sys.stderr)
    print(f'{len(modules) - len(failed)}/{len(modules)} script test modules passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
