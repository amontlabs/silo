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

spec = importlib.util.spec_from_file_location('package_engine', Path(__file__).with_name('package-engine.py'))
engine = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engine)

COMMIT = 'ab' * 20
VERSION = engine.engine_version()
ROOT = f'silo-engine-{VERSION}'
UNIVERSAL = f'silo-engine-{VERSION}-macos-universal.tar.gz'


class PackageEngineTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / 'binary'
        self.binary.write_bytes(b'\xcf\xfa\xed\xfe fake universal silod')
        self.out = self.root / 'out'

    def package(self, targets=engine.UNIVERSAL_TARGETS, silod=engine.SILOD, commit=COMMIT):
        return engine.package(self.out, targets, commit=commit, binary_for=lambda requested: self.binary,
                              toolchain='1.94.0', silod=silod)

    def members(self, archive):
        with tarfile.open(archive) as tar:
            return {member.name: member for member in tar}

    def read(self, archive, name):
        with tarfile.open(archive) as tar:
            return tar.extractfile(f'{ROOT}/{name}').read()

    def test_the_archive_has_one_top_level_directory_with_exactly_the_contracted_files(self):
        archive, _ = self.package()
        members = self.members(archive)
        self.assertEqual(archive.name, UNIVERSAL)
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
        self.assertEqual(info['targets'], ['aarch64-apple-darwin', 'x86_64-apple-darwin'])
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
        self.assertEqual(checksum.name, f'{UNIVERSAL}.sha256')
        text = checksum.read_text()
        self.assertRegex(text, rf'^[0-9a-f]{{64}}  {re.escape(UNIVERSAL)}\n$')
        self.assertEqual(text.split()[0], hashlib.sha256(archive.read_bytes()).hexdigest())

    def test_the_archive_is_reproducible(self):
        first, _ = self.package()
        digest = first.read_bytes()
        shutil.rmtree(self.out)
        second, _ = self.package()
        self.assertEqual(second.read_bytes(), digest)

    def test_a_single_architecture_build_is_named_and_listed_by_its_target(self):
        archive, _ = self.package(('aarch64-apple-darwin',))
        self.assertEqual(archive.name, f'silo-engine-{VERSION}-macos-arm64.tar.gz')
        self.assertEqual(json.loads(self.read(archive, 'release.json'))['targets'], ['aarch64-apple-darwin'])
        with self.assertRaises(ValueError):
            engine.archive_name(VERSION, ('aarch64-unknown-linux-gnu',))

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

    def test_main_builds_each_architecture_joins_them_and_names_the_commit(self):
        built, joined = [], []

        def build(target, toolchain):
            built.append((target, toolchain))
            return self.binary

        def lipo(binaries, output):
            joined.append(list(binaries))
            shutil.copy2(self.binary, output)
            return output

        engine.main(['--universal', '--out-dir', str(self.out)], build=build, lipo=lipo, commit_of=lambda: COMMIT)
        self.assertEqual(built, [('aarch64-apple-darwin', '1.94.0'), ('x86_64-apple-darwin', '1.94.0')])
        self.assertEqual(len(joined), 1)
        self.assertEqual(json.loads(self.read(self.out / UNIVERSAL, 'release.json'))['sourceCommit'], COMMIT)
        engine.main(['--target', 'aarch64-apple-darwin', '--out-dir', str(self.out), '--source-commit', 'cd' * 20],
                    build=build, lipo=lipo, commit_of=lambda: self.fail('--source-commit wins'))
        self.assertEqual(len(joined), 1)
        arm = self.out / f'silo-engine-{VERSION}-macos-arm64.tar.gz'
        self.assertEqual(json.loads(self.read(arm, 'release.json'))['sourceCommit'], 'cd' * 20)

    def test_cargo_is_run_with_the_pinned_toolchain_and_the_lockfile(self):
        calls = []

        def run(command, **kwargs):
            calls.append(command)
            return subprocess.CompletedProcess(command, 0, json.dumps({'target_directory': '/t'}))

        binary = engine.cargo_build('x86_64-apple-darwin', '1.94.0', run)
        self.assertEqual(binary, Path('/t/x86_64-apple-darwin/release/silod'))
        self.assertEqual(calls[0], ['cargo', '+1.94.0', 'build', '--locked', '--release', '-p', 'silod',
                                    '--target', 'x86_64-apple-darwin'])

    def test_lipo_must_produce_both_architectures(self):
        def run(archs):
            return lambda command, **kwargs: subprocess.CompletedProcess(command, 0, archs)

        output = self.root / 'joined'
        self.assertEqual(engine.lipo_create([Path('a'), Path('b')], output, run('x86_64 arm64\n')), output)
        with self.assertRaisesRegex(RuntimeError, 'arm64 and x86_64'):
            engine.lipo_create([Path('a')], output, run('arm64\n'))


if __name__ == '__main__':
    unittest.main()
