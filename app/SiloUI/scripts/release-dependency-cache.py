#!/usr/bin/env python3
"""Prepare, validate and consume the public release dependency cache."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tomllib


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def paths(app):
    return app / 'src-tauri/target/release-compile', Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')).resolve()


def prepare(args):
    app = args.app_root.resolve()
    args.state.mkdir(parents=True, exist_ok=True)
    target_dir, cargo_home = paths(app)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir), CARGO_HOME=str(cargo_home))
    metadata_path = args.state / 'metadata.json'
    with metadata_path.open('w') as output:
        subprocess.run(['cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', args.target,
                        '--features', 'tauri/custom-protocol', '--manifest-path', str(app / 'src-tauri/Cargo.toml')],
                       env=env, stdout=output, check=True)
    raw_path = args.state / 'raw-context.json'
    module('dependency-cache-benchmark').context(argparse.Namespace(app_root=app, target=args.target, report=raw_path))
    raw = json.loads(raw_path.read_text())
    for name in ['app/SiloUI/src-tauri/Cargo.toml', 'app/SiloUI/src-tauri/tauri.conf.json']:
        raw['files'].pop(name, None)
    for name in ['config', 'config.toml']:
        path = cargo_home / name
        raw['files']['cargo-home/' + name] = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    # Capture all profile/target and compiler overrides, storing hashes only.
    prefixes = ('CARGO_PROFILE_', 'CARGO_TARGET_', 'CC_', 'CXX_', 'AR_', 'CFLAGS_', 'CXXFLAGS_', 'LDFLAGS_')
    for name, value in os.environ.items():
        if name.startswith(prefixes):
            raw['environmentHashes'][name] = hashlib.sha256(value.encode()).hexdigest()
    raw.update(targetDir=str(target_dir), cargoHome=str(cargo_home))
    semantic = module('cargo-dependency-identity').semantic_inputs(
        json.loads(metadata_path.read_text()), tomllib.loads((app / 'Cargo.lock').read_text()),
        tomllib.loads((app / 'src-tauri/Cargo.toml').read_text()),
        json.loads((app / 'src-tauri/tauri.conf.json').read_text()), raw)
    semantic['identityMode'] = 'semantic-root-version-v1'
    context = args.state / 'context.json'
    context.write_text(json.dumps(semantic, sort_keys=True, separators=(',', ':')) + '\n')
    digest = hashlib.sha256(context.read_bytes())
    for name in ['cargo-dependency-cache.py', 'cargo-dependency-identity.py', 'release-dependency-cache.py', 'dependency-cache-benchmark.py']:
        digest.update((app / 'scripts' / name).read_bytes())
    print('key=silo-public-release-deps-v1-' + args.target + '-' + digest.hexdigest())


def cache_command(args, command):
    target_dir, cargo_home = paths(args.app_root.resolve())
    return [sys.executable, str(args.app_root / 'scripts/cargo-dependency-cache.py'), command,
            '--metadata', str(args.state / 'metadata.json'), '--lockfile', str(args.app_root / 'Cargo.lock'),
            '--context', str(args.state / 'context.json'), '--target', args.target,
            '--target-dir', str(target_dir), '--cargo-home', str(cargo_home), '--cache-dir', str(args.state / 'cache')]


def restore(args):
    target_dir, _ = paths(args.app_root.resolve())
    if target_dir.is_symlink():
        raise ValueError('Dedicated compilation target must not be a symlink')
    if not (args.state / 'cache/manifest.json').is_file():
        print('Public dependency cache miss; compiling normally.')
        return
    result = subprocess.run(cache_command(args, 'restore'))
    if result.returncode:
        shutil.rmtree(target_dir, ignore_errors=True)
        print('Public dependency cache rejected; compiling normally with an empty target.')
    else:
        print('Verified public dependency cache restored; application will compile.')


def build(args):
    app = args.app_root.resolve()
    target_dir, cargo_home = paths(app)
    metadata = json.loads((args.state / 'metadata.json').read_text())
    root = metadata['resolve']['root']
    artifacts = []
    command = [str(app / 'node_modules/.bin/tauri'), 'build', '--target', args.target, '--no-bundle', '--ci',
               '--', '--locked', '--timings', '--message-format=json']
    with (args.state / 'messages.jsonl').open('w') as output:
        with subprocess.Popen(command, cwd=app, env=dict(os.environ, CARGO_TARGET_DIR=str(target_dir), CARGO_HOME=str(cargo_home)),
                              stdout=subprocess.PIPE, text=True, encoding='utf-8', errors='replace', start_new_session=True) as process:
            try:
                for line in process.stdout:
                    try:
                        row = json.loads(line)
                    except json.JSONDecodeError:
                        print(line, end='', flush=True)
                        continue
                    if not isinstance(row, dict):
                        print(line, end='', flush=True)
                        continue
                    output.write(json.dumps(row) + '\n')
                    if row.get('reason') == 'compiler-message':
                        rendered = row.get('message', {}).get('rendered')
                        if rendered:
                            print(rendered, end='' if rendered.endswith('\n') else '\n', file=sys.stderr, flush=True)
                    if row.get('reason') == 'compiler-artifact' and row.get('package_id') == root and row.get('executable'):
                        artifacts.append(row)
                process.stdout.close()
                code = process.wait()
            except BaseException:
                # Keep the group ID reserved until its compiler children stop.
                if process.returncode is None:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                raise
    if code:
        return code if code >= 0 else 128 - code
    if not artifacts or any(row.get('fresh') is not False for row in artifacts):
        raise ValueError('Release application must be freshly compiled')
    print('Release application freshly compiled.')
    return 0


def export(args):
    if os.environ.get('GITHUB_REF') != 'refs/heads/main' or os.environ.get('SILO_GITHUB_CLIENT_SECRET') != 'SILO-CACHE-PRODUCER-CONFIG-SENTINEL':
        raise ValueError('Only the main-branch synthetic warmer may export dependencies')
    subprocess.run(cache_command(args, 'export') + ['--messages', str(args.state / 'messages.jsonl'),
                   '--forbid', 'SILO-CACHE-PRODUCER-CONFIG-SENTINEL'], check=True)
    subprocess.run(cache_command(args, 'audit') + ['--forbid', 'SILO-CACHE-PRODUCER-CONFIG-SENTINEL'], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['prepare', 'restore', 'build', 'export'])
    parser.add_argument('--app-root', type=Path, default=Path.cwd())
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--target', required=True)
    args = parser.parse_args()
    return globals()[args.command](args) or 0


if __name__ == '__main__':
    sys.exit(main())
