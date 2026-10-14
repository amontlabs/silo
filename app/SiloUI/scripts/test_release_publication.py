"""Exercise the final publication gate with downloaded, complete draft assets."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publication_gate', Path(__file__).with_name('publish-release.py'))
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)


class PublicationGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.assets = Path(self.temporary.name)
        for name in publish.EXPECTED:
            (self.assets / name).write_text('signaturefixture' if name.endswith('.sig') else 'signed package fixture')
        publish.prepare(self.assets, '0.2.0', 'test/repository', 'Reviewed release notes')
        self.calls = []
        self.verifiers = []

    def checksum(self, name):
        """Change the draft's own checksum too, isolating later validation gates."""
        checksum_file = self.assets / 'SHA256SUMS'
        digest = hashlib.sha256((self.assets / name).read_bytes()).hexdigest()
        lines = [f'{digest}  {name}' if line.endswith(f'  {name}') else line
                 for line in checksum_file.read_text().splitlines()]
        checksum_file.write_text('\n'.join(lines) + '\n')

    def gh(self, *args):
        self.calls.append(args)
        if args[:1] == ('api',):
            return json.dumps([[{'tag_name': 'v0.2.0', 'draft': True, 'prerelease': False,
                                'assets': [{'name': path.name, 'size': path.stat().st_size}
                                           for path in self.assets.iterdir()]}]])
        if args[:2] == ('release', 'download'):
            for path in self.assets.iterdir():
                shutil.copyfile(path, Path(args[4]) / path.name)
            return ''
        if args[:2] == ('release', 'edit'):
            self.assertEqual(self.verifiers, ['verify-release-signatures.py', 'verify-release-metadata.py'])
            return ''
        self.fail(f'Unexpected GitHub action: {args}')

    def run_publish(self, fail_verifier=None):
        def verify(command, *, check):
            self.assertTrue(check)
            name = Path(command[1]).name
            self.verifiers.append(name)
            self.assertTrue(Path(command[2], 'Silo-macos-arm64.app.tar.gz').is_file())
            if name == fail_verifier:
                raise subprocess.CalledProcessError(1, command)
            return subprocess.CompletedProcess(command, 0)

        with patch.dict(os.environ, GH_REPO='test/repository'), \
                patch('sys.argv', ['publish-release', '0.2.0', '--publish']), \
                patch.object(publish, 'gh', side_effect=self.gh), \
                patch.object(publish.subprocess, 'run', side_effect=verify):
            publish.main()

    def assert_not_published(self):
        self.assertTrue(any(call[:2] == ('release', 'download') for call in self.calls))
        self.assertFalse(any(call[:2] == ('release', 'edit') for call in self.calls))

    def test_complete_verified_draft_is_published_only_after_both_verifiers(self):
        self.run_publish()
        self.assertEqual(self.verifiers, ['verify-release-signatures.py', 'verify-release-metadata.py'])
        self.assertEqual(self.calls[-1], ('release', 'edit', 'v0.2.0', '--draft=false', '--latest', '--prerelease=false'))
        self.assertEqual(sum(call[:2] == ('release', 'edit') for call in self.calls), 1)

    def test_empty_unexpected_draft_asset_cannot_publish(self):
        (self.assets / 'unreviewed-placeholder.zip').write_bytes(b'')
        with self.assertRaisesRegex(RuntimeError, 'Draft is incomplete'):
            self.run_publish()
        self.assertFalse(any(call[:2] == ('release', 'download') for call in self.calls))
        self.assertFalse(any(call[:2] == ('release', 'edit') for call in self.calls))

    def test_empty_required_draft_asset_cannot_publish(self):
        (self.assets / 'Silo-macos-arm64.app.tar.gz').write_bytes(b'')
        with self.assertRaisesRegex(RuntimeError, 'Draft is incomplete'):
            self.run_publish()
        self.assertFalse(any(call[:2] == ('release', 'download') for call in self.calls))
        self.assertFalse(any(call[:2] == ('release', 'edit') for call in self.calls))

    def test_changed_downloaded_package_cannot_publish(self):
        (self.assets / 'Silo-macos-arm64.app.tar.gz').write_text('changed after checksum creation')
        with self.assertRaisesRegex(RuntimeError, 'checksum mismatch'):
            self.run_publish()
        self.assert_not_published()
        self.assertEqual(self.verifiers, [])

    def test_feed_url_outside_versioned_release_cannot_publish_even_with_matching_checksum(self):
        feed = json.loads((self.assets / 'latest.json').read_text())
        feed['platforms']['darwin-aarch64']['url'] = 'https://example.invalid/other.tar.gz'
        (self.assets / 'latest.json').write_text(json.dumps(feed))
        self.checksum('latest.json')
        with self.assertRaisesRegex(RuntimeError, 'unexpected URL'):
            self.run_publish()
        self.assert_not_published()
        self.assertEqual(self.verifiers, [])

    def test_malformed_optional_feed_metadata_cannot_publish_with_matching_checksum(self):
        original = (self.assets / 'latest.json').read_text()
        for field, value in [('pub_date', 'not a date'), ('pub_date', '2026-02-30T12:00:00Z'),
                             ('pub_date', '2026-10-02T12:00:00'), ('pub_date', 42),
                             ('notes', {'text': 'invalid shape'})]:
            with self.subTest(field=field, value=value):
                self.calls.clear()
                self.verifiers.clear()
                feed = json.loads(original)
                feed[field] = value
                (self.assets / 'latest.json').write_text(json.dumps(feed))
                self.checksum('latest.json')
                with self.assertRaisesRegex(RuntimeError, 'Draft update feed'):
                    self.run_publish()
                self.assert_not_published()
                self.assertEqual(self.verifiers, [])

    def test_supported_optional_feed_metadata_publishes(self):
        original = (self.assets / 'latest.json').read_text()
        for date in (None, '2026-10-02T12:00:00Z', '2026-10-02T12:00:00.123456+02:00'):
            with self.subTest(date=date):
                self.calls.clear()
                self.verifiers.clear()
                feed = json.loads(original)
                feed['pub_date'] = date
                feed['notes'] = None
                (self.assets / 'latest.json').write_text(json.dumps(feed))
                self.checksum('latest.json')
                self.run_publish()
                self.assertEqual(self.calls[-1][:2], ('release', 'edit'))

    def test_invalid_signature_cannot_publish_even_with_matching_checksums(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_publish('verify-release-signatures.py')
        self.assert_not_published()
        self.assertEqual(self.verifiers, ['verify-release-signatures.py'])

    def test_signed_wrong_package_version_cannot_publish(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_publish('verify-release-metadata.py')
        self.assert_not_published()
        self.assertEqual(self.verifiers, ['verify-release-signatures.py', 'verify-release-metadata.py'])

    def test_duplicate_checksum_entry_cannot_publish(self):
        path = self.assets / 'SHA256SUMS'
        lines = path.read_text().splitlines()
        lines[-1] = lines[0]
        path.write_text('\n'.join(lines) + '\n')
        with self.assertRaisesRegex(RuntimeError, 'incomplete or duplicated'):
            self.run_publish()
        self.assert_not_published()


if __name__ == '__main__':
    unittest.main()
