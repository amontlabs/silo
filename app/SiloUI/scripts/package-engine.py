"""Package the engine (`silod`) as release tarballs.

macOS (`macos-arm64`): the archive other apps embed. It holds the unsigned binary and the files an
app needs to assemble `Silo Engine.app`. The app re-signs it and sets `SiloEngineCodeRequirement`
in the engine Info.plist to its own signer (docs/SiloUI-RELEASES.md#engine-releases).

Linux (`linux-x64`, `linux-arm64`): a complete engine directory for a Linux host, `silod` with the
`silo` command, the `silo-runner` CI helper and the bundled tools beside it and `resources/` holding
the manifest and the runtime (docs/SiloUI-ENGINE.md#silod-on-a-linux-host). The runtime comes from
`npm run runtime:prepare` on a host of the same architecture."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import tarfile
import tempfile
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
MACOS = 'macos-arm64'

TAURI = APP / 'src-tauri'
NOTICES = APP / 'THIRD-PARTY-NOTICES.md'
LICENSE = APP.parent.parent / 'LICENSE'
# Linux release platforms, their Rust targets and ELF machine numbers.
LINUX_TARGETS = {
    'linux-x64': ('x86_64-unknown-linux-gnu', 62),
    'linux-arm64': ('aarch64-unknown-linux-gnu', 183),
}
PLATFORMS = (MACOS, *LINUX_TARGETS)
# The engine's channel decides the manifest; the `silo` command's is compiled in from this
# variable (silo-cli `engine::build_channel`), and only production sets it.
CHANNELS = {
    'production': ('macos/resources/engine-manifest.production.json', 'org.silo.preview'),
    'development': ('macos/resources/engine-manifest.development.json', None),
}
CLI_CHANNEL_VARIABLE = 'SILO_CHANNEL_IDENTIFIER'
# Beside silod: bundled_tools::resolve is the executable's directory for a plain engine
# directory, where the runtime finds `msb` and Git its executables (GIT_EXEC_PATH).
LINUX_TOOLS = ('msb', 'git', 'git-lfs', 'git-remote-http', 'git-remote-https')
# The engine archive also carries the network gateway (named for the Mac it began on): it runs a
# container computer's network on a Linux host, beside silod like the tools.
LINUX_ENGINE_TOOLS = (*LINUX_TOOLS, 'silo-macos-gateway')
# The container computer helpers (silo-container-shim): built with silod, so they sit beside it in
# the cargo release directory, and ship beside it in the archive.
CONTAINER_HELPERS = ('silo-ctr', 'silo-init')
LINUX_ROOT = 'silo-engine'
# The CI runner helper (a Go program of its own), built for the platform and shipped beside silod
# with the licenses of the Go modules it links (docs/SiloUI-CI-RUNNER.md#linux-hosts).
RUNNER = APP / 'runner'
RUNNER_LICENSES = RUNNER / 'licenses'
RUNNER_GOARCH = {'linux-x64': 'amd64', 'linux-arm64': 'arm64'}
LIBKRUNFW = 'libkrunfw.so.5.6.1'
# runtime:prepare's outputs under src-tauri/runtime, and where each goes under resources/: the
# paths the app bundle's Tauri resources use (tauri.conf.json, tauri.linux.conf.json).
LINUX_RESOURCE_TREES = (
    ('microsandbox/licenses', 'microsandbox/licenses'),
    ('git/licenses', 'git-support/licenses'),
    ('git/share', 'git-support/share'),
    ('git/ssl', 'git-support/ssl'),
    ('lfs-transfer', 'git-support/lfs-transfer'),
    ('guest-silo', 'guest-support/silo'),
)
GUEST_ARCHITECTURES = {'x86_64-unknown-linux-gnu': 'amd64', 'aarch64-unknown-linux-gnu': 'arm64'}


def engine_version(silod=SILOD, manifest_path=MANIFEST):
    """The silod crate version, which must equal `engineVersion` in the manifest (production's by default)."""
    crate = tomllib.loads((silod / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    manifest = json.loads((silod / manifest_path).read_text(encoding='utf-8'))
    if manifest['engineVersion'] != crate:
        raise RuntimeError(f'silod Cargo.toml version {crate} and engineVersion {manifest["engineVersion"]} '
                           f'in {manifest_path} must agree.')
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', crate):
        raise RuntimeError(f'The engine version must be MAJOR.MINOR.PATCH, not {crate}.')
    return crate


def archive_name(version, platform=MACOS):
    return f'silo-engine-{version}-{platform}.tar.gz'


def sha256_of(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def release_info(version, commit, toolchain, manifest, target=TARGET):
    return {
        'engineVersion': version,
        'sourceCommit': commit,
        'targets': [target],
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


def check_commit(commit):
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('The source commit must be a full 40-character lowercase hex SHA.')


def fresh_outputs(out_dir, name):
    out_dir = Path(out_dir)
    archive, checksum = out_dir / name, out_dir / f'{name}.sha256'
    if archive.exists() or checksum.exists():
        raise RuntimeError(f'{name} already exists in {out_dir}; use a fresh output directory.')
    return out_dir, archive, checksum


def package(out_dir, *, commit, binary, toolchain, silod=SILOD):
    """Writes the tarball and its `.sha256`; returns their paths. `binary` is the arm64 silod to ship."""
    check_commit(commit)
    version = engine_version(silod)
    name = archive_name(version)
    out_dir, archive, checksum = fresh_outputs(out_dir, name)
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


def elf_machine(path):
    """The ELF `e_machine` of a little-endian executable, or None when `path` is not one."""
    header = Path(path).read_bytes()[:20]
    if len(header) < 20 or header[:4] != b'\x7fELF' or header[5] != 1:
        return None
    return int.from_bytes(header[18:20], 'little')


def read_json(path, label):
    try:
        return json.loads(Path(path).read_text(encoding='utf-8'))
    except FileNotFoundError:
        raise RuntimeError(f'{label} is missing: {path}. Run `npm run runtime:prepare` on a host of '
                           'this architecture first.') from None


def tree_files(source, destination):
    """Every regular file under `source` as {destination/relative: (path, mode)}; executables keep 0755."""
    files = {}
    for path in sorted(Path(source).rglob('*')):
        if path.is_symlink():
            raise RuntimeError(f'{path} is a symbolic link; the engine archive holds regular files only.')
        if path.is_file():
            mode = 0o755 if path.stat().st_mode & 0o111 else 0o644
            files[f'{destination}/{path.relative_to(source).as_posix()}'] = (path, mode)
    return files


def linux_runtime_files(target, tauri=TAURI):
    """The bundled tools and runtime resources that runtime:prepare staged for `target`, checked
    against their own manifests, as archive members relative to the engine directory."""
    binaries, runtime = tauri / 'binaries', tauri / 'runtime'
    microsandbox = read_json(runtime / 'microsandbox/manifest.json', 'The MicroSandbox runtime manifest')
    git = read_json(runtime / 'git/manifest.json', 'The bundled Git manifest')
    transfer = read_json(runtime / 'lfs-transfer/manifest.json', 'The Git LFS transfer manifest')
    guest_silo = read_json(runtime / 'guest-silo/manifest.json', "The Linux program of Silo's tools")
    if not guest_silo.get('available'):
        raise RuntimeError("The staged runtime has no Linux program for Silo's tools: "
                           f"{guest_silo.get('reason', 'it was not built')} A release needs Zig; run `npm run runtime:prepare` "
                           'with SILO_REQUIRE_GUEST_SILO=1.')
    guest_binary = runtime / 'guest-silo/silo'
    if not guest_binary.is_file() or sha256_of(guest_binary) != guest_silo.get('binarySha256'):
        raise RuntimeError(f"{guest_binary} does not match the guest silo manifest. Run `npm run runtime:prepare` again.")
    for label, found, wanted in (('MicroSandbox runtime', microsandbox.get('targetTriple'), target),
                                 ('bundled Git', git.get('targetTriple'), target),
                                 ('Git LFS transfer server', transfer.get('architecture'), GUEST_ARCHITECTURES[target]),
                                 ("Linux program of Silo's tools", guest_silo.get('architecture'), GUEST_ARCHITECTURES[target])):
        if found != wanted:
            raise RuntimeError(f'The staged {label} is for {found}, not {wanted}; prepare the runtime on this architecture.')
    files = {}
    for tool in LINUX_ENGINE_TOOLS:
        source = binaries / f'{tool}-{target}'
        if not source.is_file():
            raise RuntimeError(f'{source} is missing. Run `npm run runtime:prepare` first.')
        files[tool] = (source, 0o755)
    library = runtime / 'microsandbox' / target / 'lib' / LIBKRUNFW
    for path, expected in ((files['msb'][0], microsandbox['executable']['sha256']),
                           (library, microsandbox['library']['sha256'])):
        if not path.is_file() or sha256_of(path) != expected:
            raise RuntimeError(f'{path} does not match the MicroSandbox runtime manifest.')
    files[f'resources/microsandbox/{target}/lib/{LIBKRUNFW}'] = (library, 0o644)
    files['resources/microsandbox/manifest.json'] = (runtime / 'microsandbox/manifest.json', 0o644)
    files['resources/git-support/manifest.json'] = (runtime / 'git/manifest.json', 0o644)
    for source, destination in LINUX_RESOURCE_TREES:
        if (runtime / source).is_dir():
            files.update(tree_files(runtime / source, f'resources/{destination}'))
    # Every Linux Git build verifies HTTPS against the bundled bundle (git-runtime.mjs,
    # needsCertificateBundle; host_push reads git-support/ssl/cacert.pem).
    for required in ('resources/git-support/share/git-core/templates', 'resources/git-support/lfs-transfer/git-lfs-transfer',
                     'resources/guest-support/silo/silo', 'resources/guest-support/silo/manifest.json',
                     'resources/git-support/ssl/cacert.pem',
                     'resources/microsandbox/licenses', 'resources/git-support/licenses'):
        if not any(name == required or name.startswith(f'{required}/') for name in files):
            raise RuntimeError(f'The staged runtime has no {required}. Run `npm run runtime:prepare` again.')
    return files


def package_linux(out_dir, *, platform, commit, silod_binary, cli_binary, runner_binary, toolchain,
                  channel='production', silod=SILOD, tauri=TAURI, runner_licenses=RUNNER_LICENSES):
    """Writes the Linux engine tarball and its `.sha256`; returns their paths. `silod_binary`,
    `cli_binary` and `runner_binary` are silod, `silo` (for the same channel) and `silo-runner` built
    for the platform."""
    check_commit(commit)
    target, machine = LINUX_TARGETS[platform]
    manifest_path, _ = CHANNELS[channel]
    version = engine_version(silod, manifest_path)
    name = archive_name(version, platform)
    out_dir, archive, checksum = fresh_outputs(out_dir, name)
    manifest = json.loads((silod / manifest_path).read_text(encoding='utf-8'))
    manifest['engineBuild'] = f'silo-{commit}'
    files = {
        'release.json': (json.dumps(release_info(version, commit, toolchain, manifest, target), indent=2).encode() + b'\n', 0o644),
        'silod': (Path(silod_binary), 0o755),
        'silo': (Path(cli_binary), 0o755),
        'silo-runner': (Path(runner_binary), 0o755),
        'resources/engine-manifest.json': ((json.dumps(manifest, indent=2) + '\n').encode(), 0o644),
        'api/silo-api.schema.json': (SCHEMA, 0o644),
        'THIRD-PARTY-NOTICES.md': (NOTICES, 0o644),
        'LICENSE': (LICENSE, 0o644),
    }
    files.update(linux_runtime_files(target, tauri))
    licenses = sorted(path for path in Path(runner_licenses).glob('*')
                      if path.is_file() and not path.name.startswith('.'))
    if not licenses:
        raise RuntimeError(f'{runner_licenses} holds no licenses for silo-runner.')
    files.update({f'resources/silo-runner-licenses/{path.name}': (path, 0o644) for path in licenses})
    for helper in CONTAINER_HELPERS:
        source = Path(silod_binary).parent / helper
        if not source.is_file():
            raise RuntimeError(f'{source} is missing. Build silo-container-shim for the platform.')
        files[helper] = (source, 0o755)
    for executable in ('silod', 'silo', 'silo-runner', *CONTAINER_HELPERS, *LINUX_ENGINE_TOOLS, 'resources/git-support/lfs-transfer/git-lfs-transfer',
                       'resources/guest-support/silo/silo'):
        found = elf_machine(files[executable][0])
        if found != machine:
            raise RuntimeError(f'{executable} is not a {platform} executable (ELF machine {found}, wanted {machine}).')
    out_dir.mkdir(parents=True, exist_ok=True)
    write_archive(archive, LINUX_ROOT, files)
    checksum.write_text(f'{sha256_of(archive)}  {name}\n', encoding='utf-8')
    return archive, checksum


def cargo_release_dir(target, toolchain, run=subprocess.run, packages=('silod',), env=None):
    """Builds `packages` for `target` with the pinned toolchain and returns their release directory."""
    command = ['cargo', f'+{toolchain}', 'build', '--locked', '--release']
    for name in packages:
        command += ['-p', name]
    options = {'cwd': APP, 'check': True}
    if env is not None:
        options['env'] = env
    run(command + ['--target', target], **options)
    metadata = run(['cargo', f'+{toolchain}', 'metadata', '--format-version', '1', '--no-deps', '--locked'],
                   cwd=APP, check=True, capture_output=True, text=True)
    return Path(json.loads(metadata.stdout)['target_directory']) / target / 'release'


def cargo_build(target, toolchain, run=subprocess.run):
    """Builds silod for `target` and returns the binary."""
    return cargo_release_dir(target, toolchain, run) / 'silod'


def cargo_build_linux(target, toolchain, channel, run=subprocess.run):
    """Builds silod, `silo` and the container helpers for `target`; the CLI's channel is compiled in.
    Returns silod and `silo`; the helpers are beside silod."""
    env = {key: value for key, value in os.environ.items() if key != CLI_CHANNEL_VARIABLE}
    identifier = CHANNELS[channel][1]
    if identifier:
        env[CLI_CHANNEL_VARIABLE] = identifier
    release = cargo_release_dir(target, toolchain, run, packages=('silod', 'silo-cli', 'silo-container-shim'), env=env)
    return release / 'silod', release / 'silo'


def go_build_runner(platform, version, run=subprocess.run, out_dir=None):
    """Builds `silo-runner` for the platform (a static binary, so any host can build either
    architecture) and returns its path, in out_dir (the caller's, which it removes). Needs the Go
    version in runner/go.mod."""
    if out_dir is None:
        raise ValueError('go_build_runner needs a directory for the binary.')
    out = Path(out_dir) / 'silo-runner'
    env = {**os.environ, 'GOOS': 'linux', 'GOARCH': RUNNER_GOARCH[platform], 'CGO_ENABLED': '0'}
    run(['go', 'build', '-trimpath', '-ldflags', f'-s -w -X main.version={version}', '-o', str(out), '.'],
        cwd=RUNNER, env=env, check=True)
    return out


def toolchain_version(runtime_inputs=RUNTIME_INPUTS):
    return json.loads(runtime_inputs.read_text(encoding='utf-8'))['toolchain']


def source_commit():
    result = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=APP, capture_output=True, text=True, check=True)
    return result.stdout.strip()


def main(argv=None, *, build=cargo_build, build_linux=cargo_build_linux, build_runner=go_build_runner,
         commit_of=source_commit):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out-dir', type=Path, default=Path('engine-release'))
    parser.add_argument('--source-commit', default=None, help='default: git rev-parse HEAD')
    parser.add_argument('--platform', choices=PLATFORMS, default=MACOS)
    parser.add_argument('--channel', choices=tuple(CHANNELS), default='production',
                        help='Linux only: development packages a Silo Dev engine for tests')
    args = parser.parse_args(argv)
    toolchain = toolchain_version()
    if args.platform == MACOS:
        if args.channel != 'production':
            parser.error('the macOS archive is production only')
        archive, checksum = package(args.out_dir, commit=args.source_commit or commit_of(),
                                    binary=build(TARGET, toolchain), toolchain=toolchain)
    else:
        target = LINUX_TARGETS[args.platform][0]
        silod_binary, cli_binary = build_linux(target, toolchain, args.channel)
        # The runner is built in a directory of its own that goes away once it is in the archive.
        with tempfile.TemporaryDirectory(prefix='silo-runner-') as runner_dir:
            runner_binary = build_runner(args.platform, engine_version(SILOD, CHANNELS[args.channel][0]),
                                         out_dir=runner_dir)
            archive, checksum = package_linux(args.out_dir, platform=args.platform,
                                              commit=args.source_commit or commit_of(),
                                              silod_binary=silod_binary, cli_binary=cli_binary,
                                              runner_binary=runner_binary, toolchain=toolchain,
                                              channel=args.channel)
    print(archive)
    print(checksum)


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Engine packaging failed: {error}', file=sys.stderr)
        sys.exit(1)
