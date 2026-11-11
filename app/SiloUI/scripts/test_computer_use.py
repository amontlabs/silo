import contextlib
import fcntl
import hashlib
import importlib.machinery
import importlib.util
import io
import json
import os
import re
import stat
import subprocess
import tarfile
import tempfile
import time
import tomllib
import unittest
from pathlib import Path
from unittest import mock


class ScriptTime:
    def __init__(self, sleeps):
        self.sleeps = sleeps

    def sleep(self, seconds):
        self.sleeps.append(seconds)

    def __getattr__(self, name):
        return getattr(time, name)


SCRIPT = Path(__file__).parents[1] / 'src-tauri/guest/silo-computer-use.py'
LOCK_FILE = Path(__file__).parents[1] / 'src-tauri/guest/cua-driver-lock.json'
SPEC = importlib.util.spec_from_file_location('silo_computer_use', SCRIPT)
cu = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cu)
REAL_RUN = cu.run
REAL_BINARY_INSTALLED = cu.binary_installed

VERSION = '0.38.0'
RELEASE = f'https://github.com/trycua/cua/releases/download/cua-driver-rs-v{VERSION}/'
# The fixture binary is a file holding its version, so the pinned binary's digest is that of VERSION.
BINARY_SHA256 = hashlib.sha256(VERSION.encode()).hexdigest()
SHIM_FILE = Path(__file__).parents[1] / 'src-tauri/guest/cua-driver-shim'
SESSION = {'DISPLAY': ':1', 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/tmp/dbus-AAAA',
           'XDG_RUNTIME_DIR': '/run/silo-desktop/user'}
DOCTOR_OK = {'ok': True, 'probes': [{'label': 'binary', 'status': 'ok'},
                                    {'label': 'display server', 'status': 'ok'},
                                    {'label': 'X11 connection', 'status': 'ok'},
                                    {'label': 'AT-SPI', 'status': 'ok'}]}


def make_archive(directory, version=VERSION, members=None):
    """A release archive like Cua Driver's: the binary (here a file holding its version) beside
    files that must never be extracted."""
    path = Path(directory) / 'driver.tar.gz'
    files = members or (('cua-driver', version.encode(), 0o755), ('cua-cursor-theme', b'x', 0o755),
                        ('libcua_driver_sdk.so', b'x', 0o755), ('wayland-helper/install.sh', b'x', 0o755))
    with tarfile.open(path, 'w:gz') as archive:
        for name, body, mode in files:
            info = tarfile.TarInfo(name)
            info.size = len(body)
            info.mode = mode
            archive.addfile(info, io.BytesIO(body))
    return path, hashlib.sha256(path.read_bytes()).hexdigest(), path.stat().st_size


class Guest(unittest.TestCase):
    """A fake guest: temp state and agent files, a scripted `cua-driver` and desktop."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = self.root = Path(self.tmp.name)
        self.state = root / 'state'
        self.state.mkdir()
        self.link = root / 'bin/cua-driver'
        self.link.parent.mkdir()
        self.served_dir = root / 'served'
        self.served_dir.mkdir()
        self.served, self.sha, self.size = make_archive(self.served_dir)
        self.commands = []
        self.sleeps = []
        self.curl_failures_left = 0
        self.session = True
        self.start_effect = None
        self.session_environ = dict(SESSION)
        self.installed_agents = {'claude', 'codex'}
        self.doctor = dict(DOCTOR_OK)
        self.doctor_code = 0
        self.spawned = []
        self.hook = root / 'profile-hook.sh'
        self.claude_state = root / 'home/.claude.json'
        self.claude_settings = root / 'home/.claude/settings.json'
        self.codex = root / 'home/.codex/config.toml'
        self.legacy_prefix = root / 'opt-lcu'
        self.legacy_receipt = root / 'lcu.json'
        patches = [
            mock.patch.object(cu, 'PROFILE_HOOK', self.hook),
            mock.patch.object(cu, 'PRIVACY_PROFILE', root / 'profile-cua-driver.sh'),
            mock.patch.object(cu, 'SYSTEM_ENVIRONMENT', root / 'environment'),
            mock.patch.object(cu, 'WATCH_LOCK', self.state / 'watch.lock'),
            mock.patch.object(cu, 'spawn_watcher', lambda: self.spawned.append(True)),
            mock.patch.object(cu, 'STATE', self.state),
            mock.patch.object(cu, 'PINNED', self.state / 'pinned.json'),
            mock.patch.object(cu, 'RECEIPT', self.state / 'receipt.json'),
            mock.patch.object(cu, 'LOCK', self.state / 'lock'),
            mock.patch.object(cu, 'STAGE', self.state / 'stage'),
            mock.patch.object(cu, 'LOG', root / 'log'),
            mock.patch.object(cu, 'DRIVER_LINK', self.link),
            mock.patch.object(cu, 'LAUNCHER', root / 'libexec/silo-cua-driver-mcp'),
            mock.patch.object(cu, 'INSTALL_ROOT', root / 'opt-cua-driver'),
            mock.patch.object(cu, 'IMAGE_ROOT', root / 'image-cua-driver'),
            mock.patch.object(cu, 'LEGACY_PREFIX', self.legacy_prefix),
            mock.patch.object(cu, 'LEGACY_RECEIPT', self.legacy_receipt),
            mock.patch.object(cu, 'CLAUDE_STATE', self.claude_state),
            mock.patch.object(cu, 'CLAUDE_SETTINGS', self.claude_settings),
            mock.patch.object(cu, 'CODEX_CONFIG', self.codex),
            mock.patch.object(cu, 'LEGACY_DATA', root / 'home/.local/share/lcu'),
            mock.patch.object(cu, 'LEGACY_SKILL_DIRS', [root / 'home' / d for d in
                                                        ('.claude/skills', '.codex/skills', '.agents/skills',
                                                         '.pi/agent/skills')]),
            mock.patch.object(cu, 'PI_SETTINGS', root / 'home/.pi/agent/settings.json'),
            mock.patch.object(cu, 'AGENT_BIN_GLOBS', [str(root / 'home/.nvm/versions/node/*/bin')]),
            mock.patch.object(cu, 'USER_BIN', [str(root / 'home/.local/bin')]),
            mock.patch.object(cu, 'as_user', contextlib.nullcontext),
            mock.patch.object(cu, 'architecture', lambda: 'amd64'),
            mock.patch.object(cu, 'binary_installed', lambda name, directories=None: name in self.installed_agents),
            mock.patch.object(cu, 'session_env', self.fake_session_env),
            mock.patch.object(cu, 'desktop_session', self.fake_session),
            mock.patch.object(cu, 'DESKTOP_CONFIG', root / 'desktop-config.json'),
            mock.patch.object(cu, 'run', self.fake_run),
            # Files written by the tests are owned by the test user, not root.
            mock.patch.object(cu, 'read_json', self.read_json),
            # Only the script's own waits are recorded; subprocess polling keeps the real sleep.
            mock.patch.object(cu, 'time', ScriptTime(self.sleeps)),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.write_pinned()
        (root / 'desktop-config.json').write_text(json.dumps({'autoStart': True}))

    def fake_session_env(self, strict=False):
        if strict and 'DBUS_SESSION_BUS_ADDRESS' not in self.session_environ:
            raise cu.Failure('session-unavailable')
        return dict(self.session_environ)

    def fake_session(self):
        value = self.session
        if isinstance(value, list):
            return value.pop(0) if len(value) > 1 else value[0]
        if value is True:
            return 'running'
        return 'starting' if value is False else value

    @staticmethod
    def read_json(path):
        try:
            value = json.loads(Path(path).read_text())
        except (OSError, ValueError):
            return None
        return value if isinstance(value, dict) else None

    def write_pinned(self, **changes):
        asset = {'url': RELEASE + 'driver.tar.gz', 'sha256': self.sha, 'bytes': self.size,
                 'binarySha256': BINARY_SHA256}
        pinned = {'schemaVersion': 1, 'version': VERSION, 'source': 'https://example.invalid/',
                  'assets': {'arm64': dict(asset), 'amd64': dict(asset)}}
        pinned.update(changes)
        # Assets a test passes without the binary digest get the pinned one.
        for value in (pinned.get('assets') or {}).values() if isinstance(pinned.get('assets'), dict) else ():
            if isinstance(value, dict):
                value.setdefault('binarySha256', BINARY_SHA256)
        (self.state / 'pinned.json').write_text(json.dumps(pinned))

    def install_image_driver(self, version=VERSION):
        """What the v5 image ships: the binary under IMAGE_ROOT and the shim that runs it."""
        binary = self.root / 'image-cua-driver' / version / 'cua-driver'
        binary.parent.mkdir(parents=True)
        binary.write_text(version)
        self.link.write_text(cu.shim_text(binary))
        self.link.chmod(0o755)
        return binary

    def shim_target(self):
        """The binary the installed shim runs."""
        text = self.link.read_text()
        self.assertFalse(self.link.is_symlink())
        self.assertTrue(os.access(self.link, os.X_OK))
        match = re.search(r'^exec (\S+) "\$@"$', text, re.MULTILINE)
        self.assertEqual(text, cu.shim_text(match.group(1)))
        return Path(match.group(1))

    def fake_run(self, argv, *, user=False, timeout=900, check=True, cwd=None, extra_env=None, quiet=False):
        self.commands.append((list(argv), bool(user), dict(extra_env or {})))
        name = Path(argv[0]).name
        if name == 'cua-driver' or argv[0] == str(self.link) or Path(argv[0]).name == 'cua-driver':
            path = Path(argv[0])
            if path.is_file() and path.read_text().startswith('#!'):
                # The shim: run the binary it names.
                path = Path(re.search(r'^exec (\S+) "\$@"$', path.read_text(), re.MULTILINE).group(1))
            if argv[1:2] == ['--version']:
                if not path.exists():
                    return subprocess.CompletedProcess(argv, 127, stdout='', stderr='')
                return subprocess.CompletedProcess(argv, 0, stdout=f'cua-driver {path.read_text()}\n', stderr='')
            if argv[1:3] == ['doctor', '--json']:
                return subprocess.CompletedProcess(argv, self.doctor_code, stdout=json.dumps(self.doctor), stderr='')
        if name == 'silo-desktop':
            if argv[1:2] == ['start'] and self.start_effect is not None:
                self.session = self.start_effect
            return subprocess.CompletedProcess(argv, 0, stdout='', stderr='')
        if name == 'curl' and self.curl_failures_left > 0:
            self.curl_failures_left -= 1
            raise cu.Failure('command-failed', 'curl exited 52')
        if name == 'curl':
            Path(argv[argv.index('--output') + 1]).write_bytes(self.served.read_bytes())
        return subprocess.CompletedProcess(argv, 0, stdout='', stderr='')

    def receipt(self):
        return json.loads((self.state / 'receipt.json').read_text())

    def curls(self):
        return [argv for argv, *_ in self.commands if argv[0] == 'curl']

    def doctors(self):
        return [argv for argv, *_ in self.commands if argv[1:2] == ['doctor']]

    def desktop_starts(self):
        return [argv for argv, *_ in self.commands if argv[:2] == [cu.DESKTOP_COMMAND, 'start']]

    def claude(self):
        return json.loads(self.claude_state.read_text())

    def allowed(self):
        return json.loads(self.claude_settings.read_text()).get('permissions', {}).get('allow', [])

    def codex_config(self):
        return tomllib.loads(self.codex.read_text())


class Lock(unittest.TestCase):
    def test_the_bundled_lock_is_a_valid_pin(self):
        lock = json.loads(LOCK_FILE.read_text())
        self.assertEqual((lock['schemaVersion'], lock['version']), (1, '0.38.0'))
        for arch, name in (('arm64', 'arm64'), ('amd64', 'x86_64')):
            asset = lock['assets'][arch]
            self.assertRegex(asset['sha256'], r'^[0-9a-f]{64}$')
            self.assertTrue(asset['url'].startswith('https://github.com/trycua/cua/releases/download/cua-driver-rs-v0.38.0/'))
            self.assertTrue(asset['url'].endswith(f'linux-{name}-binary.tar.gz'))
            self.assertGreater(asset['bytes'], 0)

    def test_the_bundled_lock_is_accepted_by_the_helper(self):
        with tempfile.TemporaryDirectory() as directory:
            pinned = Path(directory) / 'pinned.json'
            pinned.write_text(LOCK_FILE.read_text())
            with mock.patch.object(cu, 'PINNED', pinned), mock.patch.object(cu, 'read_json', Guest.read_json):
                self.assertEqual(cu.load_pinned()['version'], '0.38.0')


class Status(Guest):
    def test_without_the_pinned_lock_nothing_is_configured(self):
        (self.state / 'pinned.json').unlink()
        self.assertEqual(cu.status(), {
            'schemaVersion': 2, 'state': 'not-set-up', 'reason': 'not-configured', 'driverVersion': None,
            'approval': None, 'approvalOutcome': None, 'agents': None, 'pending': None, 'verifiedAt': None})

    def test_a_malformed_lock_is_ignored(self):
        for change in ({'schemaVersion': 2}, {'version': '$(id)'}, {'assets': {}},
                       {'assets': {'arm64': {'url': 'http://x/', 'sha256': self.sha, 'bytes': 1},
                                   'amd64': {'url': 'https://x/', 'sha256': self.sha, 'bytes': 1}}},
                       {'assets': {'arm64': {'url': RELEASE + 'x', 'sha256': 'abc', 'bytes': 1},
                                   'amd64': {'url': RELEASE + 'x', 'sha256': self.sha, 'bytes': 1}}},
                       # Only the driver's own GitHub release, for the pinned version.
                       {'assets': {'arm64': {'url': 'https://example.invalid/d.tar.gz', 'sha256': self.sha, 'bytes': 1},
                                   'amd64': {'url': RELEASE + 'x', 'sha256': self.sha, 'bytes': 1}}},
                       {'assets': {'arm64': {'url': RELEASE.replace(VERSION, '0.30.0') + 'x', 'sha256': self.sha,
                                             'bytes': 1},
                                   'amd64': {'url': RELEASE + 'x', 'sha256': self.sha, 'bytes': 1}}}):
            self.write_pinned(**change)
            self.assertEqual(cu.status()['reason'], 'not-configured', change)

    def test_status_reads_the_receipt_only(self):
        self.assertEqual(cu.status()['state'], 'not-set-up')
        cu.write_receipt(cu.load_pinned(), 'auto', 'ready', agents=['codex'], pending=['claude-code'],
                         approvalOutcome='applied', verifiedAt=7)
        self.commands.clear()
        status = cu.status()
        self.assertEqual(status, {
            'schemaVersion': 2, 'state': 'ready', 'reason': None, 'driverVersion': VERSION,
            'approval': 'auto', 'approvalOutcome': 'applied', 'agents': ['codex'],
            'pending': ['claude-code'], 'verifiedAt': 7})
        self.assertEqual(self.commands, [])

    def test_an_old_schema_receipt_counts_as_not_set_up(self):
        (self.state / 'receipt.json').write_text(json.dumps(
            {'schemaVersion': 1, 'state': 'ready', 'lcuVersion': '0.11.0', 'appDir': 'x'}))
        self.assertEqual(cu.status()['state'], 'not-set-up')

    def test_a_receipt_for_another_driver_version_does_not_count(self):
        cu.write_receipt(cu.load_pinned(), 'ask', 'ready')
        self.write_pinned(version='0.39.0')
        self.assertEqual((cu.status()['state'], cu.status()['driverVersion']), ('not-set-up', None))

    def test_a_held_lock_means_installing_and_a_stale_installing_receipt_failed(self):
        cu.write_receipt(cu.load_pinned(), 'ask', 'installing')
        self.assertEqual((cu.status()['state'], cu.status()['reason']), ('failed', 'interrupted'))
        with open(self.state / 'lock', 'a') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(cu.status()['state'], 'installing')

    def test_hostile_receipt_fields_are_dropped(self):
        cu.write_receipt(cu.load_pinned(), 'ask', 'ready', agents=['codex', '$(id)', 5], pending='x',
                         approvalOutcome='odd', verifiedAt='soon')
        status = cu.status()
        self.assertEqual((status['agents'], status['pending'], status['approvalOutcome'], status['verifiedAt']),
                         (['codex'], None, None, None))


class Driver(Guest):
    def test_the_image_driver_at_the_pinned_version_is_used_without_a_download(self):
        self.install_image_driver()
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['driverVersion']), ('ready', VERSION))
        self.assertEqual(self.curls(), [])
        self.assertEqual(self.shim_target(), self.root / 'image-cua-driver' / VERSION / 'cua-driver')

    def test_without_the_driver_the_pinned_archive_is_downloaded_verified_and_linked(self):
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'ready')
        curl, = self.curls()
        self.assertEqual(curl[curl.index('--proto') + 1], '=https')
        self.assertIn(RELEASE + 'driver.tar.gz', curl)
        installed = self.root / 'opt-cua-driver' / VERSION
        # Only the binary is extracted.
        self.assertEqual([p.name for p in installed.iterdir()], ['cua-driver'])
        self.assertEqual(self.shim_target(), installed / 'cua-driver')
        self.assertFalse((self.state / 'stage').exists())

    def test_an_image_driver_of_another_version_is_replaced_by_the_pinned_one(self):
        self.install_image_driver('0.30.0')
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(len(self.curls()), 1)
        self.assertEqual(self.shim_target(), self.root / 'opt-cua-driver' / VERSION / 'cua-driver')

    def test_an_earlier_install_of_the_pinned_version_is_linked_without_a_download(self):
        cu.apply('ask')
        self.link.unlink()
        self.commands.clear()
        self.assertEqual(cu.apply('ask', force=True)['state'], 'ready')
        self.assertEqual(self.curls(), [])
        self.assertEqual(self.shim_target(), self.root / 'opt-cua-driver' / VERSION / 'cua-driver')

    def test_the_shim_matches_the_shipped_file_and_turns_telemetry_off(self):
        self.assertEqual(cu.SHIM_TEMPLATE, SHIM_FILE.read_text())
        text = cu.shim_text('/opt/cua-driver/0.38.0/cua-driver')
        self.assertIn('export CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false DO_NOT_TRACK=1\n', text)
        self.assertTrue(text.endswith('exec /opt/cua-driver/0.38.0/cua-driver "$@"\n'))
        # The shim runs: the binary it names sees the switches whatever the caller's environment.
        binary = self.root / 'real-driver'
        binary.write_text('#!/bin/sh\necho "$CUA_DRIVER_RS_TELEMETRY_ENABLED $CUA_DRIVER_RS_UPDATE_CHECK $DO_NOT_TRACK $*"\n')
        binary.chmod(0o755)
        shim = self.root / 'shim'
        shim.write_text(cu.shim_text(binary))
        shim.chmod(0o755)
        output = subprocess.run([str(shim), 'doctor', '--json'], capture_output=True, text=True, check=True,
                                env=dict(os.environ, CUA_DRIVER_RS_TELEMETRY_ENABLED='1')).stdout
        self.assertEqual(output, '0 false 1 doctor --json\n')

    def test_a_symlink_left_by_an_earlier_helper_is_replaced_by_the_shim(self):
        installed = self.root / 'opt-cua-driver' / VERSION / 'cua-driver'
        installed.parent.mkdir(parents=True)
        installed.write_text(VERSION)
        self.link.symlink_to(installed)
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(self.curls(), [])
        self.assertEqual(self.shim_target(), installed)
        # A configured computer that still has the symlink gets the shim on its next apply too.
        self.link.unlink()
        self.link.symlink_to(installed)
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(self.shim_target(), installed)

    def test_an_unchanged_shim_is_not_rewritten(self):
        self.install_image_driver()
        cu.apply('ask')
        before = self.link.stat().st_ino
        cu.apply('ask', force=True)
        self.assertEqual(self.link.stat().st_ino, before)

    def test_the_driver_runs_as_the_working_account_with_telemetry_off(self):
        self.install_image_driver()
        cu.apply('ask')
        runs = [(user, env) for argv, user, env in self.commands if argv[1:2] in (['--version'], ['doctor'])]
        self.assertTrue(runs)
        for user, env in runs:
            self.assertTrue(user)
            self.assertEqual(env['CUA_DRIVER_RS_TELEMETRY_ENABLED'], '0')
            self.assertEqual(env['CUA_DRIVER_RS_UPDATE_CHECK'], 'false')

    def test_an_archive_that_cannot_be_downloaded_fails_clearly_and_is_retryable(self):
        self.curl_failures_left = 99
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'driver-archive-unavailable'))
        self.assertEqual(result['apply'], {'approval': 'ask', 'outcome': 'failed', 'reason': 'driver-archive-unavailable'})
        self.assertEqual(len(self.curls()), 5)
        self.assertEqual(self.sleeps, [5, 10, 20, 40])

    def test_a_download_that_fails_a_few_times_is_retried_with_backoff_and_logged(self):
        self.curl_failures_left = 3
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(len(self.curls()), 4)
        self.assertEqual(self.sleeps, [5, 10, 20])
        log = cu.LOG.read_text()
        self.assertIn('attempt 4 of 5', log)
        self.assertIn('driver download attempt 1 failed', log)

    def test_while_creating_nothing_is_downloaded(self):
        with mock.patch.object(cu, 'CREATION', True):
            result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['reason']), ('failed', 'driver-archive-unavailable'))
        self.assertEqual(self.curls(), [])
        self.assertEqual(self.sleeps, [])

    def test_while_creating_the_image_driver_is_used(self):
        self.install_image_driver()
        with mock.patch.object(cu, 'CREATION', True):
            self.assertEqual(cu.apply('ask', boot=True)['state'], 'ready')
        self.assertEqual(self.curls(), [])

    def test_the_creation_flag_is_parsed(self):
        with mock.patch.object(cu, 'apply', return_value={}) as apply, \
                mock.patch.object(cu.os, 'geteuid', return_value=0), \
                mock.patch('builtins.print'):
            cu.main(['apply', '--approval', 'ask', '--boot', '--creation'])
            self.assertTrue(cu.CREATION)
            cu.main(['apply', '--approval', 'ask'])
            self.assertFalse(cu.CREATION)
        self.assertEqual(apply.call_count, 2)

    def test_the_download_uses_curls_own_retry_options(self):
        cu.apply('ask')
        curl, = self.curls()
        for option in ('--retry-all-errors', '--retry-connrefused', '--connect-timeout', '--max-time'):
            self.assertIn(option, curl)

    def test_a_hash_mismatch_is_refused_and_never_retried(self):
        asset = {'url': RELEASE + 'd.tar.gz', 'sha256': '0' * 64, 'bytes': self.size}
        self.write_pinned(assets={'arm64': asset, 'amd64': asset})
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'driver-archive-mismatch'))
        self.assertEqual(len(self.curls()), 1)
        self.assertEqual(self.sleeps, [])
        self.assertFalse(self.link.exists() or self.link.is_symlink())

    def test_a_size_mismatch_is_refused(self):
        asset = {'url': RELEASE + 'd.tar.gz', 'sha256': self.sha, 'bytes': self.size + 1}
        self.write_pinned(assets={'arm64': asset, 'amd64': asset})
        self.assertEqual(cu.apply('ask')['reason'], 'driver-archive-mismatch')

    def test_unsafe_archive_members_are_refused(self):
        for name in ('../escape', '/abs/cua-driver'):
            bad, _, _ = make_archive(self.root, members=((name, b'x', 0o755), ('cua-driver', b'0.38.0', 0o755)))
            with self.assertRaises(cu.Failure) as caught:
                cu.extract_driver(bad, self.root / 'out')
            self.assertEqual(caught.exception.reason, 'driver-archive-invalid')
            self.assertFalse((self.root / 'out').exists())

    def test_an_archive_without_the_driver_or_with_a_link_for_it_is_invalid(self):
        missing, _, _ = make_archive(self.root, members=(('other', b'x', 0o755),))
        with self.assertRaises(cu.Failure) as caught:
            cu.extract_driver(missing, self.root / 'out')
        self.assertEqual(caught.exception.reason, 'driver-archive-invalid')
        linked = self.root / 'linked.tar.gz'
        with tarfile.open(linked, 'w:gz') as archive:
            info = tarfile.TarInfo('cua-driver')
            info.type = tarfile.SYMTYPE
            info.linkname = '/etc/passwd'
            archive.addfile(info)
        with self.assertRaises(cu.Failure) as caught:
            cu.extract_driver(linked, self.root / 'out')
        self.assertEqual(caught.exception.reason, 'driver-archive-invalid')
        junk = self.root / 'junk.tar.gz'
        junk.write_bytes(b'not an archive')
        with self.assertRaises(cu.Failure):
            cu.extract_driver(junk, self.root / 'out')

    def test_a_downloaded_driver_of_the_wrong_version_is_not_linked(self):
        wrong, sha, size = make_archive(self.root, version='0.1.0')
        self.served = wrong
        asset = {'url': RELEASE + 'd.tar.gz', 'sha256': sha, 'bytes': size}
        self.write_pinned(assets={'arm64': asset, 'amd64': asset})
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'driver-archive-invalid'))
        self.assertFalse(self.link.exists() or self.link.is_symlink())

    def test_a_lock_without_a_valid_binary_digest_is_ignored(self):
        for digest in (None, 'abc'):
            asset = {'url': RELEASE + 'd.tar.gz', 'sha256': self.sha, 'bytes': self.size, 'binarySha256': digest}
            if digest is None:
                del asset['binarySha256']
                pinned = json.loads((self.state / 'pinned.json').read_text())
                pinned['assets'] = {'arm64': asset, 'amd64': asset}
                (self.state / 'pinned.json').write_text(json.dumps(pinned))
            else:
                self.write_pinned(assets={'arm64': asset, 'amd64': asset})
            self.assertEqual(cu.status()['reason'], 'not-configured', digest)

    def test_an_archive_whose_binary_is_not_the_pinned_one_is_refused(self):
        # Same version string, other bytes: the archive matches its lock entry, the binary does not.
        other, sha, size = make_archive(self.root, members=(('cua-driver', b'0.38.0\n', 0o755),))
        self.served = other
        self.write_pinned(assets={a: {'url': RELEASE + 'd.tar.gz', 'sha256': sha, 'bytes': size} for a in ('arm64', 'amd64')})
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'driver-archive-invalid'))
        self.assertFalse(self.link.exists() or self.link.is_symlink())
        self.assertFalse((self.root / 'opt-cua-driver' / VERSION / 'cua-driver').exists())

    def test_a_tampered_image_or_earlier_binary_is_not_used(self):
        binary = self.install_image_driver()
        binary.write_text(VERSION + '\n')
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(len(self.curls()), 1)
        self.assertEqual(self.shim_target(), self.root / 'opt-cua-driver' / VERSION / 'cua-driver')

    def test_an_unsupported_architecture_has_no_driver(self):
        with mock.patch.object(cu, 'architecture', lambda: None):
            self.assertEqual(cu.apply('ask')['reason'], 'driver-archive-unavailable')


class Apply(Guest):
    def setUp(self):
        super().setUp()
        self.install_image_driver()

    def test_first_apply_registers_checks_the_doctor_and_writes_a_ready_receipt(self):
        with mock.patch.object(cu, 'now', return_value=99):
            result = cu.apply('ask')
        self.assertEqual(result['apply'], {'approval': 'ask', 'outcome': 'applied', 'reason': None})
        self.assertEqual(result, {
            'schemaVersion': 2, 'state': 'ready', 'reason': None, 'driverVersion': VERSION,
            'approval': 'ask', 'approvalOutcome': 'applied', 'agents': ['claude-code', 'codex'],
            'pending': [], 'verifiedAt': 99, 'apply': result['apply']})
        self.assertEqual(len(self.doctors()), 1)

    def test_an_up_to_date_computer_runs_only_a_version_check_between_applies(self):
        cu.apply('ask')
        self.commands.clear()
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'ready')
        self.assertEqual([argv[1:] for argv, *_ in self.commands], [['--version']])

    def test_a_boot_reverifies_without_downloading_and_updates_verified_at(self):
        cu.apply('ask')
        self.commands.clear()
        with mock.patch.object(cu, 'now', return_value=123456):
            result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['apply']['outcome']), ('ready', 'applied'))
        self.assertEqual(self.receipt()['verifiedAt'], 123456)
        self.assertEqual(len(self.doctors()), 1)
        self.assertEqual(self.curls(), [])

    def test_a_changed_approval_registers_again_and_applies_it(self):
        cu.apply('ask')
        self.assertNotIn('mcp__cua-driver', json.dumps(self.claude()))
        self.assertFalse(self.claude_settings.exists())
        result = cu.apply('auto')
        self.assertEqual((result['approval'], result['apply']['outcome']), ('auto', 'applied'))
        self.assertEqual(self.allowed(), ['mcp__cua-driver'])
        self.assertEqual(self.codex_config()['mcp_servers']['cua-driver']['default_tools_approval_mode'], 'approve')
        cu.apply('ask')
        self.assertEqual(self.allowed(), [])
        self.assertNotIn('default_tools_approval_mode', self.codex_config()['mcp_servers']['cua-driver'])

    def test_a_failed_doctor_fails_the_run_but_keeps_the_registration_outcome(self):
        self.doctor_code = 1
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'doctor-failed'))
        self.assertEqual(result['apply']['outcome'], 'applied')
        self.assertEqual(result['approvalOutcome'], 'applied')
        self.assertEqual(result['agents'], ['claude-code', 'codex'])

    def test_the_doctor_needs_the_display_the_x_connection_and_the_accessibility_bus(self):
        for label in ('display server', 'X11 connection', 'AT-SPI'):
            self.doctor = {'ok': True, 'probes': [dict(p, status='warn') if p['label'] == label else p
                                                  for p in DOCTOR_OK['probes']]}
            self.assertEqual(cu.apply('ask', force=True)['reason'], 'doctor-failed', label)
        self.doctor = {'ok': False, 'probes': DOCTOR_OK['probes']}
        self.assertEqual(cu.apply('ask', force=True)['reason'], 'doctor-failed')
        self.doctor = DOCTOR_OK
        self.assertEqual(cu.apply('ask', force=True)['state'], 'ready')

    def test_the_doctor_runs_with_the_session_environment(self):
        cu.apply('ask')
        env = next(env for argv, _, env in self.commands if argv[1:2] == ['doctor'])
        self.assertEqual(env['DISPLAY'], ':1')
        self.assertEqual(env['DBUS_SESSION_BUS_ADDRESS'], SESSION['DBUS_SESSION_BUS_ADDRESS'])
        self.assertEqual(env['CUA_DRIVER_RS_TELEMETRY_ENABLED'], '0')

    def test_a_ready_receipt_does_not_hide_a_failed_new_boot(self):
        cu.apply('ask')
        self.session = 'failed'
        result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['reason']), ('failed', 'session-unavailable'))
        self.assertEqual(len(self.desktop_starts()), cu.SESSION_REPAIR_ATTEMPTS)
        self.assertEqual(result['apply']['outcome'], 'applied')
        self.assertEqual(cu.status()['state'], 'failed')

    def test_a_session_that_failed_during_boot_is_started_again_before_the_doctor(self):
        self.session = 'failed'
        self.start_effect = True
        result = cu.apply('ask', boot=True)
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(len(self.desktop_starts()), 1)

    def test_a_session_still_starting_is_waited_for_without_starting_it_again(self):
        self.session = ['starting', 'starting', 'running']
        self.assertEqual(cu.apply('ask', boot=True)['state'], 'ready')
        self.assertEqual(self.desktop_starts(), [])

    def test_repair_waits_with_backoff_and_gives_up_after_three_starts(self):
        self.session = 'failed'
        cu.apply('ask', boot=True)
        self.assertEqual(self.sleeps, [2, 4, 8])

    def test_a_stopped_session_is_not_restarted_when_the_desktop_is_manual(self):
        (self.root / 'desktop-config.json').write_text(json.dumps({'autoStart': False}))
        self.session = 'stopped'
        with mock.patch.object(cu, 'SESSION_WAIT_BOOT', 3):
            result = cu.apply('ask', boot=True)
        self.assertEqual(result['reason'], 'session-unavailable')
        self.assertEqual(self.desktop_starts(), [])

    def test_a_later_apply_never_restarts_a_desktop_the_user_stopped(self):
        self.session = 'stopped'
        with mock.patch.object(cu, 'SESSION_WAIT', 3):
            result = cu.apply('ask')
        self.assertEqual(result['reason'], 'session-unavailable')
        self.assertEqual(self.desktop_starts(), [])
        # The agents were registered with what is known; the desktop failing does not hide it.
        self.assertEqual(result['apply']['outcome'], 'applied')

    def test_a_desktop_that_never_starts_fails_with_session_unavailable(self):
        self.session = False
        with mock.patch.object(cu, 'SESSION_WAIT', 3):
            self.assertEqual(cu.apply('ask')['reason'], 'session-unavailable')

    def test_a_concurrent_run_waits_then_finds_it_done(self):
        cu.apply('ask')
        self.commands.clear()
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(len(self.doctors()), 0)

    def test_the_lock_is_not_acquired_twice(self):
        with open(self.state / 'lock', 'a') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with mock.patch.object(cu, 'LOCK_WAIT', 0), self.assertRaises(cu.Failure) as caught:
                cu.apply('ask')
        self.assertEqual(caught.exception.reason, 'busy')


class Registration(Guest):
    def test_null_containers_count_as_absent(self):
        self.claude_state.parent.mkdir(parents=True)
        self.claude_state.write_text(json.dumps({'mcpServers': None, 'theme': 'dark'}))
        self.claude_settings.parent.mkdir(parents=True)
        self.claude_settings.write_text(json.dumps({'permissions': None}))
        result = cu.apply('auto')
        self.assertEqual((result['state'], result['agents']), ('ready', ['claude-code', 'codex']))
        self.assertIn('cua-driver', self.claude()['mcpServers'])
        self.assertEqual(self.allowed(), ['mcp__cua-driver'])
        self.claude_settings.write_text(json.dumps({'permissions': {'allow': None}}))
        cu.apply('auto', force=True)
        self.assertEqual(self.allowed(), ['mcp__cua-driver'])

    def test_one_agents_unusual_configuration_fails_only_that_agent(self):
        with mock.patch.dict(cu.EDITORS, {'claude-code': mock.Mock(side_effect=TypeError('odd'))}):
            result = cu.apply('ask')
        self.assertIn('codex', result['agents'])
        self.assertNotIn('claude-code', result['agents'])
        self.assertIn('cua-driver', self.codex_config()['mcp_servers'])

    def test_a_registration_removed_since_the_last_apply_is_made_again(self):
        cu.apply('auto')
        state = self.claude()
        del state['mcpServers']['cua-driver']
        self.claude_state.write_text(json.dumps(state))
        self.assertEqual(cu.apply('auto')['apply']['outcome'], 'applied')
        self.assertIn('cua-driver', self.claude()['mcpServers'])
        # The approval rule and the Codex section are checked too.
        self.claude_settings.write_text(json.dumps({}))
        cu.apply('auto')
        self.assertEqual(self.allowed(), ['mcp__cua-driver'])
        self.codex.write_text('model = "x"\n')
        cu.apply('auto')
        self.assertEqual(self.codex_config()['mcp_servers']['cua-driver']['default_tools_approval_mode'], 'approve')
        # Nothing changed: the fast path runs no readiness check.
        self.commands.clear()
        cu.apply('auto')
        self.assertFalse([argv for argv, *_ in self.commands if argv[1:2] == ['doctor']])

    def test_lcu_skills_data_and_pi_package_are_removed_and_nothing_else(self):
        home = self.root / 'home'
        for directory in ('.claude/skills/lcu', '.codex/skills/lcu-desktop', '.agents/skills/lcu',
                          '.pi/agent/skills/lcu', '.claude/skills/mine', '.local/share/lcu/state'):
            (home / directory).mkdir(parents=True)
        (home / '.claude/skills/lcu/SKILL.md').write_text('x')
        pi = home / '.pi/agent/settings.json'
        pi.write_text(json.dumps({'packages': ['/opt/lcu/current/pi', {'source': '/home/silo/.local/share/lcu/pi'},
                                               'npm:keep-me'], 'theme': 'dark'}))
        cu.apply('ask')
        for gone in ('.claude/skills/lcu', '.codex/skills/lcu-desktop', '.agents/skills/lcu', '.pi/agent/skills/lcu',
                     '.local/share/lcu'):
            self.assertFalse((home / gone).exists(), gone)
        self.assertTrue((home / '.claude/skills/mine').is_dir())
        self.assertEqual(json.loads(pi.read_text()), {'packages': ['npm:keep-me'], 'theme': 'dark'})

    def test_pi_settings_are_never_created(self):
        cu.apply('ask')
        self.assertFalse((self.root / 'home/.pi').exists())

    def test_agents_installed_by_version_managers_are_found(self):
        node_bin = self.root / 'home/.nvm/versions/node/v22.1.0/bin'
        node_bin.mkdir(parents=True)
        (node_bin / 'claude').write_text('#!/bin/sh\n')
        (node_bin / 'claude').chmod(0o755)
        self.assertIsNotNone(REAL_BINARY_INSTALLED('claude'))
        self.assertIsNone(REAL_BINARY_INSTALLED('codex'))

    def setUp(self):
        super().setUp()
        self.install_image_driver()

    def test_claude_code_gets_a_plain_stdio_server_through_the_launcher(self):
        cu.apply('ask')
        self.assertEqual(self.claude()['mcpServers']['cua-driver'], {
            'type': 'stdio', 'command': str(cu.LAUNCHER), 'args': [],
            'env': {'CUA_DRIVER_RS_TELEMETRY_ENABLED': '0', 'CUA_DRIVER_RS_UPDATE_CHECK': 'false'}})

    def repaired(self):
        """Runs the root repair with every owner reported as root; returns the chown calls."""
        owner = mock.Mock(pw_uid=1000, pw_gid=1000)
        real_stat, real_fstat = os.stat, os.fstat
        class Root:
            def __init__(self, info):
                self.st_uid, self.st_mode = 0, info.st_mode
        calls = []
        def stat_as_root(path, *args, **kwargs):
            return Root(real_stat(path, *args, **kwargs))
        with mock.patch.object(cu.os, 'geteuid', return_value=0), \
                mock.patch.object(cu.pwd, 'getpwnam', return_value=owner), \
                mock.patch.object(cu, 'HOME', str(self.claude_state.parent)), \
                mock.patch.object(cu.os, 'stat', stat_as_root), \
                mock.patch.object(cu.os, 'fstat', lambda fd: Root(real_fstat(fd))), \
                mock.patch.object(cu.os, 'fchown', lambda fd, uid, gid: calls.append(('dir', uid, gid))), \
                mock.patch.object(cu.os, 'chown', lambda name, uid, gid, **kw: calls.append((name, uid, gid, kw))):
            cu.repair_agent_files()
        return calls

    def test_root_owned_agent_files_and_directories_are_given_back_to_the_account(self):
        self.claude_settings.parent.mkdir(parents=True)
        self.claude_settings.write_text('{}')
        self.codex.parent.mkdir(parents=True)
        self.codex.write_text('')
        calls = self.repaired()
        self.assertEqual([call for call in calls if call[0] == 'dir'], [('dir', 1000, 1000)] * 2)
        files = [call for call in calls if call[0] != 'dir']
        self.assertEqual({call[0] for call in files}, {'settings.json', 'config.toml'})
        for _, uid, gid, kwargs in files:
            self.assertEqual((uid, gid), (1000, 1000))
            self.assertIs(kwargs['follow_symlinks'], False)
            self.assertIsInstance(kwargs['dir_fd'], int)

    def test_a_symlinked_parent_is_not_followed_by_the_root_repair(self):
        outside = Path(tempfile.mkdtemp())
        (outside / 'config.toml').write_text('')
        self.claude_settings.parent.mkdir(parents=True)
        self.claude_settings.write_text('{}')
        self.codex.parent.symlink_to(outside, target_is_directory=True)
        calls = self.repaired()
        names = {call[0] for call in calls}
        self.assertNotIn('config.toml', names)
        self.assertEqual([call for call in calls if call[0] == 'dir'], [('dir', 1000, 1000)])
        self.assertIn('settings.json', names)
        self.assertEqual((outside / 'config.toml').stat().st_uid, os.getuid())

    def test_silos_own_server_is_registered_beside_the_driver_and_removed_alone(self):
        cu.apply('auto')
        self.claude_settings.write_text(json.dumps({'permissions': {'allow': ['mcp__cua-driver', 'Bash(ls)']}}))
        registered, pending, failed, conflicts = cu.configure_server('silo', '/usr/local/bin/silo', ['mcp'])
        self.assertEqual((registered, pending, failed, conflicts), (['claude-code', 'codex'], [], [], []))
        servers = self.claude()['mcpServers']
        self.assertEqual(servers['silo'], {'type': 'stdio', 'command': '/usr/local/bin/silo', 'args': ['mcp'],
                                           'env': {'SILO_MANAGED': '1'}})
        self.assertIn('cua-driver', servers)
        section = self.codex_config()['mcp_servers']
        self.assertEqual((section['silo']['command'], section['silo']['args']), ('/usr/local/bin/silo', ['mcp']))
        self.assertEqual(section['silo']['env'], {'SILO_MANAGED': '1'})
        self.assertEqual(section['cua-driver']['default_tools_approval_mode'], 'approve')
        # Registering again changes nothing, and the driver's approval rule is untouched.
        before = (self.claude_state.read_text(), self.codex.read_text())
        cu.configure_server('silo', '/usr/local/bin/silo', ['mcp'])
        self.assertEqual(before, (self.claude_state.read_text(), self.codex.read_text()))
        self.assertEqual(json.loads(self.claude_settings.read_text())['permissions']['allow'], ['mcp__cua-driver', 'Bash(ls)'])
        removed, failed, conflicts = cu.remove_server('silo')
        self.assertEqual((removed, failed, conflicts), (['claude-code', 'codex'], [], []))
        self.assertNotIn('silo', self.claude()['mcpServers'])
        self.assertIn('cua-driver', self.claude()['mcpServers'])
        self.assertNotIn('silo', self.codex_config()['mcp_servers'])
        self.assertIn('cua-driver', self.codex_config()['mcp_servers'])

    def test_a_server_of_another_name_is_kept_when_silos_is_removed_and_a_missing_file_is_not_created(self):
        self.claude_state.parent.mkdir(parents=True, exist_ok=True)
        self.claude_state.write_text(json.dumps({'mcpServers': {
            'mine': {'command': 'x'}, 'silo': {'command': 'old', 'env': {'SILO_MANAGED': '1'}}}}))
        cu.remove_server('silo')
        self.assertEqual(self.claude()['mcpServers'], {'mine': {'command': 'x'}})
        self.assertFalse(self.codex.exists())

    def test_a_server_the_user_added_under_silos_name_is_left_alone_and_reported(self):
        self.claude_state.parent.mkdir(parents=True, exist_ok=True)
        self.claude_state.write_text(json.dumps({'mcpServers': {'silo': {'command': 'mine'}}}))
        self.codex.parent.mkdir(parents=True, exist_ok=True)
        self.codex.write_text('[mcp_servers.silo]\ncommand = "mine"\nargs = []\n')
        before = (self.claude_state.read_text(), self.codex.read_text())
        self.assertEqual(cu.configure_server('silo', '/usr/local/bin/silo', ['mcp']),
                         ([], [], [], ['claude-code', 'codex']))
        self.assertEqual(cu.remove_server('silo', '/usr/local/bin/silo', ['mcp']),
                         ([], [], ['claude-code', 'codex']))
        self.assertEqual(cu.remove_server('silo'), ([], [], ['claude-code', 'codex']))
        self.assertEqual(before, (self.claude_state.read_text(), self.codex.read_text()))

    def test_the_unmarked_entry_an_earlier_silo_wrote_is_marked_and_removable(self):
        old = {'type': 'stdio', 'command': '/usr/local/bin/silo', 'args': ['mcp'], 'env': {}}
        self.claude_state.parent.mkdir(parents=True, exist_ok=True)
        self.claude_state.write_text(json.dumps({'mcpServers': {'silo': old, 'mine': {'command': 'x'}}}))
        self.codex.parent.mkdir(parents=True, exist_ok=True)
        old_toml = '[mcp_servers.silo]\ncommand = "/usr/local/bin/silo"\nargs = ["mcp"]\n'
        self.codex.write_text('model = "x"\n\n' + old_toml)
        self.assertEqual(cu.configure_server('silo', '/usr/local/bin/silo', ['mcp']),
                         (['claude-code', 'codex'], [], [], []))
        self.assertEqual(self.claude()['mcpServers']['silo']['env'], {'SILO_MANAGED': '1'})
        self.assertEqual(self.claude()['mcpServers']['mine'], {'command': 'x'})
        self.assertEqual(self.codex_config()['mcp_servers']['silo']['env'], {'SILO_MANAGED': '1'})
        self.assertEqual(self.codex_config()['model'], 'x')
        # An earlier entry is also taken out when the command is named.
        self.claude_state.write_text(json.dumps({'mcpServers': {'silo': old}}))
        self.codex.write_text(old_toml)
        self.assertEqual(cu.remove_server('silo', '/usr/local/bin/silo', ['mcp']),
                         (['claude-code', 'codex'], [], []))
        self.assertEqual(self.claude()['mcpServers'], {})
        self.assertNotIn('silo', self.codex_config().get('mcp_servers', {}))

    def test_a_change_claude_code_made_while_silo_edited_is_kept(self):
        self.claude_state.parent.mkdir(parents=True, exist_ok=True)
        self.claude_state.write_text(json.dumps({'userID': '1'}))
        read = cu.read_object
        reads = []

        def read_then_change(path):
            found = read(path)
            reads.append(path)
            if path == cu.CLAUDE_STATE and len(reads) == 1:
                path.write_text(json.dumps({'userID': '1', 'projects': {'a': 1}}))
            return found

        with mock.patch.object(cu, 'read_object', side_effect=read_then_change):
            cu.configure_server('silo', '/usr/local/bin/silo', ['mcp'])
        saved = self.claude()
        self.assertEqual(saved['projects'], {'a': 1})
        self.assertIn('silo', saved['mcpServers'])

    def test_an_agent_that_is_not_installed_is_pending_for_silos_server(self):
        self.installed_agents.discard('codex')
        registered, pending, failed, conflicts = cu.configure_server('silo', '/usr/local/bin/silo', ['mcp'])
        self.assertEqual((registered, pending, failed, conflicts), (['claude-code'], ['codex'], [], []))
        self.assertFalse(self.codex.exists())

    def test_only_a_plain_name_and_an_absolute_command_are_registered(self):
        for name, command in (('cua-driver', '/x'), ('Silo', '/x'), ('a b', '/x'), ('', '/x'), ('silo', 'silo'), ('silo', '')):
            with self.assertRaises(cu.Failure, msg=(name, command)):
                cu.configure_server(name, command, ['mcp'])
        with self.assertRaises(cu.Failure):
            cu.remove_server('cua-driver')
        self.assertFalse(self.claude_state.exists())

    def test_the_command_line_registers_and_removes_and_reports_a_file_it_cannot_edit(self):
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('sys.stdout', new=io.StringIO()) as out:
            code = cu.main(['server-add', '--name', 'silo', '--command', '/usr/local/bin/silo', '--arg', 'mcp'])
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out.getvalue())['registered'], ['claude-code', 'codex'])
        self.claude_state.write_text('[]')
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('sys.stdout', new=io.StringIO()) as out:
            code = cu.main(['server-add', '--name', 'silo', '--command', '/usr/local/bin/silo', '--arg', 'mcp'])
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(out.getvalue())['failed'], ['claude-code'])
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('sys.stdout', new=io.StringIO()) as out:
            code = cu.main(['server-remove', '--name', 'cua-driver'])
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(out.getvalue())['error'], 'invalid-server')

    def test_codex_gets_the_same_server_in_its_own_section(self):
        cu.apply('auto')
        server = self.codex_config()['mcp_servers']['cua-driver']
        self.assertEqual((server['command'], server['args']), (str(cu.LAUNCHER), []))
        self.assertEqual(server['default_tools_approval_mode'], 'approve')
        self.assertEqual(server['env'], {'CUA_DRIVER_RS_TELEMETRY_ENABLED': '0', 'CUA_DRIVER_RS_UPDATE_CHECK': 'false'})

    def test_the_launcher_is_written_on_every_apply_and_matches_the_shipped_file(self):
        shipped = (SCRIPT.parent / 'silo-cua-driver-mcp').read_text()
        self.assertEqual(cu.LAUNCHER_TEXT, shipped)
        cu.apply('ask')
        self.assertEqual(cu.LAUNCHER.read_text(), shipped)
        self.assertTrue(os.access(cu.LAUNCHER, os.X_OK))
        cu.LAUNCHER.write_text('old')
        cu.apply('ask')
        self.assertEqual(cu.LAUNCHER.read_text(), shipped)
        cu.LAUNCHER.write_text('old')
        cu.reconcile()
        self.assertEqual(cu.LAUNCHER.read_text(), shipped)

    def test_ask_leaves_no_empty_permissions_behind(self):
        cu.apply('auto')
        self.assertEqual(self.allowed(), ['mcp__cua-driver'])
        cu.apply('ask')
        self.assertNotIn('permissions', json.loads(self.claude_settings.read_text()))

    def test_ask_clears_auto_approval_even_when_the_agent_is_not_installed(self):
        self.installed_agents = set()
        self.claude_settings.parent.mkdir(parents=True)
        self.claude_settings.write_text(json.dumps({'permissions': {'allow': ['Bash(ls)', 'mcp__cua-driver']}}))
        self.codex.parent.mkdir(parents=True)
        self.codex.write_text('[mcp_servers.cua-driver]\ncommand = "x"\ndefault_tools_approval_mode = "approve"\n')
        cu.apply('ask')
        self.assertEqual(json.loads(self.claude_settings.read_text())['permissions'], {'allow': ['Bash(ls)']})
        self.assertNotIn('approve', self.codex.read_text())
        self.assertIn('command = "x"', self.codex.read_text())

    def test_ask_keeps_other_rules(self):
        self.claude_settings.parent.mkdir(parents=True)
        self.claude_settings.write_text(json.dumps({'permissions': {'allow': ['Bash(ls)', 'mcp__cua-driver']}}))
        cu.apply('ask')
        self.assertEqual(json.loads(self.claude_settings.read_text())['permissions'], {'allow': ['Bash(ls)']})

    def test_telemetry_is_switched_off_for_every_shell(self):
        environment = cu.SYSTEM_ENVIRONMENT
        environment.write_text('PATH="/usr/bin"\nCUA_DRIVER_RS_UPDATE_CHECK=false')
        cu.apply('ask')
        self.assertEqual(cu.PRIVACY_PROFILE.read_text(), cu.PRIVACY_PROFILE_TEXT)
        self.assertEqual(environment.read_text(),
                         'PATH="/usr/bin"\nCUA_DRIVER_RS_UPDATE_CHECK=false\nCUA_DRIVER_RS_TELEMETRY_ENABLED=0\n')
        before = environment.stat().st_mtime_ns
        cu.apply('ask', force=True)
        self.assertEqual(environment.stat().st_mtime_ns, before)

    def test_image_privacy_profile_matches_the_helper(self):
        dockerfile = (Path(__file__).parents[1] / 'guest-image/Dockerfile').read_text()
        for line in cu.PRIVACY_PROFILE_TEXT.splitlines():
            self.assertIn(f"'{line}'", dockerfile)

    def test_registration_is_idempotent_and_rewrites_nothing(self):
        cu.apply('auto')
        before = [(p.read_text(), p.stat().st_mtime_ns) for p in (self.claude_state, self.claude_settings, self.codex)]
        cu.apply('auto', force=True)
        cu.apply('auto', boot=True)
        after = [(p.read_text(), p.stat().st_mtime_ns) for p in (self.claude_state, self.claude_settings, self.codex)]
        self.assertEqual(before, after)

    def test_other_configuration_is_preserved(self):
        self.claude_state.parent.mkdir(parents=True)
        self.claude_state.write_text(json.dumps({'theme': 'dark', 'mcpServers': {'other': {'command': 'x'}}}))
        self.claude_settings.parent.mkdir(parents=True)
        self.claude_settings.write_text(json.dumps({'permissions': {'allow': ['Bash(ls)'], 'deny': ['Read(.env)']}}))
        self.codex.parent.mkdir(parents=True)
        self.codex.write_text('model = "x"\n\n[mcp_servers.other]\ncommand = "y"\n\n[profiles.p]\na = 1\n')
        cu.apply('auto')
        self.assertEqual(self.claude()['theme'], 'dark')
        self.assertIn('other', self.claude()['mcpServers'])
        self.assertEqual(self.allowed(), ['Bash(ls)', 'mcp__cua-driver'])
        config = self.codex_config()
        self.assertEqual((config['model'], config['mcp_servers']['other'], config['profiles']['p']),
                         ('x', {'command': 'y'}, {'a': 1}))
        cu.apply('ask')
        self.assertEqual(self.allowed(), ['Bash(ls)'])

    def test_leftover_lcu_registrations_approvals_hooks_and_files_are_removed(self):
        self.claude_state.parent.mkdir(parents=True)
        self.claude_state.write_text(json.dumps({'mcpServers': {'lcu': {'command': 'node'}, 'other': {}}}))
        self.claude_settings.parent.mkdir(parents=True)
        self.claude_settings.write_text(json.dumps({
            'permissions': {'allow': ['mcp__lcu', 'Bash(ls)', 'Read(~/lcu-notes/**)', 'Bash(lcu status)'],
                            'deny': ['mcp__lcu__host_only', 'mcp__lcutils']},
            'hooks': {'Stop': [{'hooks': [{'type': 'command', 'command': '/opt/lcu/current/bin/lcu hook'}]},
                               {'hooks': [{'type': 'command', 'command': 'echo mine'}]},
                               {'hooks': [{'type': 'command', 'command': 'cp log /opt/lcu-archive/'}]}],
                      'PreToolUse': [{'hooks': [{'type': 'command', 'command': '/opt/lcu/x'}]}]}}))
        self.codex.parent.mkdir(parents=True)
        self.codex.write_text('[mcp_servers.lcu]\ncommand = "lcu"\ndefault_tools_approval_mode = "approve"\n\n'
                              '[mcp_servers.lcu.env]\nA = "b"\n\n[mcp_servers.keep]\ncommand = "k"\n\n'
                              '[hooks.state."/opt/lcu/hooks.json:stop"]\ntrusted_hash = "x"\n')
        (self.legacy_prefix / 'current').mkdir(parents=True)
        self.legacy_receipt.write_text('{}')
        cu.apply('ask')
        self.assertEqual(list(self.claude()['mcpServers']), ['other', 'cua-driver'])
        settings = json.loads(self.claude_settings.read_text())
        # Only what LCU wrote goes; the user's own rules and hooks that mention "lcu" stay.
        self.assertEqual(settings['permissions'], {'allow': ['Bash(ls)', 'Read(~/lcu-notes/**)', 'Bash(lcu status)'],
                                                   'deny': ['mcp__lcutils']})
        self.assertEqual(settings['hooks'], {'Stop': [{'hooks': [{'type': 'command', 'command': 'echo mine'}]},
                                                      {'hooks': [{'type': 'command', 'command': 'cp log /opt/lcu-archive/'}]}]})
        config = self.codex_config()
        self.assertEqual(sorted(config['mcp_servers']), ['cua-driver', 'keep'])
        self.assertNotIn('hooks', config)
        self.assertNotIn('lcu', self.codex.read_text())
        self.assertFalse(self.legacy_prefix.exists() or self.legacy_receipt.exists())

    def test_cleanup_creates_no_files_for_agents_that_have_none(self):
        self.installed_agents = set()
        cu.apply('ask')
        self.assertFalse(self.claude_state.exists() or self.claude_settings.exists() or self.codex.exists())

    def test_an_agent_that_is_not_installed_yet_is_pending_not_a_failure(self):
        self.installed_agents = {'codex'}
        result = cu.apply('ask')
        self.assertEqual(result['apply'], {'approval': 'ask', 'outcome': 'applied', 'reason': None})
        self.assertEqual((result['agents'], result['pending']), (['codex'], ['claude-code']))
        self.assertFalse(self.claude_state.exists())
        self.assertEqual(self.spawned, [True], 'a watcher starts while agents are pending')
        self.assertIn('silo-computer-use reconcile', self.hook.read_text())

    def test_nothing_pending_starts_no_watcher_but_leaves_the_login_hook(self):
        cu.apply('ask')
        self.assertEqual((self.receipt()['pending'], self.spawned), ([], []))
        self.assertTrue(self.hook.is_file())

    def test_a_boot_registers_an_agent_installed_while_the_computer_was_off(self):
        self.installed_agents = {'codex'}
        cu.apply('auto')
        self.installed_agents = {'codex', 'claude'}
        result = cu.apply('auto', boot=True)
        self.assertEqual((result['agents'], result['pending']), (['claude-code', 'codex'], []))
        self.assertEqual(self.allowed(), ['mcp__cua-driver'])

    def test_the_no_op_path_still_registers_a_newly_installed_agent(self):
        self.installed_agents = {'claude'}
        cu.apply('ask')
        self.installed_agents = {'claude', 'codex'}
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'applied')
        self.assertEqual((self.receipt()['pending'], self.receipt()['agents']), ([], ['claude-code', 'codex']))
        self.assertTrue(self.codex.exists())

    def test_the_reconcile_command_registers_with_the_receipts_approval_and_skips_when_busy_or_not_ready(self):
        self.assertEqual(cu.reconcile(), {'reconcile': 'skipped'})
        self.installed_agents = {'claude'}
        cu.apply('auto')
        self.installed_agents = {'claude', 'codex'}
        self.assertEqual(cu.reconcile(), {'reconcile': 'done', 'agents': ['claude-code', 'codex'], 'pending': []})
        self.assertEqual(self.codex_config()['mcp_servers']['cua-driver']['default_tools_approval_mode'], 'approve')
        with open(self.state / 'lock', 'a') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(cu.reconcile(), {'reconcile': 'busy'})

    def test_no_session_address_is_stored_in_a_registration(self):
        cu.apply('ask')
        self.session_environ['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=/tmp/dbus-BBBB'
        cu.apply('ask')
        for text in (self.claude_state.read_text(), self.codex.read_text(), json.dumps(self.receipt())):
            self.assertNotIn('dbus', text.lower())
            self.assertNotIn('DISPLAY', text)

    def test_an_unreadable_agent_file_is_partial_when_others_are_configured(self):
        self.claude_state.parent.mkdir(parents=True)
        self.claude_state.write_text('{not json')
        result = cu.apply('ask')
        self.assertEqual(result['apply'], {'approval': 'ask', 'outcome': 'partial', 'reason': 'setup-partial'})
        self.assertEqual(self.claude_state.read_text(), '{not json', 'a file we cannot parse is left alone')
        self.assertEqual((result['agents'], result['pending']), (['codex'], ['claude-code']))
        self.assertEqual(result['state'], 'ready')

    def test_nothing_configured_because_of_failures_is_failed_and_retried(self):
        self.installed_agents = {'claude'}
        self.claude_state.parent.mkdir(parents=True)
        self.claude_state.write_text('[]')
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'setup-failed'))
        self.assertEqual(result['apply']['outcome'], 'failed')
        self.installed_agents = set()
        self.assertEqual(cu.apply('ask')['state'], 'ready')

    def test_an_unparsable_codex_result_is_never_written(self):
        self.codex.parent.mkdir(parents=True)
        self.codex.write_text('a = [\n')
        original = self.codex.read_text()
        with self.assertRaises(cu.Failure):
            cu.edit_codex(cu.server_entry(), 'ask')
        self.assertEqual(self.codex.read_text(), original)

    def test_agents_are_reported_in_the_order_of_their_ids(self):
        self.assertEqual(sorted(cu.AGENTS), ['claude-code', 'codex'])


class Watcher(Guest):
    def test_the_watcher_ends_when_the_receipt_is_not_ready_or_after_its_maximum(self):
        self.install_image_driver()
        bin_dir = self.root / 'agentbin'
        bin_dir.mkdir()
        self.installed_agents = set()
        cu.apply('ask')
        receipt = self.receipt()
        binary = bin_dir / 'codex'
        binary.write_text('#!/bin/sh\n')
        binary.chmod(0o755)
        runs = []
        with mock.patch.object(cu.subprocess, 'run', lambda argv, **k: runs.append(argv)), \
                mock.patch.object(cu, 'binary_installed', REAL_BINARY_INSTALLED):
            (self.state / 'receipt.json').write_text(json.dumps(dict(receipt, state='failed')))
            cu.watch(interval=0, directories=[bin_dir])
            self.assertEqual(runs, [], 'a receipt that is not ready ends the watcher at once')
            (self.state / 'receipt.json').write_text(json.dumps(receipt))
            with mock.patch.object(cu, 'WATCH_MAX', 0):
                cu.watch(interval=0, directories=[bin_dir])
            self.assertEqual(runs, [], 'the watcher ends after WATCH_MAX')

    def test_the_watcher_reconciles_once_when_a_pending_binary_appears_and_ends_when_none_is_pending(self):
        self.install_image_driver()
        bin_dir = self.root / 'agentbin'
        bin_dir.mkdir()
        self.installed_agents = set()
        cu.apply('ask')
        self.assertEqual(self.receipt()['pending'], ['claude-code', 'codex'])
        runs = []
        with mock.patch.object(cu.subprocess, 'run', lambda argv, **k: runs.append(argv)), \
                mock.patch.object(cu, 'binary_installed', REAL_BINARY_INSTALLED):
            cu.watch(interval=0, directories=[bin_dir], rounds=2)
            self.assertEqual(runs, [], 'nothing installed yet')
            binary = bin_dir / 'codex'
            binary.write_text('#!/bin/sh\n')
            binary.chmod(0o755)
            cu.watch(interval=0, directories=[bin_dir], rounds=3)
            self.assertEqual(runs, [[cu.HELPER, 'reconcile']])
            receipt = self.receipt()
            receipt['pending'] = []
            (self.state / 'receipt.json').write_text(json.dumps(receipt))
            cu.watch(interval=0, directories=[bin_dir])
        self.assertEqual(runs, [[cu.HELPER, 'reconcile']])

    def test_the_agents_are_looked_up_by_their_executable_names(self):
        self.assertEqual(cu.EXECUTABLES, {'claude-code': 'claude', 'codex': 'codex'})


class SessionEnvironment(unittest.TestCase):
    SESSION = ('DISPLAY=:1\nDBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/dbus-Z\n'
               'XDG_RUNTIME_DIR=/run/silo-desktop/user\nXAUTHORITY=/run/x\nPATH=/ignored\n')

    def read(self, directory, proc=None, **options):
        account = mock.Mock(pw_uid=os.getuid())
        with mock.patch.object(cu.pwd, 'getpwnam', return_value=account):
            return cu.session_env(directory=directory, proc=Path(proc or directory), **options)

    def test_the_environment_is_read_from_the_file_the_session_wrote(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / 'session.env').write_text(self.SESSION)
            found = self.read(directory)
        self.assertEqual(found, {'DISPLAY': ':1', 'XDG_RUNTIME_DIR': '/run/silo-desktop/user',
                                 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/tmp/dbus-Z', 'XAUTHORITY': '/run/x'})

    def test_without_the_file_defaults_apply_and_strict_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(self.read(directory),
                             {'DISPLAY': ':1', 'XDG_RUNTIME_DIR': '/run/silo-desktop/user'})
            with self.assertRaises(cu.Failure) as caught:
                self.read(directory, strict=True)
        self.assertEqual(caught.exception.reason, 'session-unavailable')

    def test_a_session_an_earlier_desktop_service_started_is_read_from_its_process(self):
        # A computer made by an earlier Silo keeps its desktop service, which writes no file.
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as proc:
            fake_session_processes(Path(proc))
            found = self.read(directory, proc=proc, strict=True)
        self.assertEqual(found, {'DISPLAY': ':1', 'XDG_RUNTIME_DIR': '/run/silo-desktop/user',
                                 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/tmp/dbus-OLD', 'XAUTHORITY': '/run/x'})

    def test_the_file_wins_over_a_session_process(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as proc:
            fake_session_processes(Path(proc))
            (Path(directory) / 'session.env').write_text(self.SESSION)
            found = self.read(directory, proc=proc)
        self.assertEqual(found['DBUS_SESSION_BUS_ADDRESS'], 'unix:path=/tmp/dbus-Z')


def fake_session_processes(proc):
    """A `/proc` with a shell and the `xfce4-session` of a desktop session."""
    for pid, comm, environ in ((10, 'bash', 'DISPLAY=:9\0DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/other'),
                               (11, 'xfce4-session',
                                'DISPLAY=:1\0DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/dbus-OLD\0'
                                'XDG_RUNTIME_DIR=/run/silo-desktop/user\0XAUTHORITY=/run/x\0PATH=/bin')):
        (proc / str(pid)).mkdir()
        (proc / str(pid) / 'comm').write_text(comm + '\n')
        (proc / str(pid) / 'environ').write_bytes(environ.encode())


class Launcher(unittest.TestCase):
    def load(self):
        spec = importlib.util.spec_from_file_location('launcher', SCRIPT.parent / 'silo-cua-driver-mcp',
                                                      loader=importlib.machinery.SourceFileLoader(
                                                          'launcher', str(SCRIPT.parent / 'silo-cua-driver-mcp')))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    SESSION = ('DISPLAY=:1\nDBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/dbus-NEW\n'
               'XDG_RUNTIME_DIR=/run/silo-desktop/user\nXAUTHORITY=/run/xauth\nPATH=/ignored\n')

    def test_it_reads_the_file_the_session_wrote(self):
        launcher = self.load()
        with tempfile.TemporaryDirectory() as directory:
            session = Path(directory) / 'session.env'
            session.write_text(self.SESSION)
            found = launcher.session_env(str(session), proc=directory)
            self.assertEqual(found, {'DISPLAY': ':1', 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/tmp/dbus-NEW',
                                     'XDG_RUNTIME_DIR': '/run/silo-desktop/user', 'XAUTHORITY': '/run/xauth'})
            # A file of another account is not used.
            other = launcher.session_env(str(session), uid=os.getuid() + 1, proc=directory)
            self.assertNotIn('XAUTHORITY', other)
            self.assertNotEqual(other['DBUS_SESSION_BUS_ADDRESS'], 'unix:path=/tmp/dbus-NEW')

    def test_it_does_not_follow_a_link_or_read_a_pipe(self):
        launcher = self.load()
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'target'
            target.write_text(self.SESSION)
            link = Path(directory) / 'link'
            link.symlink_to(target)
            pipe = Path(directory) / 'pipe'
            os.mkfifo(pipe)
            for path in (link, pipe):
                self.assertNotEqual(launcher.session_env(str(path), proc=directory)['DBUS_SESSION_BUS_ADDRESS'],
                                    'unix:path=/tmp/dbus-NEW')

    def test_without_the_file_it_falls_back_to_the_standard_addresses(self):
        launcher = self.load()
        with tempfile.TemporaryDirectory() as directory:
            found = launcher.session_env(str(Path(directory) / 'none'), uid=1234, proc=directory)
        self.assertEqual(found['DISPLAY'], ':1')
        self.assertTrue(found['DBUS_SESSION_BUS_ADDRESS'].endswith('/bus'))

    def test_a_session_an_earlier_desktop_service_started_is_read_from_its_process(self):
        # That service writes no file; the session's own process still says where the session is.
        launcher = self.load()
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as proc:
            fake_session_processes(Path(proc))
            found = launcher.session_env(str(Path(directory) / 'none'), proc=proc)
            self.assertEqual(found, {'DISPLAY': ':1', 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/tmp/dbus-OLD',
                                     'XDG_RUNTIME_DIR': '/run/silo-desktop/user', 'XAUTHORITY': '/run/x'})
            # Another account's session is not this account's.
            other = launcher.session_env(str(Path(directory) / 'none'), uid=os.getuid() + 1, proc=proc)
            self.assertNotIn('XAUTHORITY', other)
            # The file, when the session wrote one, is what counts.
            session = Path(directory) / 'session.env'
            session.write_text(self.SESSION)
            self.assertEqual(launcher.session_env(str(session), proc=proc)['DBUS_SESSION_BUS_ADDRESS'],
                             'unix:path=/tmp/dbus-NEW')

    def test_it_execs_the_driver_with_privacy_variables_and_the_arguments(self):
        launcher = self.load()
        with mock.patch.object(launcher.os, 'execve') as execve, \
                mock.patch.object(launcher, 'session_env', return_value={'DISPLAY': ':1'}):
            launcher.main(['launcher', '--extra'])
        path, argv, environment = execve.call_args.args
        self.assertEqual((path, argv), ('/usr/local/bin/cua-driver', ['/usr/local/bin/cua-driver', 'mcp', '--extra']))
        self.assertEqual((environment['CUA_DRIVER_RS_TELEMETRY_ENABLED'], environment['CUA_DRIVER_RS_UPDATE_CHECK'],
                          environment['DISPLAY']), ('0', 'false', ':1'))


class CommandLine(Guest):
    def setUp(self):
        super().setUp()
        self.install_image_driver()

    def test_the_user_commands_run_with_the_agents_install_directories_on_the_path(self):
        with mock.patch.object(cu.subprocess, 'run') as run:
            run.return_value = subprocess.CompletedProcess([], 0, stdout='', stderr='')
            REAL_RUN(['cua-driver', 'doctor'], user=True, extra_env={'DISPLAY': ':1'})
        argv = run.call_args.args[0]
        self.assertIn(f'PATH={cu.USER_PATH}', argv)
        self.assertIn('DISPLAY=:1', argv)
        for directory in ('/home/silo/.local/bin', '/home/silo/.bun/bin', '/usr/local/bin'):
            self.assertIn(directory, cu.USER_PATH.split(':'))

    def test_main_applies_with_the_requested_mode_and_prints_the_outcome(self):
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('builtins.print') as shown:
            self.assertEqual(cu.main(['apply', '--approval', 'auto', '--boot']), 0)
        printed = json.loads(shown.call_args.args[0])
        self.assertEqual(printed['apply'], {'approval': 'auto', 'outcome': 'applied', 'reason': None})
        for bad in (['apply'], ['apply', '--approval', 'maybe'], ['apply', '--approval', 'ask', '--revision', '3']):
            with mock.patch.object(cu.os, 'geteuid', return_value=0), self.assertRaises(SystemExit):
                cu.main(bad)

    def test_a_run_that_cannot_start_is_still_a_report(self):
        with mock.patch.object(cu, 'apply', side_effect=cu.Failure('busy')), \
                mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('builtins.print') as shown:
            self.assertEqual(cu.main(['apply', '--approval', 'ask']), 0)
        printed = json.loads(shown.call_args.args[0])
        self.assertEqual(printed['apply'], {'approval': 'ask', 'outcome': 'failed', 'reason': 'busy'})

    def test_main_runs_reconcile_and_prints_it(self):
        cu.apply('ask')
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('builtins.print') as shown:
            self.assertEqual(cu.main(['reconcile']), 0)
        self.assertEqual(json.loads(shown.call_args.args[0])['reconcile'], 'done')

    def test_main_prints_status_and_requires_root(self):
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('builtins.print') as shown:
            self.assertEqual(cu.main(['status']), 0)
        self.assertEqual(json.loads(shown.call_args.args[0])['state'], 'not-set-up')
        with mock.patch.object(cu.os, 'geteuid', return_value=1000):
            self.assertEqual(cu.main(['status']), 1)


class ReceiptPersistence(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name) / 'receipt.json'
        self.path.write_text('{"state":"installing"}\n')
        receipt = mock.patch.object(cu, 'RECEIPT', self.path)
        receipt.start()
        self.addCleanup(receipt.stop)
        self.pinned = {'version': 'fixture-version'}

    def test_ready_receipt_syncs_final_permissions_and_published_directory(self):
        synced = []
        fsync = os.fsync

        def sync(fd):
            info = os.fstat(fd)
            if stat.S_ISREG(info.st_mode):
                self.assertEqual(json.loads(self.path.read_text())['state'], 'installing')
                self.assertEqual(json.loads(os.pread(fd, info.st_size, 0))['state'], 'ready')
                self.assertEqual(stat.S_IMODE(info.st_mode), 0o644)
                synced.append('file')
            else:
                self.assertTrue(stat.S_ISDIR(info.st_mode))
                self.assertEqual(info.st_ino, self.path.parent.stat().st_ino)
                self.assertEqual(json.loads(self.path.read_text())['state'], 'ready')
                synced.append('directory')
            fsync(fd)

        with mock.patch.object(cu.os, 'fsync', side_effect=sync):
            cu.write_receipt(self.pinned, 'ask', 'ready')
        self.assertEqual(synced, ['file', 'directory'])
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])

    def test_ready_receipt_does_not_acknowledge_directory_sync_failure(self):
        fsync = os.fsync

        def sync(fd):
            if stat.S_ISDIR(os.fstat(fd).st_mode):
                raise OSError('receipt directory sync failed')
            fsync(fd)

        with mock.patch.object(cu.os, 'fsync', side_effect=sync):
            with self.assertRaisesRegex(OSError, 'receipt directory sync failed'):
                cu.write_receipt(self.pinned, 'ask', 'ready')
        self.assertEqual(json.loads(self.path.read_text())['state'], 'ready')
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])


class SessionFile(unittest.TestCase):
    def test_root_reads_only_a_small_regular_file_of_the_account(self):
        uid = os.getuid()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'good').write_text('DISPLAY=:1\n')
            self.assertEqual(cu.read_session_file(root / 'good', uid), 'DISPLAY=:1\n')
            with self.assertRaises(OSError):
                cu.read_session_file(root / 'good', uid + 1)
            os.symlink(root / 'good', root / 'link')
            with self.assertRaises(OSError):
                cu.read_session_file(root / 'link', uid)
            os.mkfifo(root / 'pipe')
            with self.assertRaises(OSError):
                cu.read_session_file(root / 'pipe', uid)
            (root / 'big').write_text('A' * 100000)
            self.assertEqual(len(cu.read_session_file(root / 'big', uid)), 8192)


class BrowserLaunchers(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__('shutil').rmtree(self.root, ignore_errors=True))
        self.bin = self.root / 'bin'
        self.apps = self.root / 'applications'
        self.bin.mkdir()

    def write(self, container):
        env = {'SILO_RUNTIME': 'container'} if container else {}
        with mock.patch.dict(os.environ, env, clear=False):
            if not container:
                os.environ.pop('SILO_RUNTIME', None)
            cu.write_browser_launchers(self.bin, self.apps)

    def test_a_container_gets_a_no_sandbox_launcher_for_each_browser_name(self):
        self.write(True)
        for name in cu.BROWSER_NAMES:
            launcher = self.bin / name
            self.assertTrue(os.access(launcher, os.X_OK), name)
            self.assertIn('--no-sandbox', launcher.read_text())
        # Each packaged desktop entry is replaced by one that runs the launcher, which adds the
        # flag once.
        for entry_name, launcher in (('google-chrome.desktop', 'google-chrome-stable'),
                                     ('chromium.desktop', 'chromium')):
            entry = (self.apps / entry_name).read_text()
            self.assertIn(f'Exec={self.bin}/{launcher} %U\n', entry)
            self.assertNotIn('--no-sandbox', entry)

    def test_an_entry_this_helper_did_not_write_is_left_alone(self):
        self.apps.mkdir()
        (self.apps / 'chromium.desktop').write_text('[Desktop Entry]\nName=Mine\n')
        self.write(True)
        self.assertEqual((self.apps / 'chromium.desktop').read_text(), '[Desktop Entry]\nName=Mine\n')
        self.assertTrue((self.apps / 'google-chrome.desktop').exists())

    def test_a_virtual_machine_keeps_the_browsers_sandbox(self):
        self.write(False)
        self.assertEqual(list(self.bin.iterdir()), [])
        self.assertFalse(self.apps.exists())

    def test_a_file_this_helper_did_not_write_is_left_alone(self):
        (self.bin / 'chromium').write_text('#!/bin/sh\necho mine\n')
        self.write(True)
        self.assertEqual((self.bin / 'chromium').read_text(), '#!/bin/sh\necho mine\n')
        self.assertIn('--no-sandbox', (self.bin / 'google-chrome').read_text())

    def test_the_launcher_runs_the_first_installed_browser_with_the_flag_and_the_arguments(self):
        browser = self.root / 'chrome'
        browser.write_text('#!/bin/sh\nprintf "%s " "$@"\n')
        browser.chmod(0o755)
        text = cu.BROWSER_LAUNCHER_TEXT.replace('/opt/google/chrome/google-chrome', str(browser))
        launcher = self.root / 'launcher'
        launcher.write_text(text)
        launcher.chmod(0o755)
        result = subprocess.run([str(launcher), 'about:blank', '--x'], capture_output=True, text=True)
        self.assertEqual(result.stdout, '--no-sandbox about:blank --x ')

    def test_the_launcher_says_so_when_no_browser_is_installed(self):
        text = cu.BROWSER_LAUNCHER_TEXT
        for path in cu.BROWSER_BINARIES:
            text = text.replace(path, str(self.root / 'absent' / Path(path).name))
        launcher = self.root / 'launcher'
        launcher.write_text(text)
        launcher.chmod(0o755)
        result = subprocess.run([str(launcher)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 127)
        self.assertIn('No browser is installed', result.stderr)

    def test_an_update_rewrites_a_stale_launcher_and_keeps_it_executable(self):
        self.write(True)
        (self.bin / 'google-chrome').write_text(cu.BROWSER_LAUNCHER_MARK + ' old\n')
        self.write(True)
        self.assertEqual((self.bin / 'google-chrome').read_text(), cu.BROWSER_LAUNCHER_TEXT)


if __name__ == '__main__':
    unittest.main()
