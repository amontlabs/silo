#!/usr/bin/env python3
"""Measure a synthetic, dependency-only Cargo cache without enabling production reuse."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import signal
import subprocess
import sys
import tarfile
import time

PRODUCER = 'SILO-CACHE-PRODUCER-CONFIG-SENTINEL'
CONSUMER = 'SILO-CACHE-CONSUMER-CONFIG-SENTINEL'


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + '\n')


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def command_output(command):
    return subprocess.check_output(command, text=True, stderr=subprocess.STDOUT).strip()


def context(args):
    app = args.app_root.resolve()
    config = json.loads((app / 'src-tauri/tauri.conf.json').read_text())
    files = [app / 'src-tauri/Cargo.toml', app / 'src-tauri/tauri.conf.json']
    files += sorted((app / 'src-tauri/vendor').rglob('*'))
    for root in [app.parent.parent, app, app / 'src-tauri']:
        files += [root / '.cargo/config', root / '.cargo/config.toml']
    hashes = {str(path.relative_to(app.parent.parent)): sha(path) for path in files if path.is_file()}
    tools = {'rustc': command_output(['rustc', '-vV']), 'cargo': command_output(['cargo', '-V'])}
    if sys.platform == 'darwin':
        tools.update(sdkPath=command_output(['xcrun', '--show-sdk-path']),
                     sdkVersion=command_output(['xcrun', '--show-sdk-version']),
                     xcode=command_output(['xcodebuild', '-version']),
                     compiler=command_output(['xcrun', 'clang', '--version']))
    else:
        tools.update(compiler=command_output(['cc', '--version']),
                     libc=command_output(['ldd', '--version']),
                     nativeLibraries=command_output(['pkg-config', '--modversion', 'webkit2gtk-4.1', 'gtk+-3.0', 'openssl']))
    value = {'schemaVersion': 1, 'target': args.target, 'tools': tools, 'files': hashes,
             'profile': 'release', 'features': ['tauri/custom-protocol'],
             'tauriCli': json.loads((app / 'node_modules/@tauri-apps/cli/package.json').read_text())['version'],
             'minimumMacos': config['bundle']['macOS']['minimumSystemVersion'],
             'runnerImage': {name: os.environ.get(name, '') for name in ['ImageOS', 'ImageVersion']},
             'environmentHashes': {name: hashlib.sha256(os.environ.get(name, '').encode()).hexdigest() for name in [
                 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                 'CC', 'CXX', 'AR', 'CFLAGS', 'CXXFLAGS', 'LDFLAGS', 'SDKROOT', 'MACOSX_DEPLOYMENT_TARGET',
                 'CARGO_INCREMENTAL', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_DEBUG',
                 'CARGO_PROFILE_RELEASE_LTO', 'CARGO_PROFILE_RELEASE_CODEGEN_UNITS']}}
    write(args.report, value)


def build(args):
    started = time.monotonic()
    app = args.app_root.resolve()
    metadata = json.loads(args.metadata.read_text())
    root_id = metadata['resolve']['root']
    registry_ids = {package['id'] for package in metadata['packages'] if package.get('source') == 'registry+https://github.com/rust-lang/crates.io-index'}
    registry_units = {'fresh': 0, 'compiled': 0}
    target_dir = Path(os.environ['CARGO_TARGET_DIR']).resolve()
    if target_dir != app / 'src-tauri/target/release-compile':
        raise ValueError('Use the stable dedicated release-compile target')
    if args.role == 'producer' and target_dir.exists() and any(target_dir.iterdir()):
        raise ValueError('Control compilation must begin with an empty target')
    invocation = [str(app / 'node_modules/.bin/tauri'), 'build', '--target', args.target,
                  '--no-bundle', '--ci', '--config', '{"build":{"beforeBuildCommand":""}}',
                  '--', '--locked', '--timings', '--message-format=json']
    root_artifacts = []
    args.messages.parent.mkdir(parents=True, exist_ok=True)
    with args.messages.open('w') as messages:
        with subprocess.Popen(invocation, cwd=app, stdout=subprocess.PIPE, text=True,
                              encoding='utf-8', errors='replace', start_new_session=True) as process:
            try:
                for line in process.stdout:
                    print(line, end='', flush=True)
                    try:
                        item = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if not isinstance(item, dict):
                        continue
                    messages.write(json.dumps(item) + '\n')
                    if item.get('reason') == 'compiler-artifact' and item.get('package_id') in registry_ids:
                        registry_units['fresh' if item.get('fresh') else 'compiled'] += 1
                    if item.get('reason') == 'compiler-artifact' and item.get('package_id') == root_id and item.get('executable'):
                        root_artifacts.append(item)
                process.stdout.close()
                code = process.wait()
            except BaseException:
                if process.returncode is None:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                raise
    marker = PRODUCER if args.role == 'producer' else CONSUMER
    forbidden = CONSUMER if args.role == 'producer' else PRODUCER
    fresh = bool(root_artifacts) and all(item.get('fresh') is False for item in root_artifacts)
    rotated = bool(root_artifacts) and all(marker.encode() in Path(item['executable']).read_bytes()
                                         and forbidden.encode() not in Path(item['executable']).read_bytes()
                                         for item in root_artifacts)
    report = {'role': args.role, 'compileSeconds': time.monotonic() - started, 'exitCode': code,
              'appRecompiled': fresh, 'configurationVerified': rotated, 'registryUnits': registry_units,
              'artifactScope': 'synthetic unbundled application; no package/release readiness claim'}
    write(args.report, report)
    if code:
        return code if code >= 0 else 128 - code
    if not fresh or not rotated:
        print('Application freshness or synthetic configuration boundary failed.', file=sys.stderr)
        return 1
    return 0


def pack(args):
    started = time.monotonic()
    if args.cache_dir.is_symlink():
        raise ValueError('Cache directory must not be a symlink')
    with tarfile.open(args.archive, 'w:gz', compresslevel=1) as archive:
        for path in sorted(args.cache_dir.rglob('*')):
            if path.is_symlink():
                raise ValueError('Cache payload symlinks are forbidden')
            if path.is_file():
                archive.add(path, arcname=str(path.relative_to(args.cache_dir)), recursive=False)
    archive_sha = sha(args.archive)
    if args.report:
        write(args.report, {'archiveSeconds': time.monotonic() - started, 'archiveBytes': args.archive.stat().st_size})
    print(f'sha256={archive_sha}')


def extract(archive_path, cache_dir, expected):
    if sha(archive_path) != expected:
        raise ValueError('Dependency archive SHA256 differs from producer output')
    if cache_dir.exists():
        raise ValueError('Dependency cache extraction directory must be new')
    with tarfile.open(archive_path, 'r:gz') as archive:
        names = set()
        members = archive.getmembers()
        for item in members:
            path = PurePosixPath(item.name)
            if (not item.isfile() or path.is_absolute() or '..' in path.parts or item.name in names
                    or str(path) != item.name or (item.name != 'manifest.json' and path.parts[0] != 'artifacts')):
                raise ValueError('Unsafe or unapproved dependency archive member')
            names.add(item.name)
        if 'manifest.json' not in names:
            raise ValueError('Dependency archive has no manifest')
        cache_dir.mkdir(parents=True)
        for item in members:
            path = cache_dir / item.name
            path.parent.mkdir(parents=True, exist_ok=True)
            with archive.extractfile(item) as source, path.open('wb') as output:
                shutil.copyfileobj(source, output)


def restore(args):
    expected_target = args.app_root.resolve() / 'src-tauri/target/release-compile'
    if args.target_dir.resolve() != expected_target or args.target_dir.is_symlink():
        raise ValueError('Refusing cleanup outside the dedicated benchmark target')
    accepted, error = False, None
    try:
        extract(args.archive, args.cache_dir, args.sha256)
        command = [sys.executable, str(args.app_root / 'scripts/cargo-dependency-cache.py'), 'restore',
                   '--metadata', str(args.metadata), '--lockfile', str(args.app_root / 'Cargo.lock'),
                   '--context', str(args.context), '--target', args.target, '--target-dir', str(args.target_dir),
                   '--cargo-home', str(args.cargo_home), '--cache-dir', str(args.cache_dir), '--report', str(args.audit_report),
                   '--forbid', PRODUCER, '--forbid', CONSUMER]
        subprocess.run(command, check=True)
        accepted = True
    except (ValueError, OSError, subprocess.CalledProcessError, tarfile.TarError) as failure:
        error = str(failure)
        print('Dependency cache rejected; continue with an empty target and normal compilation.', file=sys.stderr)
        shutil.rmtree(args.target_dir, ignore_errors=True)
        args.target_dir.mkdir(parents=True, exist_ok=True)
    started = json.loads(args.started.read_text())['monotonic']
    write(args.report, {'accepted': accepted, 'error': error, 'transferRestoreSeconds': time.monotonic() - started})


def summarize(args):
    reports = {}
    for name, path in [('control', args.control), ('consumer', args.consumer), ('restore', args.restore_report)]:
        reports[name] = json.loads(path.read_text()) if path.exists() else None
    control, consumer, imported = [reports[name] for name in ['control', 'consumer', 'restore']]
    eligible, gain = False, None
    if control and consumer and imported:
        net = imported['transferRestoreSeconds'] + consumer['compileSeconds']
        gain = 1 - net / control['compileSeconds']
        eligible = (gain >= 0.30 and imported['accepted'] and all(
            report['exitCode'] == 0 and report['appRecompiled'] and report['configurationVerified']
            for report in [control, consumer]))
    write(args.report, {'target': args.target, 'eligibleAtThirtyPercent': eligible, 'netReduction': gain,
                        'reports': reports, 'scope': 'one synthetic compile pair including consumer transfer; no production cache enabled'})
    print(f'Dependency benchmark {args.target}: eligible={eligible}, net reduction={gain}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['context', 'build', 'pack', 'mark', 'restore', 'summarize'])
    parser.add_argument('--app-root', type=Path, default=Path.cwd())
    for name in ['report', 'metadata', 'messages', 'cache-dir', 'archive', 'target-dir', 'cargo-home',
                 'context', 'audit-report', 'started', 'control', 'consumer', 'restore-report']:
        parser.add_argument('--' + name, type=Path)
    parser.add_argument('--target')
    parser.add_argument('--role', choices=['producer', 'consumer'])
    parser.add_argument('--sha256')
    args = parser.parse_args()
    if args.command == 'mark':
        write(args.report, {'monotonic': time.monotonic()})
        return 0
    return {'context': context, 'build': build, 'pack': pack, 'restore': restore, 'summarize': summarize}[args.command](args) or 0


if __name__ == '__main__':
    sys.exit(main())
