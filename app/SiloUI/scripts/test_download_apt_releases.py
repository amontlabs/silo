import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('download_apt_releases', Path(__file__).with_name('download-apt-releases.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class DownloadTests(unittest.TestCase):
    def download(self, latest='v0.2.0', tamper=False, mutable=(), unattested=(), missing=()):
        releases = [dict(tag_name=v, draft=d, prerelease=False, immutable=v not in mutable) for v, d in [('v0.3.0', True), ('v0.1.0', False), ('v0.2.0', False)]]
        self.verified = []
        def gh(*args):
            self.assertTrue(args[-1].startswith('repos/example/fork/releases'))
            return json.dumps({'tag_name': latest} if args[-1].endswith('/latest') else [releases])
        def fetch(args, **kwargs):
            self.assertEqual(args[args.index('--repo') + 1], 'example/fork')
            if args[:3] == ['gh', 'release', 'verify-asset']:
                tag, package = args[3], Path(args[4])
                if tag in unattested:
                    return subprocess.CompletedProcess(args, 1, '', 'digest mismatch for asset')
                if tag in missing:
                    return subprocess.CompletedProcess(args, 1, '', f'no attestations for tag {tag} (sha1:0000)')
                self.verified.append((tag, package.name))
                return subprocess.CompletedProcess(args, 0, '', '')
            self.assertTrue(kwargs['check'])
            self.assertEqual(args[:3], ['gh', 'release', 'download'])
            directory = Path(args[args.index('--dir') + 1])
            sums = []
            for name in ('Silo-linux-x64.deb', 'Silo-linux-arm64.deb'):
                data = b'disposable package download fixture'
                (directory / name).write_bytes(data + (b'changed' if tamper else b''))
                sums.append(hashlib.sha256(data).hexdigest() + '  ' + name)
            (directory / 'SHA256SUMS').write_text('\n'.join(sums))
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        output = Path(tmp.name) / 'packages'
        with patch.object(module, 'gh', side_effect=gh), patch.object(module.subprocess, 'run', side_effect=fetch):
            module.download(output, 'example/fork')
        return output

    def test_downloads_only_two_latest_public_stable_releases(self):
        output = self.download()
        self.assertEqual({p.name for p in output.iterdir() if p.is_dir()}, {'0.1.0', '0.2.0'})
        self.assertEqual((output / 'latest-version').read_text(), 'v0.2.0')

    def test_rejects_latest_pointing_to_an_older_version(self):
        with self.assertRaisesRegex(ValueError, 'highest'):
            self.download(latest='v0.1.0')

    def test_rejects_package_checksum_mismatch(self):
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.download(tamper=True)

    def test_verifies_every_package_against_the_release_attestation(self):
        self.download()
        self.assertEqual(sorted(self.verified), sorted((tag, name) for tag in ('v0.1.0', 'v0.2.0') for name in ('Silo-linux-x64.deb', 'Silo-linux-arm64.deb')))

    def test_rejects_a_package_without_a_matching_attestation(self):
        # A replaced package and its rewritten SHA256SUMS still fail attestation.
        with self.assertRaises(subprocess.CalledProcessError):
            self.download(unattested=('v0.1.0',))

    def test_rejects_a_mutable_release(self):
        with self.assertRaisesRegex(ValueError, 'not an immutable release'):
            self.download(mutable=('v0.1.0',))

    def test_rejects_a_mismatched_attestation_for_the_latest_release(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.download(unattested=('v0.2.0',))

    def test_requires_an_attestation_for_the_latest_release(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.download(missing=('v0.2.0',))

    def test_leaves_out_a_predecessor_without_any_attestation(self):
        output = self.download(missing=('v0.1.0',))
        self.assertEqual({p.name for p in output.iterdir() if p.is_dir()}, {'0.2.0'})
        self.assertEqual(sorted(self.verified), [('v0.2.0', 'Silo-linux-arm64.deb'), ('v0.2.0', 'Silo-linux-x64.deb')])
