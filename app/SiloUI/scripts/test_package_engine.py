"""The engine release archive has the layout, names and metadata that apps embedding silod rely on."""
import hashlib
import importlib.util
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import unittest.mock

spec = importlib.util.spec_from_file_location('package_engine', Path(__file__).with_name('package-engine.py'))
engine = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engine)

COMMIT = 'ab' * 20
VERSION = engine.engine_version()
ROOT = f'silo-engine-{VERSION}'
ARCHIVE = f'silo-engine-{VERSION}-macos-arm64.tar.gz'


class PackageEngineTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / 'binary'
        self.binary.write_bytes(b'\xcf\xfa\xed\xfe fake arm64 silod')
        self.out = self.root / 'out'

    def package(self, silod=engine.SILOD, commit=COMMIT):
        return engine.package(self.out, commit=commit, binary=self.binary, toolchain='1.94.0', silod=silod)

    def members(self, archive):
        with tarfile.open(archive) as tar:
            return {member.name: member for member in tar}

    def read(self, archive, name):
        with tarfile.open(archive) as tar:
            return tar.extractfile(f'{ROOT}/{name}').read()

    def test_the_archive_has_one_top_level_directory_with_exactly_the_contracted_files(self):
        archive, _ = self.package()
        members = self.members(archive)
        self.assertEqual(archive.name, ARCHIVE)
        files = sorted(name for name, member in members.items() if member.isfile())
        self.assertEqual(files, sorted(f'{ROOT}/{name}' for name in (
            'release.json', 'bin/silod', 'macos/Info.production.plist', 'macos/Entitlements.plist',
            'macos/LaunchAgents/org.silo.preview.engine.plist', 'macos/resources/engine-manifest.production.json',
            'api/silo-api.schema.json')))
        self.assertTrue(all(name == ROOT or name.startswith(f'{ROOT}/') for name in members))
        self.assertEqual(members[f'{ROOT}/bin/silod'].mode, 0o755)
        self.assertEqual(self.read(archive, 'bin/silod'), self.binary.read_bytes())
        for member in members.values():
            self.assertEqual((member.uid, member.gid, member.mtime), (0, 0, 0))

    def test_release_json_describes_the_build(self):
        archive, _ = self.package()
        info = json.loads(self.read(archive, 'release.json'))
        schema = engine.SCHEMA.read_bytes()
        self.assertEqual(list(info), ['engineVersion', 'sourceCommit', 'targets', 'rustToolchain',
                                      'schemaSha256', 'protocolMin', 'protocolMax'])
        self.assertEqual(info['engineVersion'], VERSION)
        self.assertEqual(info['sourceCommit'], COMMIT)
        self.assertEqual(info['targets'], ['aarch64-apple-darwin'])
        self.assertEqual(info['rustToolchain'], '1.94.0')
        self.assertEqual(info['schemaSha256'], hashlib.sha256(schema).hexdigest())
        self.assertEqual(self.read(archive, 'api/silo-api.schema.json'), schema)
        manifest = json.loads(self.read(archive, 'macos/resources/engine-manifest.production.json'))
        self.assertEqual((info['protocolMin'], info['protocolMax']), (manifest['protocolMin'], manifest['protocolMax']))
        self.assertEqual(manifest['engineBuild'], f'silo-{COMMIT}')
        self.assertEqual(manifest['engineVersion'], VERSION)

    def test_the_engine_plist_leaves_the_signer_requirement_to_the_embedding_app(self):
        archive, _ = self.package()
        info = plistlib.loads(self.read(archive, 'macos/Info.production.plist'))
        self.assertEqual(info['CFBundleIdentifier'], 'org.silo.preview.engine')
        self.assertNotIn('SiloEngineCodeRequirement', info)

    def test_the_checksum_file_is_one_line_that_matches_the_archive(self):
        archive, checksum = self.package()
        self.assertEqual(checksum.name, f'{ARCHIVE}.sha256')
        text = checksum.read_text()
        self.assertRegex(text, rf'^[0-9a-f]{{64}}  {re.escape(ARCHIVE)}\n$')
        self.assertEqual(text.split()[0], hashlib.sha256(archive.read_bytes()).hexdigest())

    def test_the_archive_is_reproducible(self):
        first, _ = self.package()
        digest = first.read_bytes()
        shutil.rmtree(self.out)
        second, _ = self.package()
        self.assertEqual(second.read_bytes(), digest)

    def test_an_existing_archive_is_never_overwritten(self):
        self.package()
        with self.assertRaisesRegex(RuntimeError, 'already exists'):
            self.package()

    def test_the_source_commit_must_be_a_full_sha(self):
        for commit in ('abc123', 'AB' * 20, ''):
            with self.subTest(commit=commit), self.assertRaises(ValueError):
                self.package(commit=commit)

    def copy_silod(self, version=None, engine_version=None):
        silod = self.root / 'silod'
        (silod / 'macos/resources').mkdir(parents=True, exist_ok=True)
        for relative in engine.ENGINE_FILES:
            (silod / relative).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(engine.SILOD / relative, silod / relative)
        manifest = json.loads((engine.SILOD / engine.MANIFEST).read_text())
        manifest['engineVersion'] = engine_version or VERSION
        (silod / engine.MANIFEST).write_text(json.dumps(manifest))
        (silod / 'Cargo.toml').write_text(f'[package]\nname = "silod"\nversion = "{version or VERSION}"\n')
        return silod

    def test_the_cargo_version_must_equal_the_manifest_engine_version(self):
        self.assertEqual(engine.engine_version(self.copy_silod()), VERSION)
        with self.assertRaisesRegex(RuntimeError, 'must agree'):
            engine.engine_version(self.copy_silod(version='9.9.9'))
        with self.assertRaisesRegex(RuntimeError, 'must agree'):
            self.package(silod=self.copy_silod(engine_version='9.9.9'))
        with self.assertRaisesRegex(RuntimeError, 'MAJOR.MINOR.PATCH'):
            engine.engine_version(self.copy_silod(version='0.1', engine_version='0.1'))

    def test_the_checked_in_engine_is_packageable(self):
        self.assertEqual(engine.toolchain_version(), '1.94.0')
        self.assertEqual(engine.engine_version(), VERSION)

    def test_the_embedded_plist_must_not_pin_a_signer(self):
        silod = self.copy_silod()
        path = silod / 'macos/Info.production.plist'
        info = plistlib.loads(path.read_bytes())
        info['SiloEngineCodeRequirement'] = 'anchor apple'
        path.write_bytes(plistlib.dumps(info))
        with self.assertRaisesRegex(RuntimeError, 'must not set'):
            self.package(silod=silod)

    def test_main_builds_arm64_and_names_the_commit(self):
        built = []

        def build(target, toolchain):
            built.append((target, toolchain))
            return self.binary

        engine.main(['--out-dir', str(self.out)], build=build, commit_of=lambda: COMMIT)
        self.assertEqual(built, [('aarch64-apple-darwin', '1.94.0')])
        self.assertEqual(json.loads(self.read(self.out / ARCHIVE, 'release.json'))['sourceCommit'], COMMIT)
        shutil.rmtree(self.out)
        engine.main(['--out-dir', str(self.out), '--source-commit', 'cd' * 20], build=build,
                    commit_of=lambda: self.fail('--source-commit wins'))
        self.assertEqual(json.loads(self.read(self.out / ARCHIVE, 'release.json'))['sourceCommit'], 'cd' * 20)

    def test_only_apple_silicon_is_accepted(self):
        with self.assertRaises(SystemExit):
            engine.main(['--universal'], build=None)
        with self.assertRaises(SystemExit):
            engine.main(['--target', 'x86_64-apple-darwin'], build=None)

    def test_cargo_is_run_with_the_pinned_toolchain_and_the_lockfile(self):
        calls = []

        def run(command, **kwargs):
            calls.append(command)
            return subprocess.CompletedProcess(command, 0, json.dumps({'target_directory': '/t'}))

        binary = engine.cargo_build('aarch64-apple-darwin', '1.94.0', run)
        self.assertEqual(binary, Path('/t/aarch64-apple-darwin/release/silod'))
        self.assertEqual(calls[0], ['cargo', '+1.94.0', 'build', '--locked', '--release', '-p', 'silod',
                                    '--target', 'aarch64-apple-darwin'])


def elf(machine, text):
    """A stub little-endian ELF executable for `machine` (62 x86-64, 183 AArch64)."""
    return b'\x7fELF\x02\x01\x01' + bytes(9) + (2).to_bytes(2, 'little') + machine.to_bytes(2, 'little') + text.encode()


X64 = 'x86_64-unknown-linux-gnu'
LINUX_ARCHIVE = f'silo-engine-{VERSION}-linux-x64.tar.gz'


class LinuxPackageTests(unittest.TestCase):
    """The Linux archive is a runnable engine directory built from runtime:prepare's outputs."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.out = self.root / 'out'
        self.tauri = self.root / 'src-tauri'
        self.silod = self.root / 'silod-bin'
        self.cli = self.root / 'silo-bin'
        self.runner = self.root / 'silo-runner-bin'
        self.licenses = self.root / 'runner-licenses'
        self.licenses.mkdir()
        (self.licenses / 'scaleset-MIT.txt').write_text('MIT')
        self.silod.write_bytes(elf(62, 'silod'))
        self.cli.write_bytes(elf(62, 'silo'))
        self.runner.write_bytes(elf(62, 'silo-runner'))
        for helper in engine.CONTAINER_HELPERS:
            (self.silod.parent / helper).write_bytes(elf(62, helper))
        self.stage(X64, 62, 'amd64')

    def stage(self, target, machine, guest):
        """What runtime:prepare leaves in src-tauri for `target`."""
        binaries, runtime = self.tauri / 'binaries', self.tauri / 'runtime'
        binaries.mkdir(parents=True, exist_ok=True)
        for tool in engine.LINUX_ENGINE_TOOLS:
            (binaries / f'{tool}-{target}').write_bytes(elf(machine, tool))
            (binaries / f'{tool}-{target}').chmod(0o755)
        library = runtime / 'microsandbox' / target / 'lib' / engine.LIBKRUNFW
        library.parent.mkdir(parents=True, exist_ok=True)
        library.write_bytes(b'krunfw')
        (runtime / 'microsandbox/licenses').mkdir(parents=True, exist_ok=True)
        (runtime / 'microsandbox/licenses/microsandbox-Apache-2.0.txt').write_text('Apache')
        (runtime / 'microsandbox/manifest.json').write_text(json.dumps({
            'targetTriple': target,
            'executable': {'bundledName': 'msb', 'sha256': engine.sha256_of(binaries / f'msb-{target}')},
            'library': {'bundledName': engine.LIBKRUNFW, 'sha256': engine.sha256_of(library)},
        }))
        git = runtime / 'git'
        (git / 'licenses').mkdir(parents=True, exist_ok=True)
        (git / 'licenses/git-GPL-2.0.txt').write_text('GPL')
        (git / 'share/git-core/templates/hooks').mkdir(parents=True, exist_ok=True)
        (git / 'share/git-core/templates/description').write_text('template')
        hook = git / 'share/git-core/templates/hooks/pre-commit.sample'
        hook.write_text('#!/bin/sh\n')
        hook.chmod(0o755)
        (git / 'ssl').mkdir(exist_ok=True)
        (git / 'ssl/cacert.pem').write_text('certificates')
        (git / 'bin').mkdir(exist_ok=True)
        (git / 'bin/git').write_bytes(b'not shipped from here')
        (git / 'manifest.json').write_text(json.dumps({'targetTriple': target}))
        transfer = runtime / 'lfs-transfer'
        (transfer / 'licenses').mkdir(parents=True, exist_ok=True)
        (transfer / 'licenses/Go-LICENSE').write_text('Go')
        (transfer / 'LICENSE-MIT.txt').write_text('MIT')
        (transfer / 'git-lfs-transfer').write_bytes(elf(machine, 'transfer'))
        (transfer / 'git-lfs-transfer').chmod(0o755)
        (transfer / 'manifest.json').write_text(json.dumps({'architecture': guest}))
        silo = runtime / 'guest-silo'
        silo.mkdir(parents=True, exist_ok=True)
        (silo / 'silo').write_bytes(elf(machine, 'guest silo'))
        (silo / 'silo').chmod(0o755)
        (silo / 'manifest.json').write_text(json.dumps({'schemaVersion': 1, 'available': True, 'architecture': guest,
                                                       'binarySha256': engine.sha256_of(silo / 'silo')}))

    def package(self, platform='linux-x64', channel='production', commit=COMMIT):
        return engine.package_linux(self.out, platform=platform, commit=commit, silod_binary=self.silod,
                                    cli_binary=self.cli, runner_binary=self.runner, toolchain='1.94.0',
                                    channel=channel, tauri=self.tauri, runner_licenses=self.licenses)

    def members(self, archive):
        with tarfile.open(archive) as tar:
            return {member.name: member for member in tar}

    def read(self, archive, name):
        with tarfile.open(archive) as tar:
            return tar.extractfile(f'silo-engine/{name}').read()

    def test_the_archive_is_the_engine_directory_silod_runs_from(self):
        archive, checksum = self.package()
        self.assertEqual(archive.name, LINUX_ARCHIVE)
        members = self.members(archive)
        files = {name.removeprefix('silo-engine/'): member.mode for name, member in members.items() if member.isfile()}
        self.assertEqual(files, {
            'release.json': 0o644, 'LICENSE': 0o644, 'THIRD-PARTY-NOTICES.md': 0o644,
            'api/silo-api.schema.json': 0o644,
            'silod': 0o755, 'silo': 0o755, 'silo-runner': 0o755,
            'silo-ctr': 0o755, 'silo-init': 0o755,
            'resources/silo-runner-licenses/scaleset-MIT.txt': 0o644,
            'msb': 0o755, 'git': 0o755, 'git-lfs': 0o755, 'git-remote-http': 0o755, 'git-remote-https': 0o755,
            'silo-macos-gateway': 0o755,
            'resources/engine-manifest.json': 0o644,
            'resources/microsandbox/manifest.json': 0o644,
            'resources/microsandbox/licenses/microsandbox-Apache-2.0.txt': 0o644,
            f'resources/microsandbox/{X64}/lib/libkrunfw.so.5.6.1': 0o644,
            'resources/git-support/manifest.json': 0o644,
            'resources/git-support/licenses/git-GPL-2.0.txt': 0o644,
            'resources/git-support/share/git-core/templates/description': 0o644,
            'resources/git-support/share/git-core/templates/hooks/pre-commit.sample': 0o755,
            'resources/git-support/ssl/cacert.pem': 0o644,
            'resources/git-support/lfs-transfer/manifest.json': 0o644,
            'resources/git-support/lfs-transfer/LICENSE-MIT.txt': 0o644,
            'resources/git-support/lfs-transfer/licenses/Go-LICENSE': 0o644,
            'resources/git-support/lfs-transfer/git-lfs-transfer': 0o755,
            'resources/guest-support/silo/manifest.json': 0o644,
            'resources/guest-support/silo/silo': 0o755,
        })
        self.assertTrue(all(name == 'silo-engine' or name.startswith('silo-engine/') for name in members))
        for member in members.values():
            self.assertEqual((member.uid, member.gid, member.mtime, member.uname), (0, 0, 0, ''))
        self.assertEqual(self.read(archive, 'silod'), self.silod.read_bytes())
        self.assertEqual(self.read(archive, 'silo'), self.cli.read_bytes())
        self.assertEqual(self.read(archive, 'silo-runner'), self.runner.read_bytes())
        self.assertEqual(self.read(archive, 'msb'), (self.tauri / f'binaries/msb-{X64}').read_bytes())
        self.assertEqual(self.read(archive, 'resources/guest-support/silo/silo'), (self.tauri / 'runtime/guest-silo/silo').read_bytes())
        self.assertEqual(checksum.read_text(), f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {LINUX_ARCHIVE}\n')

    def test_the_certificate_bundle_linux_git_needs_is_required(self):
        self.assertIn('needsCertificateBundle: true', (engine.APP / 'scripts/git-runtime.mjs').read_text())
        self.assertIn('support.join("ssl/cacert.pem")', (engine.TAURI / 'engine/src/host_push.rs').read_text())
        (self.tauri / 'runtime/git/ssl/cacert.pem').unlink()
        with self.assertRaisesRegex(RuntimeError, 'has no resources/git-support/ssl/cacert.pem'):
            self.package()
        self.assertFalse(self.out.exists() and any(self.out.iterdir()))

    def test_the_paths_are_where_the_engine_looks(self):
        # bundled_tools::resolve: the executable's directory; bundled_runtime_library:
        # resources/microsandbox/<target>/lib/libkrunfw.so.5.6.1; dependencies: git-support.
        engine_rs = (engine.TAURI / 'engine/src/runtime/mod.rs').read_text()
        self.assertIn('.join("microsandbox")\n            .join(target)\n            .join("lib/libkrunfw.so.5.6.1")', engine_rs)
        self.assertIn(engine.LIBKRUNFW, engine_rs)
        paths_rs = (engine.SILOD / 'src/paths.rs').read_text()
        self.assertIn('pub const LINUX_RESOURCES_DIR: &str = "resources";', paths_rs)
        dependencies = (engine.TAURI / 'engine/src/dependencies.rs').read_text()
        self.assertIn('resource_dir.join("git-support/manifest.json")', dependencies)
        self.assertIn('resource_dir.join("microsandbox/manifest.json")', dependencies)
        self.assertIn('support.join("lfs-transfer/git-lfs-transfer")', (engine.TAURI / 'engine/src/host_push.rs').read_text())
        self.assertIn('"guest-support/silo"', (engine.TAURI / 'engine/src/guest_components/artifact.rs').read_text())
        conf = json.loads((engine.TAURI / 'tauri.conf.json').read_text())
        self.assertEqual(sorted(Path(name).name for name in conf['bundle']['externalBin']), sorted(engine.LINUX_TOOLS))

    def test_release_json_and_the_manifest_name_the_build_and_its_channel(self):
        archive, _ = self.package()
        info = json.loads(self.read(archive, 'release.json'))
        self.assertEqual(info['targets'], [X64])
        self.assertEqual(info['sourceCommit'], COMMIT)
        manifest = json.loads(self.read(archive, 'resources/engine-manifest.json'))
        self.assertEqual(manifest['channelIdentifier'], 'org.silo.preview')
        self.assertEqual(manifest['engineBuild'], f'silo-{COMMIT}')
        shutil.rmtree(self.out)
        archive, _ = self.package(channel='development')
        manifest = json.loads(self.read(archive, 'resources/engine-manifest.json'))
        self.assertEqual(manifest['channelIdentifier'], 'org.silo.dev')
        self.assertEqual(manifest['engineVersion'], VERSION)

    def test_the_archive_is_reproducible_and_never_overwritten(self):
        first, _ = self.package()
        digest = first.read_bytes()
        with self.assertRaisesRegex(RuntimeError, 'already exists'):
            self.package()
        shutil.rmtree(self.out)
        self.assertEqual(self.package()[0].read_bytes(), digest)

    def test_every_executable_must_be_built_for_the_platform(self):
        # The runtime staged for x86-64 is refused for arm64 by its own manifest.
        with self.assertRaisesRegex(RuntimeError, 'is for x86_64-unknown-linux-gnu, not aarch64'):
            self.package(platform='linux-arm64')
        self.stage('aarch64-unknown-linux-gnu', 183, 'arm64')
        with self.assertRaisesRegex(RuntimeError, 'silod is not a linux-arm64 executable'):
            self.package(platform='linux-arm64')
        self.silod.write_bytes(elf(183, 'silod'))
        self.cli.write_bytes(elf(183, 'silo'))
        with self.assertRaisesRegex(RuntimeError, 'silo-runner is not a linux-arm64 executable'):
            self.package(platform='linux-arm64')
        self.runner.write_bytes(elf(183, 'silo-runner'))
        with self.assertRaisesRegex(RuntimeError, 'silo-ctr is not a linux-arm64 executable'):
            self.package(platform='linux-arm64')
        for helper in engine.CONTAINER_HELPERS:
            (self.silod.parent / helper).write_bytes(elf(183, helper))
        archive, _ = self.package(platform='linux-arm64')
        self.assertEqual(archive.name, f'silo-engine-{VERSION}-linux-arm64.tar.gz')
        self.assertIn('silo-engine/resources/microsandbox/aarch64-unknown-linux-gnu/lib/libkrunfw.so.5.6.1',
                      self.members(archive))
        self.cli.write_bytes(b'#!/bin/sh\n')
        shutil.rmtree(self.out)
        with self.assertRaisesRegex(RuntimeError, 'silo is not a linux-arm64 executable'):
            self.package(platform='linux-arm64')

    def test_a_runtime_that_does_not_match_its_manifest_is_refused(self):
        (self.tauri / f'binaries/msb-{X64}').write_bytes(elf(62, 'another msb'))
        with self.assertRaisesRegex(RuntimeError, 'does not match the MicroSandbox runtime manifest'):
            self.package()
        self.stage(X64, 62, 'amd64')
        (self.tauri / 'runtime/lfs-transfer/manifest.json').write_text(json.dumps({'architecture': 'arm64'}))
        with self.assertRaisesRegex(RuntimeError, 'Git LFS transfer server is for arm64'):
            self.package()
        self.stage(X64, 62, 'amd64')
        (self.tauri / 'runtime/git/share/link').symlink_to('/etc/passwd')
        with self.assertRaisesRegex(RuntimeError, 'symbolic link'):
            self.package()

    def test_a_build_without_the_linux_program_of_silos_tools_is_not_packaged(self):
        silo = self.tauri / 'runtime/guest-silo'
        (silo / 'manifest.json').write_text(json.dumps({'schemaVersion': 1, 'available': False, 'architecture': 'amd64',
                                                       'reason': 'Zig is not installed.'}))
        (silo / 'silo').unlink()
        with self.assertRaisesRegex(RuntimeError, "no Linux program for Silo's tools: Zig is not installed.*SILO_REQUIRE_GUEST_SILO"):
            self.package()
        self.assertFalse(self.out.exists() and any(self.out.iterdir()))

    def test_a_program_that_is_not_the_manifests_or_not_for_the_architecture_is_refused(self):
        silo = self.tauri / 'runtime/guest-silo'
        (silo / 'silo').write_bytes(elf(62, 'another program'))
        with self.assertRaisesRegex(RuntimeError, 'does not match the guest silo manifest'):
            self.package()
        self.stage(X64, 62, 'amd64')
        manifest = json.loads((silo / 'manifest.json').read_text())
        (silo / 'manifest.json').write_text(json.dumps(dict(manifest, architecture='arm64')))
        with self.assertRaisesRegex(RuntimeError, "Linux program of Silo's tools is for arm64"):
            self.package()
        self.stage(X64, 62, 'amd64')
        (silo / 'silo').write_bytes(elf(183, 'arm program'))
        (silo / 'manifest.json').write_text(json.dumps(dict(manifest, binarySha256=engine.sha256_of(silo / 'silo'))))
        with self.assertRaisesRegex(RuntimeError, 'resources/guest-support/silo/silo is not a linux-x64 executable'):
            self.package()

    def test_a_missing_runtime_says_to_prepare_it(self):
        shutil.rmtree(self.tauri / 'runtime/microsandbox')
        with self.assertRaisesRegex(RuntimeError, 'runtime:prepare'):
            self.package()
        self.assertFalse(self.out.exists())

    def test_the_macos_archive_is_unchanged_by_linux_packaging(self):
        binary = self.root / 'binary'
        binary.write_bytes(b'fake arm64 silod')
        archive, _ = engine.package(self.root / 'mac', commit=COMMIT, binary=binary, toolchain='1.94.0')
        self.assertEqual(archive.name, ARCHIVE)
        self.assertNotIn('linux', json.dumps(json.loads(
            tarfile.open(archive).extractfile(f'{ROOT}/release.json').read())))

    def test_main_builds_silod_and_the_cli_for_the_platform_and_channel(self):
        built = []

        def build_linux(target, toolchain, channel):
            built.append((target, toolchain, channel))
            return self.silod, self.cli

        # main packages from src-tauri; this test packages from the fixture instead.
        calls = []
        real = engine.package_linux

        def package_linux(*args, **kwargs):
            calls.append(kwargs)
            return real(*args, tauri=self.tauri, **kwargs)

        engine.package_linux = package_linux
        self.addCleanup(setattr, engine, 'package_linux', real)
        runners = []

        build_dirs = []

        def build_runner(platform, version, out_dir):
            runners.append((platform, version))
            build_dirs.append(Path(out_dir))
            self.assertTrue(Path(out_dir).is_dir())
            return self.runner

        real_licenses = engine.RUNNER_LICENSES
        engine.RUNNER_LICENSES = self.licenses
        self.addCleanup(setattr, engine, 'RUNNER_LICENSES', real_licenses)
        engine.main(['--out-dir', str(self.out), '--platform', 'linux-x64', '--channel', 'development'],
                    build=lambda *_: self.fail('the macOS build is not used'), build_linux=build_linux,
                    build_runner=build_runner, commit_of=lambda: COMMIT)
        self.assertEqual(built, [(X64, '1.94.0', 'development')])
        self.assertEqual(calls[0]['channel'], 'development')
        self.assertEqual(runners, [('linux-x64', VERSION)])
        self.assertFalse(build_dirs[0].exists(), 'the runner build directory is removed after packaging')
        self.assertTrue((self.out / LINUX_ARCHIVE).is_file())
        with self.assertRaises(SystemExit):
            engine.main(['--channel', 'development'], build=None, build_linux=None)
        with self.assertRaises(SystemExit):
            engine.main(['--platform', 'linux-x86'], build=None, build_linux=None)

    def test_silo_runner_is_built_static_for_the_platform_with_the_engine_version(self):
        for platform, goarch in (('linux-x64', 'amd64'), ('linux-arm64', 'arm64')):
            calls = []

            def run(command, **options):
                calls.append((command, options))

            binary = engine.go_build_runner(platform, '0.9.1', run, self.root)
            self.assertEqual(binary, self.root / 'silo-runner')
            command, options = calls[0]
            self.assertEqual(command[:3], ['go', 'build', '-trimpath'])
            self.assertIn('-s -w -X main.version=0.9.1', command)
            self.assertEqual(command[-3:-1], ['-o', str(binary)])
            self.assertEqual((options['env']['GOOS'], options['env']['GOARCH'], options['env']['CGO_ENABLED']),
                             ('linux', goarch, '0'))
            self.assertEqual(options['cwd'], engine.RUNNER)
            self.assertTrue(options['check'])

    def test_hidden_files_and_directories_in_the_licenses_folder_are_not_packaged(self):
        (self.licenses / '.DS_Store').write_text('junk')
        (self.licenses / '.gitkeep').write_text('')
        (self.licenses / 'sub').mkdir()
        archive, _ = self.package()
        names = {name for name in self.members(archive) if '/resources/silo-runner-licenses/' in name}
        self.assertEqual({name.rsplit('/', 1)[1] for name in names}, {'scaleset-MIT.txt'})

    def test_the_runner_build_needs_a_directory_it_does_not_leave_behind(self):
        with self.assertRaises(ValueError):
            engine.go_build_runner('linux-x64', '0.9.1', lambda *a, **k: None)

    def test_the_checked_in_runner_licenses_cover_every_go_module_it_links(self):
        names = {path.name for path in engine.RUNNER_LICENSES.iterdir()}
        modules = (engine.RUNNER / 'go.mod').read_text()
        for module, license_file in (('actions/scaleset', 'scaleset-MIT.txt'), ('golang-jwt/jwt', 'golang-jwt-MIT.txt'),
                                     ('google/uuid', 'google-uuid-BSD-3-Clause.txt'), ('yaml.v3', 'yaml.v3-LICENSE.txt')):
            self.assertIn(module, modules)
            self.assertIn(license_file, names)
        self.assertEqual(len(re.findall(r'^\t[a-z]\S+ v', modules.split('require (')[1].split(')')[0], re.M)), 4)
        self.assertIn('go-LICENSE.txt', names)

    def test_the_cli_channel_is_compiled_in_for_production_only(self):
        for channel, expected in (('production', 'org.silo.preview'), ('development', None)):
            calls = []

            def run(command, **kwargs):
                calls.append((command, kwargs))
                return subprocess.CompletedProcess(command, 0, json.dumps({'target_directory': '/t'}))

            with self.subTest(channel=channel), unittest.mock.patch.dict(
                    engine.os.environ, {'SILO_CHANNEL_IDENTIFIER': 'org.silo.stale'}):
                silod, cli = engine.cargo_build_linux(X64, '1.94.0', channel, run)
                self.assertEqual((silod, cli), (Path(f'/t/{X64}/release/silod'), Path(f'/t/{X64}/release/silo')))
                command, options = calls[0]
                self.assertEqual(command, ['cargo', '+1.94.0', 'build', '--locked', '--release', '-p', 'silod',
                                           '-p', 'silo-cli', '-p', 'silo-container-shim', '--target', X64])
                self.assertEqual(options['env'].get('SILO_CHANNEL_IDENTIFIER'), expected)


if __name__ == '__main__':
    unittest.main()
