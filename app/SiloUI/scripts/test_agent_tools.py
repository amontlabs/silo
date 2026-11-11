"""Offline tests for the guest agent tools recipe: the lock, the download checks and the order of setup."""
import hashlib
import importlib.util
import io
import json
import os
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
                    tools.download('https://nodejs.org/x/file', hashlib.sha256(b'real').hexdigest(), destination, 1000)
                tools.download('https://nodejs.org/x/file', hashlib.sha256(b'tampered').hexdigest(), destination, 1000)
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
                tools.download('https://nodejs.org/x', hashlib.sha256(b'x').hexdigest(), Path(directory) / 'x', 1000)
        self.assertIn('--proto', seen[0])
        self.assertEqual(seen[0][seen[0].index('--proto') + 1], '=https')
        self.assertEqual(seen[0][seen[0].index('--proto-redir') + 1], '=https')
        self.assertEqual(seen[0][seen[0].index('--max-filesize') + 1], '1000')

    def test_a_download_with_the_wrong_size_or_over_the_maximum_is_removed(self):
        for size, maximum in ((5, 1000), (None, 3)):
            with tempfile.TemporaryDirectory() as directory:
                destination = Path(directory) / 'file'

                def fake_run(command, **options):
                    Path(command[command.index('-o') + 1]).write_bytes(b'tampered')
                    return mock.Mock(returncode=0, stderr='')

                with mock.patch.object(tools, 'run', fake_run):
                    with self.assertRaisesRegex(RuntimeError, 'does not match'):
                        tools.download('https://nodejs.org/x/file', hashlib.sha256(b'tampered').hexdigest(),
                                       destination, maximum, size)
                self.assertEqual(list(Path(directory).iterdir()), [])
                with mock.patch.object(tools, 'run', fake_run):
                    tools.download('https://nodejs.org/x/file', hashlib.sha256(b'tampered').hexdigest(),
                                   destination, 1000, 8)
                self.assertEqual(destination.read_bytes(), b'tampered')

    def test_the_downloader_cannot_write_more_than_the_maximum(self):
        seen = {}

        def fake_run(command, **options):
            seen.update(options)
            Path(command[command.index('-o') + 1]).write_bytes(b'x')
            return mock.Mock(returncode=0, stderr='')

        with tempfile.TemporaryDirectory() as directory, mock.patch.object(tools, 'run', fake_run):
            tools.download('https://nodejs.org/x', hashlib.sha256(b'x').hexdigest(), Path(directory) / 'x', 1000)
        with mock.patch.object(tools.resource, 'setrlimit') as setrlimit:
            seen['preexec_fn']()
        setrlimit.assert_called_once_with(tools.resource.RLIMIT_FSIZE, (1001, 1001))

    def test_versions_are_whole_words_and_compare_as_dotted_numbers(self):
        self.assertEqual(tools.first_version('codex-cli 0.167.0\n'), '0.167.0')
        self.assertEqual(tools.first_version('v24.21.0'), '24.21.0')
        self.assertEqual(tools.first_version('2.1.287 (Claude Code)'), '2.1.287')
        for text in ('0.167.0-beta', '10.167.0.1', 'x0.167.0', '', None):
            self.assertIsNone(tools.first_version(text))
        for installed, pinned, expected in (('0.167.0', '0.167.0', True), ('0.168.0', '0.167.0', True),
                                            ('0.10.0', '0.9.0', True), ('0.166.9', '0.167.0', False),
                                            ('0.167', '0.167.0', True), ('abc', '0.167.0', False),
                                            ('abc', 'abc', True), ('0.167.0-x', '0.167.0', False)):
            self.assertEqual(tools.not_older(installed, pinned), expected, (installed, pinned))

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
                  mock.patch.object(tools, 'download', lambda url, sha, destination, *rest: Path(destination).write_text('')),
                  mock.patch.object(tools, 'run', fake_run)):
                (Path(directory) / 'bin').mkdir()
                tools.install_claude(lock, directory)
        installer = commands[0]
        self.assertEqual(installer[:4], ['runuser', '-u', 'silo', '--'])
        self.assertEqual(installer[-1], '2.1.287')
        self.assertIn('DISABLE_AUTOUPDATER=1', installer)

    def test_the_lock_pins_codex_from_the_official_release_for_each_platform(self):
        lock = tools.read_lock(LOCK)
        version = lock['codex']['version']
        self.assertEqual(set(lock['codex']['assets']), {'linux-arm64', 'linux-amd64', 'macos-arm64'})
        for asset in lock['codex']['assets'].values():
            self.assertTrue(asset['url'].startswith(f'https://github.com/openai/codex/releases/download/rust-v{version}/'))
            self.assertRegex(asset['sha256'], '^[0-9a-f]{64}$')

    def test_a_codex_asset_from_another_place_or_without_a_hash_is_refused(self):
        for change in (lambda asset: asset.update(url='https://example.com/codex.tar.gz'),
                       lambda asset: asset.update(sha256='abc'),
                       lambda asset: asset.update(member='codex')):
            lock = json.loads(LOCK.read_text())
            change(lock['codex']['assets']['linux-arm64'])
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'lock.json'
                path.write_text(json.dumps(lock))
                with self.assertRaisesRegex(RuntimeError, 'Invalid bundled agent tools lock'):
                    tools.read_lock(path)

    def codex_archive(self, directory, member):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
            info = tarfile.TarInfo(member)
            info.size = 4
            info.mode = 0o755
            archive.addfile(info, io.BytesIO(b'ELF!'))
        return buffer.getvalue()

    def install_codex_from(self, data, lock, directory, versions):
        bin_dir = Path(directory) / 'bin'
        bin_dir.mkdir()
        downloads = self.downloads = []

        def fake_download(url, sha, destination, maximum, size=None):
            downloads.append((maximum, size))
            Path(destination).write_bytes(data)

        with (mock.patch.object(tools, 'CODEX_ROOT', Path(directory) / 'codex'),
              mock.patch.object(tools, 'BIN', bin_dir),
              mock.patch.object(tools.platform, 'machine', return_value='aarch64'),
              mock.patch.object(tools, 'codex_version', lambda: next(versions)),
              mock.patch.object(tools, 'download', fake_download)):
            tools.install_codex(lock, directory)
        return bin_dir

    def test_codex_is_unpacked_to_a_versioned_folder_and_linked(self):
        lock = tools.read_lock(LOCK)
        member = lock['codex']['assets']['linux-arm64']['member']
        with tempfile.TemporaryDirectory() as directory:
            bin_dir = self.install_codex_from(self.codex_archive(directory, member), lock, directory,
                                              iter([None, lock['codex']['version']]))
            binary = Path(directory) / 'codex' / lock['codex']['version'] / 'codex'
            self.assertEqual(binary.read_bytes(), b'ELF!')
            self.assertTrue(binary.stat().st_mode & 0o111)
            self.assertEqual((bin_dir / 'codex').resolve(), binary.resolve())
            asset = lock['codex']['assets']['linux-arm64']
            self.assertEqual(self.downloads, [(asset['bytes'], asset['bytes'])])

    def test_the_codex_folders_can_be_entered_and_the_program_run_by_the_working_account(self):
        lock = tools.read_lock(LOCK)
        member = lock['codex']['assets']['linux-arm64']['member']
        with tempfile.TemporaryDirectory() as directory:
            old = os.umask(0o077)
            try:
                self.install_codex_from(self.codex_archive(directory, member), lock, directory,
                                        iter([None, lock['codex']['version']]))
            finally:
                os.umask(old)
            root = Path(directory) / 'codex'
            version = root / lock['codex']['version']
            for folder in (root, version):
                self.assertEqual(folder.stat().st_mode & 0o755, 0o755, folder)
                self.assertFalse(folder.stat().st_mode & 0o022, f'{folder} is writable by others')
            self.assertEqual((version / 'codex').stat().st_mode & 0o755, 0o755)

    def test_a_newer_codex_stays_and_an_older_one_is_replaced(self):
        lock = tools.read_lock(LOCK)
        member = lock['codex']['assets']['linux-arm64']['member']
        with tempfile.TemporaryDirectory() as directory:
            self.install_codex_from(self.codex_archive(directory, member), lock, directory, iter(['99.0.0']))
            self.assertEqual(self.downloads, [])
            self.assertFalse((Path(directory) / 'codex').exists())
        with tempfile.TemporaryDirectory() as directory:
            self.install_codex_from(self.codex_archive(directory, member), lock, directory,
                                    iter(['0.1.0', lock['codex']['version']]))
            self.assertEqual(len(self.downloads), 1)

    def test_a_newer_claude_is_not_downgraded(self):
        lock = tools.read_lock(LOCK)
        with mock.patch.object(tools, 'claude_version', return_value='99.0.0'), \
                mock.patch.object(tools, 'download') as download:
            tools.install_claude(lock, '/nonexistent')
        download.assert_not_called()

    def test_a_codex_archive_with_another_file_is_refused_and_leaves_nothing(self):
        lock = tools.read_lock(LOCK)
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'expected file'):
                self.install_codex_from(self.codex_archive(directory, '../escape'), lock, directory, iter([None]))
            self.assertEqual(list((Path(directory) / 'codex').iterdir()), [])

    def test_codex_that_reports_another_version_fails_the_install(self):
        lock = tools.read_lock(LOCK)
        member = lock['codex']['assets']['linux-arm64']['member']
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'pinned version'):
                self.install_codex_from(self.codex_archive(directory, member), lock, directory, iter([None, '0.0.1']))

    def test_setting_up_codex_installs_only_codex_and_keeps_the_other_receipt_fields(self):
        lock = tools.read_lock(LOCK)
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / 'agent-tools.json'
            receipt.write_text(json.dumps({'schemaVersion': 1, 'recipeVersion': 1, 'claudeVersion': '2.1.287'}))
            with (mock.patch.object(tools.os, 'geteuid', return_value=0),
                  mock.patch.object(tools.pwd, 'getpwnam', return_value=mock.Mock()),
                  mock.patch.object(tools, 'RECEIPT', receipt), mock.patch.object(tools, 'STATE', Path(directory)),
                  mock.patch.object(tools, 'install_node', lambda *_: calls.append('node')),
                  mock.patch.object(tools, 'install_claude', lambda *_: calls.append('claude')),
                  mock.patch.object(tools, 'install_codex', lambda *_: calls.append('codex'))):
                tools.setup(lock, 'codex')
            saved = json.loads(receipt.read_text())
        self.assertEqual(calls, ['codex'])
        self.assertEqual(saved['codexVersion'], lock['codex']['version'])
        self.assertEqual(saved['claudeVersion'], '2.1.287')

    def test_the_command_line_takes_an_agent_and_refuses_an_unknown_one(self):
        with mock.patch.object(tools, 'setup') as setup, mock.patch.object(tools, 'status', return_value={}):
            self.assertEqual(tools.main(['x', str(LOCK), 'setup']), 0)
            self.assertEqual(setup.call_args.args[1], 'claude-code')
            self.assertEqual(tools.main(['x', str(LOCK), 'setup', 'codex']), 0)
            self.assertEqual(setup.call_args.args[1], 'codex')
            self.assertEqual(tools.main(['x', str(LOCK), 'setup', 'other']), 2)

    def test_status_reports_nothing_without_the_tools(self):
        with mock.patch.object(tools, 'run', side_effect=FileNotFoundError), \
                mock.patch.object(tools, 'RECEIPT', Path('/nonexistent/agent-tools.json')):
            self.assertEqual(tools.status(), {'nodeVersion': None, 'claudeVersion': None, 'codexVersion': None,
                                              'recipeVersion': None})


if __name__ == '__main__':
    unittest.main()
