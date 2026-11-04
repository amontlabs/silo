"""Run every scripts/test_*.py module as its own unittest process, in parallel.

One module (the desktop recipe tests) spends most of the suite's time waiting
on shell subprocesses, so a serial `unittest discover` is bounded by it. Output
is printed per module in name order; the exit status is non-zero if any module
fails or finds no tests.
"""
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import subprocess
import sys

SCRIPTS = Path(__file__).resolve().parent


def run(module):
    result = subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', str(SCRIPTS), '-p', module.name],
                            cwd=SCRIPTS.parent, capture_output=True, text=True)
    return module.name, result.returncode, result.stdout + result.stderr


def main():
    modules = sorted(SCRIPTS.glob('test_*.py'))
    if not modules:
        print('no script test modules found', file=sys.stderr)
        return 1
    failed = []
    with ThreadPoolExecutor(max_workers=os.cpu_count() or 2) as pool:
        for name, code, output in pool.map(run, modules):
            print(f'::group::{name}', flush=True)
            print(output.rstrip(), flush=True)
            print('::endgroup::', flush=True)
            if code != 0 or 'Ran 0 tests' in output:
                failed.append(name)
    for name in failed:
        print(f'FAILED: {name}', file=sys.stderr)
    print(f'{len(modules) - len(failed)}/{len(modules)} script test modules passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
