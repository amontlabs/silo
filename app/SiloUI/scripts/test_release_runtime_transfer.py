"""Exercise the public cache boundary and warm/cold workflow dependency decisions."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location('runtime_cache', Path(__file__).with_name('release-runtime-cache.py'))
CACHE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CACHE)
ACTION = (ROOT / '.github/actions/prepare-release-runtime/action.yml').read_text()
PLATFORM = (ROOT / '.github/workflows/release-platform.yml').read_text()
WORKFLOW = (ROOT / '.github/workflows/release.yml').read_text()
MSB = 'app/SiloUI/src-tauri/target/runtime-cache/v0.7.6/patched-builds/key/msb'


class RuntimeTransferTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.producer = self.root / 'producer'
        self.producer.mkdir()
        self.archive = self.root / 'runtime-cache.tar.gz'

    def write(self, name, value=b'public fixture', mode=0o644):
        path = self.producer / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(value)
        path.chmod(mode)
        return path

    def test_one_producer_transfers_public_inputs_to_two_consumers_preserving_modes(self):
        executable = b'compiled public runtime'
        self.write(MSB, executable, 0o755)
        self.write(MSB + '.sha256', hashlib.sha256(executable).hexdigest().encode())
        self.write('app/SiloUI/src-tauri/target/runtime-cache/dugite/v1/git.tar.gz')
        self.write('app/SiloUI/src-tauri/target/runtime-cache/git-lfs-transfer/pin/source.tar.gz')
        self.write('app/SiloUI/src-tauri/target/runtime-cache/git-lfs-transfer/pin/builds/linux-arm64/git-lfs-transfer', executable, 0o755)
        forbidden = [
            'app/SiloUI/github-build.local.json',
            'app/SiloUI/src-tauri/target/release/silo-ui',
            'app/SiloUI/src-tauri/runtime/release-info.json',
            'app/SiloUI/src-tauri/target/runtime-cache/v0.7.6/patched-builds/key/cargo-target/private',
            'app/SiloUI/src-tauri/target/runtime-cache/v0.7.6/patched-builds/key/work/private',
            'app/SiloUI/src-tauri/target/runtime-cache/git-lfs-transfer/pin/source-123/main.go',
        ]
        for name in forbidden:
            self.write(name, b'private sentinel')
        approved_digest = CACHE.pack(self.producer, self.archive)
        for name in ['native', 'package']:
            consumer = self.root / name
            consumer.mkdir()
            CACHE.unpack(consumer, self.archive, approved_digest)
            self.assertEqual((consumer / MSB).read_bytes(), executable)
            self.assertEqual((consumer / MSB).stat().st_mode & 0o777, 0o755)
            for private in forbidden:
                self.assertFalse((consumer / private).exists())

    def test_hardlinked_public_license_files_transfer_as_independent_regular_files(self):
        first_name = 'app/SiloUI/src-tauri/target/runtime-cache/v0.7.6/licenses/GPL.txt'
        second_name = 'app/SiloUI/src-tauri/target/runtime-cache/dugite/v1/licenses/GPL.txt'
        first = self.write(first_name, b'public license', 0o644)
        second = self.producer / second_name
        second.parent.mkdir(parents=True)
        os.link(first, second)
        approved_digest = CACHE.pack(self.producer, self.archive)
        consumer = self.root / 'consumer'
        consumer.mkdir()
        CACHE.unpack(consumer, self.archive, approved_digest)
        for name in (first_name, second_name):
            self.assertEqual((consumer / name).read_bytes(), b'public license')
        self.assertNotEqual((consumer / first_name).stat().st_ino,
                            (consumer / second_name).stat().st_ino)

    def test_archive_paths_match_existing_public_cache_allowlist(self):
        paths = re.findall(r'        path: \|\n((?:          .+\n)+)', ACTION)[0].split()
        self.assertEqual(set(paths), set(CACHE.FILES + CACHE.DIRECTORIES))

    def test_corrupt_transfer_fails_before_writing(self):
        self.write(MSB)
        approved_digest = CACHE.pack(self.producer, self.archive)
        with self.archive.open('ab') as stream:
            stream.write(b'corruption')
        consumer = self.root / 'consumer'
        consumer.mkdir()
        with self.assertRaisesRegex(ValueError, 'checksum'):
            CACHE.unpack(consumer, self.archive, approved_digest)
        self.assertEqual(list(consumer.iterdir()), [])

    def malicious_archive(self, member):
        with tarfile.open(self.archive, 'w:gz') as bundle:
            valid = tarfile.TarInfo(MSB)
            valid.size = 5
            bundle.addfile(valid, io.BytesIO(b'valid'))
            if member.isfile():
                member.size = 3
                bundle.addfile(member, io.BytesIO(b'bad'))
            else:
                bundle.addfile(member)
        return CACHE.digest(self.archive)

    def test_unsafe_paths_links_duplicates_and_private_files_fail_before_any_write(self):
        members = [tarfile.TarInfo(name) for name in [
            '../escape', '/absolute', MSB, 'app/SiloUI/github-build.local.json',
            MSB + '/../work/secret', 'app/SiloUI/src-tauri/target/release/silo-ui',
        ]]
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE]:
            member = tarfile.TarInfo(MSB + '.sha256')
            member.type = kind
            member.linkname = '../private'
            members.append(member)
        for index, member in enumerate(members):
            with self.subTest(name=member.name, kind=member.type):
                expected = self.malicious_archive(member)
                consumer = self.root / str(index)
                consumer.mkdir()
                with self.assertRaisesRegex(ValueError, 'unapproved'):
                    CACHE.unpack(consumer, self.archive, expected)
                self.assertEqual(list(consumer.iterdir()), [])

    def test_symlinked_producer_and_consumer_paths_are_rejected(self):
        private = self.root / 'private'
        private.write_bytes(b'private')
        source = self.write(MSB)
        source.unlink()
        source.symlink_to(private)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            CACHE.pack(self.producer, self.archive)
        source.unlink()
        self.write(MSB)
        expected = CACHE.pack(self.producer, self.archive)
        consumer = self.root / 'consumer'
        consumer.mkdir()
        (consumer / 'app').symlink_to(self.producer / 'app', target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            CACHE.unpack(consumer, self.archive, expected)

    def test_conditions_skip_warm_producer_and_preserve_cold_failure_and_benchmark_gates(self):
        # Execute the actual small workflow expressions, with contexts replaced
        # by parameters, so changing a gate changes this behavioral test.
        conditions = {}
        for job in ['runtime', 'native-tests', 'build']:
            block = PLATFORM.split(f'\n  {job}:\n', 1)[1]
            condition = re.search(r'    if: \$\{\{ (.+) \}\}', block).group(1)
            condition = condition.replace('inputs.runtime-cache-hit', 'cacheHit')
            condition = condition.replace('inputs.benchmark-schedule', 'schedule')
            condition = condition.replace('needs.runtime.result', 'runtimeResult')
            condition = condition.replace('cancelled()', 'cancelled')
            conditions[job] = condition
        cases = []
        for cache_hit, runtime_result in [(True, 'skipped'), (False, 'success'), (False, 'failure'), (False, 'cancelled')]:
            for schedule in ['parallel', 'sequential']:
                cancelled = runtime_result == 'cancelled'
                cases.append(dict(cacheHit=cache_hit, runtimeResult=runtime_result, schedule=schedule, cancelled=cancelled))
        javascript = '''
const [conditions,cases]=JSON.parse(process.argv[1]);
console.log(JSON.stringify(cases.map(({cacheHit,runtimeResult,schedule,cancelled}) =>
Object.fromEntries(Object.entries(conditions).map(([job,condition]) => [job,eval(condition)])))));
'''
        result = subprocess.run(['node', '-e', javascript, json.dumps([conditions, cases])], check=True, capture_output=True, text=True)
        for case, result in zip(cases, json.loads(result.stdout)):
            with self.subTest(case=case):
                self.assertEqual(result['runtime'], not case['cacheHit'])
                allowed = case['runtimeResult'] in ['success', 'skipped']
                self.assertEqual(result['build'], allowed)
                self.assertEqual(result['native-tests'], allowed and case['schedule'] == 'parallel')

    def test_platform_graph_and_cache_import_keep_verification_and_secret_boundaries(self):
        runtime = PLATFORM.split('\n  runtime:\n', 1)[1].split('\n  native-tests:\n', 1)[0]
        native = PLATFORM.split('\n  native-tests:\n', 1)[1].split('\n  build:\n', 1)[0]
        build = PLATFORM.split('\n  build:\n', 1)[1]
        for job in [runtime, native]:
            self.assertNotIn('secrets.', job)
            self.assertNotIn('environment:', job)
        self.assertIn("'release-signing' || 'release-verification'", build)
        self.assertIn('Require GitHub App configuration', build)
        self.assertIn('synthetic-unit-secret', native)
        self.assertEqual(PLATFORM.count('needs: runtime'), 2)
        self.assertNotIn('strategy:', PLATFORM)
        self.assertIn('needs: [validate, frontend, platforms, macos-minimum-constraints]', WORKFLOW)
        self.assertIn('uses: ./.github/workflows/release-platform.yml', WORKFLOW)
        self.assertEqual(WORKFLOW.count("lookup-only: 'true'"), 3)
        optional_cache = re.search(
            r'      - name: Restore public release dependencies only\n(.*?)(?=      - name:)',
            PLATFORM, re.S,
        )
        self.assertIsNotNone(optional_cache)
        self.assertIn('uses: actions/cache/restore@', optional_cache.group())
        self.assertIn('continue-on-error: true', optional_cache.group())
        self.assertNotIn('continue-on-error', WORKFLOW + PLATFORM.replace(optional_cache.group(), ''))
        self.assertLess(ACTION.index('release-runtime-cache.py unpack'), ACTION.index('runtime-prepare -- npm run runtime:prepare'))
        for name in ['Restore Cargo downloads', 'Prepare native resources']:
            self.assertIn(f"- name: {name}\n      if: inputs.lookup-only != 'true'", ACTION)


if __name__ == '__main__':
    unittest.main()
