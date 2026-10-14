"""Offline tests for the guest agent tools recipe: the lock, the download checks and the order of setup."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

GUEST = Path(__file__).parents[1] / 'src-tauri/guest'
SPEC = importlib.util.spec_from_file_location('agent_tools', GUEST / 'agent-tools.py')
tools = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tools)
LOCK = GUEST / 'agent-tools-lock.json'


class AgentToolsTests(unittest.TestCase):
    def test_the_bundled_lock_is_valid_and_pins_official_sources(self):
        lock = tools.read_lock(LOCK)
        self.assertEqual(lock['node']['version'], '24.21.0')
        for arch, name in (('arm64', 'arm64'), ('amd64', 'x64')):
            self.assertTrue(lock['node']['assets'][arch]['url'].startswith('https://nodejs.org/dist/v24.21.0/'))
            self.assertTrue(lock['node']['assets'][arch]['url'].endswith(f'linux-{name}.tar.xz'))

    def test_a_lock_with_another_host_is_refused(self):
        lock = json.loads(LOCK.read_text())
        lock['node']['assets']['arm64']['url'] = 'https://example.com/node.tar.xz'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'lock.json'
            path.write_text(json.dumps(lock))
            with self.assertRaisesRegex(RuntimeError, 'Invalid bundled agent tools lock'):
                tools.read_lock(path)

    def test_a_download_that_does_not_match_its_hash_is_removed(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'file'

            def fake_run(command, **options):
                Path(command[command.index('-o') + 1]).write_bytes(b'tampered')
                return mock.Mock(returncode=0, stderr='')

            with mock.patch.object(tools, 'run', fake_run):
                with self.assertRaisesRegex(RuntimeError, 'does not match'):
                    tools.download('https://nodejs.org/x/file', hashlib.sha256(b'real').hexdigest(), destination)
                tools.download('https://nodejs.org/x/file', hashlib.sha256(b'tampered').hexdigest(), destination)
            self.assertEqual(destination.read_bytes(), b'tampered')
            self.assertEqual(sorted(path.name for path in Path(directory).iterdir()), ['file'])

    def test_download_uses_https_only(self):
        seen = []
        with tempfile.TemporaryDirectory() as directory:
            def fake_run(command, **options):
                seen.append(command)
                Path(command[command.index('-o') + 1]).write_bytes(b'x')
                return mock.Mock(returncode=0, stderr='')
            with mock.patch.object(tools, 'run', fake_run):
                tools.download('https://nodejs.org/x', hashlib.sha256(b'x').hexdigest(), Path(directory) / 'x')
        self.assertIn('--proto', seen[0])
        self.assertEqual(seen[0][seen[0].index('--proto') + 1], '=https')

    def test_archive_members_cannot_escape(self):
        for name in ('/etc/passwd', '../escape', 'a/../../escape'):
            buffer = io.BytesIO()
            with tarfile.open(fileobj=buffer, mode='w') as archive:
                info = tarfile.TarInfo(name)
                archive.addfile(info, io.BytesIO(b''))
            buffer.seek(0)
            with tarfile.open(fileobj=buffer) as archive:
                with self.assertRaises(RuntimeError):
                    tools.safe_members(archive)

    def test_setup_installs_node_then_claude_and_then_writes_the_receipt(self):
        lock = tools.read_lock(LOCK)
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / 'agent-tools.json'
            with (mock.patch.object(tools.os, 'geteuid', return_value=0),
                  mock.patch.object(tools.pwd, 'getpwnam', return_value=mock.Mock()),
                  mock.patch.object(tools, 'RECEIPT', receipt), mock.patch.object(tools, 'STATE', Path(directory)),
                  mock.patch.object(tools, 'install_node', lambda *_: calls.append('node')),
                  mock.patch.object(tools, 'install_claude', lambda *_: calls.append('claude'))):
                tools.setup(lock)
                self.assertEqual(calls, ['node', 'claude'])
                self.assertEqual(json.loads(receipt.read_text())['recipeVersion'], lock['recipeVersion'])

    def test_a_failed_install_leaves_no_receipt(self):
        lock = tools.read_lock(LOCK)
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / 'agent-tools.json'
            with (mock.patch.object(tools.os, 'geteuid', return_value=0),
                  mock.patch.object(tools.pwd, 'getpwnam', return_value=mock.Mock()),
                  mock.patch.object(tools, 'RECEIPT', receipt), mock.patch.object(tools, 'STATE', Path(directory)),
                  mock.patch.object(tools, 'install_node', lambda *_: None),
                  mock.patch.object(tools, 'install_claude', side_effect=RuntimeError('offline'))):
                with self.assertRaises(RuntimeError):
                    tools.setup(lock)
            self.assertFalse(receipt.exists())

    def test_claude_is_installed_by_the_documented_installer_as_the_working_account(self):
        lock = tools.read_lock(LOCK)
        commands = []
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory) / 'home'
            (home / '.local/bin').mkdir(parents=True)
            (home / '.local/bin/claude').write_text('')
            versions = iter([None, '2.1.287'])

            def fake_run(command, **options):
                commands.append(command)
                return mock.Mock(returncode=0, stdout='', stderr='')

            with (mock.patch.object(tools, 'account_home', return_value=home),
                  mock.patch.object(tools, 'BIN', Path(directory) / 'bin'),
                  mock.patch.object(tools, 'claude_version', lambda: next(versions)),
                  mock.patch.object(tools, 'download', lambda url, sha, destination: Path(destination).write_text('')),
                  mock.patch.object(tools, 'run', fake_run)):
                (Path(directory) / 'bin').mkdir()
                tools.install_claude(lock, directory)
        installer = commands[0]
        self.assertEqual(installer[:4], ['runuser', '-u', 'silo', '--'])
        self.assertEqual(installer[-1], '2.1.287')
        self.assertIn('DISABLE_AUTOUPDATER=1', installer)

    def test_status_reports_nothing_without_the_tools(self):
        with mock.patch.object(tools, 'run', side_effect=FileNotFoundError), \
                mock.patch.object(tools, 'RECEIPT', Path('/nonexistent/agent-tools.json')):
            self.assertEqual(tools.status(), {'nodeVersion': None, 'claudeVersion': None, 'recipeVersion': None})


if __name__ == '__main__':
    unittest.main()
