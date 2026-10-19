"""Package the engine (`silod`) as the release tarball other apps embed.

The archive holds the unsigned binary and the files an app needs to assemble `Silo Engine.app`.
The app re-signs it and sets `SiloEngineCodeRequirement` in the engine Info.plist to its own
signer (docs/SiloUI-RELEASES.md#engine-releases)."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import tarfile
import tomllib

APP = Path(__file__).resolve().parent.parent
SILOD = APP / 'src-tauri/silod'
SCHEMA = APP / 'api/schema/silo-api.schema.json'
RUNTIME_INPUTS = APP / 'runtime-inputs.json'
MANIFEST = 'macos/resources/engine-manifest.production.json'
ENGINE_FILES = (
    'macos/Info.production.plist',
    'macos/Entitlements.plist',
    'macos/LaunchAgents/org.silo.preview.engine.plist',
)
# Silo hosts are Apple Silicon only, and silo-engine's macOS VM code builds for nothing else.
TARGET = 'aarch64-apple-darwin'
REQUIREMENT_KEY = 'SiloEngineCodeRequirement'


def engine_version(silod=SILOD):
    """The silod crate version, which must equal `engineVersion` in the production manifest."""
    crate = tomllib.loads((silod / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    manifest = json.loads((silod / MANIFEST).read_text(encoding='utf-8'))
    if manifest['engineVersion'] != crate:
        raise RuntimeError(f'silod Cargo.toml version {crate} and engineVersion {manifest["engineVersion"]} '
                           f'in {MANIFEST} must agree.')
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', crate):
        raise RuntimeError(f'The engine version must be MAJOR.MINOR.PATCH, not {crate}.')
    return crate


def archive_name(version):
    return f'silo-engine-{version}-macos-arm64.tar.gz'


def sha256_of(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def release_info(version, commit, toolchain, manifest):
    return {
        'engineVersion': version,
        'sourceCommit': commit,
        'targets': [TARGET],
        'rustToolchain': toolchain,
        'schemaSha256': sha256_of(SCHEMA),
        'protocolMin': manifest['protocolMin'],
        'protocolMax': manifest['protocolMax'],
    }


def write_archive(destination, root, files):
    """A reproducible gzip tarball: sorted entries, no owners, fixed times. `files` maps relative
    names to (source path or bytes, mode)."""
    directories = {root}
    for name in files:
        parent = Path(root) / name
        directories.update(str(p) for p in parent.parents if str(p) not in ('.', ''))
    with destination.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0, filename='') as compressed, \
            tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as tar:
        entries = [(directory, None, 0o755) for directory in directories] + [
            (f'{root}/{name}', content, mode) for name, (content, mode) in files.items()]
        for name, content, mode in sorted(entries, key=lambda entry: entry[0]):
            info = tarfile.TarInfo(name)
            info.mtime, info.uid, info.gid, info.uname, info.gname, info.mode = 0, 0, 0, '', '', mode
            if content is None:
                info.type = tarfile.DIRTYPE
                tar.addfile(info)
                continue
            data = content if isinstance(content, bytes) else Path(content).read_bytes()
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))


def package(out_dir, *, commit, binary, toolchain, silod=SILOD):
    """Writes the tarball and its `.sha256`; returns their paths. `binary` is the arm64 silod to ship."""
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('The source commit must be a full 40-character lowercase hex SHA.')
    version = engine_version(silod)
    name = archive_name(version)
    out_dir = Path(out_dir)
    archive, checksum = out_dir / name, out_dir / f'{name}.sha256'
    if archive.exists() or checksum.exists():
        raise RuntimeError(f'{name} already exists in {out_dir}; use a fresh output directory.')
    manifest = json.loads((silod / MANIFEST).read_text(encoding='utf-8'))
    if REQUIREMENT_KEY in plistlib.loads((silod / 'macos/Info.production.plist').read_bytes()):
        raise RuntimeError(f'Info.production.plist must not set {REQUIREMENT_KEY}; the app that embeds the engine does.')
    binary = Path(binary)
    manifest['engineBuild'] = f'silo-{commit}'
    files = {
        'release.json': (json.dumps(release_info(version, commit, toolchain, manifest), indent=2).encode() + b'\n', 0o644),
        'bin/silod': (binary, 0o755),
        MANIFEST: ((json.dumps(manifest, indent=2) + '\n').encode(), 0o644),
        'api/silo-api.schema.json': (SCHEMA, 0o644),
    }
    for relative in ENGINE_FILES:
        files[relative] = (silod / relative, 0o644)
    out_dir.mkdir(parents=True, exist_ok=True)
    write_archive(archive, f'silo-engine-{version}', files)
    checksum.write_text(f'{sha256_of(archive)}  {name}\n', encoding='utf-8')
    return archive, checksum


def cargo_build(target, toolchain, run=subprocess.run):
    """Builds silod for `target` and returns the binary."""
    run(['cargo', f'+{toolchain}', 'build', '--locked', '--release', '-p', 'silod', '--target', target],
        cwd=APP, check=True)
    metadata = run(['cargo', f'+{toolchain}', 'metadata', '--format-version', '1', '--no-deps', '--locked'],
                   cwd=APP, check=True, capture_output=True, text=True)
    return Path(json.loads(metadata.stdout)['target_directory']) / target / 'release/silod'


def toolchain_version(runtime_inputs=RUNTIME_INPUTS):
    return json.loads(runtime_inputs.read_text(encoding='utf-8'))['toolchain']


def source_commit():
    result = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=APP, capture_output=True, text=True, check=True)
    return result.stdout.strip()


def main(argv=None, *, build=cargo_build, commit_of=source_commit):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out-dir', type=Path, default=Path('engine-release'))
    parser.add_argument('--source-commit', default=None, help='default: git rev-parse HEAD')
    args = parser.parse_args(argv)
    toolchain = toolchain_version()
    archive, checksum = package(args.out_dir, commit=args.source_commit or commit_of(),
                                binary=build(TARGET, toolchain), toolchain=toolchain)
    print(archive)
    print(checksum)


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Engine packaging failed: {error}', file=sys.stderr)
        sys.exit(1)
