import contextlib
import fcntl
import hashlib
import importlib.machinery
import importlib.util
import io
import json
import os
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
            mock.patch.object(cu, 'LEGACY_PREFIX', self.legacy_prefix),
            mock.patch.object(cu, 'LEGACY_RECEIPT', self.legacy_receipt),
            mock.patch.object(cu, 'CLAUDE_STATE', self.claude_state),
            mock.patch.object(cu, 'CLAUDE_SETTINGS', self.claude_settings),
            mock.patch.object(cu, 'CODEX_CONFIG', self.codex),
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

    def fake_session_env(self, proc=None, strict=False):
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
        asset = {'url': 'https://example.invalid/driver.tar.gz', 'sha256': self.sha, 'bytes': self.size}
        pinned = {'schemaVersion': 1, 'version': VERSION, 'source': 'https://example.invalid/',
                  'assets': {'arm64': dict(asset), 'amd64': dict(asset)}}
        pinned.update(changes)
        (self.state / 'pinned.json').write_text(json.dumps(pinned))

    def install_image_driver(self, version=VERSION):
        self.link.write_text(version)

    def fake_run(self, argv, *, user=False, timeout=900, check=True, cwd=None, extra_env=None, quiet=False):
        self.commands.append((list(argv), bool(user), dict(extra_env or {})))
        name = Path(argv[0]).name
        if name == 'cua-driver' or argv[0] == str(self.link) or Path(argv[0]).name == 'cua-driver':
            path = Path(argv[0])
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
        return json.loads(self.claude_settings.read_text())['permissions']['allow']

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
                       {'assets': {'arm64': {'url': 'https://x/', 'sha256': 'abc', 'bytes': 1},
                                   'amd64': {'url': 'https://x/', 'sha256': self.sha, 'bytes': 1}}}):
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
        self.assertFalse(self.link.is_symlink())

    def test_without_the_driver_the_pinned_archive_is_downloaded_verified_and_linked(self):
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'ready')
        curl, = self.curls()
        self.assertEqual(curl[curl.index('--proto') + 1], '=https')
        self.assertIn('https://example.invalid/driver.tar.gz', curl)
        installed = self.root / 'opt-cua-driver' / VERSION
        # Only the binary is extracted.
        self.assertEqual([p.name for p in installed.iterdir()], ['cua-driver'])
        self.assertTrue(self.link.is_symlink())
        self.assertEqual(os.path.realpath(self.link), str(installed / 'cua-driver'))
        self.assertFalse((self.state / 'stage').exists())

    def test_an_image_driver_of_another_version_is_replaced_by_the_pinned_one(self):
        self.install_image_driver('0.30.0')
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(len(self.curls()), 1)
        self.assertEqual(self.link.read_text(), VERSION)

    def test_an_earlier_install_of_the_pinned_version_is_linked_without_a_download(self):
        cu.apply('ask')
        self.link.unlink()
        self.commands.clear()
        self.assertEqual(cu.apply('ask', force=True)['state'], 'ready')
        self.assertEqual(self.curls(), [])
        self.assertTrue(self.link.is_symlink())

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

    def test_the_download_uses_curls_own_retry_options(self):
        cu.apply('ask')
        curl, = self.curls()
        for option in ('--retry-all-errors', '--retry-connrefused', '--connect-timeout', '--max-time'):
            self.assertIn(option, curl)

    def test_a_hash_mismatch_is_refused_and_never_retried(self):
        asset = {'url': 'https://example.invalid/d.tar.gz', 'sha256': '0' * 64, 'bytes': self.size}
        self.write_pinned(assets={'arm64': asset, 'amd64': asset})
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'driver-archive-mismatch'))
        self.assertEqual(len(self.curls()), 1)
        self.assertEqual(self.sleeps, [])
        self.assertFalse(self.link.exists() or self.link.is_symlink())

    def test_a_size_mismatch_is_refused(self):
        asset = {'url': 'https://example.invalid/d.tar.gz', 'sha256': self.sha, 'bytes': self.size + 1}
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
        asset = {'url': 'https://example.invalid/d.tar.gz', 'sha256': sha, 'bytes': size}
        self.write_pinned(assets={'arm64': asset, 'amd64': asset})
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'driver-archive-invalid'))
        self.assertFalse(self.link.exists() or self.link.is_symlink())

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
    def setUp(self):
        super().setUp()
        self.install_image_driver()

    def test_claude_code_gets_a_plain_stdio_server_through_the_launcher(self):
        cu.apply('ask')
        self.assertEqual(self.claude()['mcpServers']['cua-driver'], {
            'type': 'stdio', 'command': str(cu.LAUNCHER), 'args': [],
            'env': {'CUA_DRIVER_RS_TELEMETRY_ENABLED': '0', 'CUA_DRIVER_RS_UPDATE_CHECK': 'false'}})

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
            'permissions': {'allow': ['mcp__lcu', 'Bash(ls)'], 'deny': ['mcp__lcu__host_only']},
            'hooks': {'Stop': [{'hooks': [{'type': 'command', 'command': '/opt/lcu/current/bin/lcu hook'}]},
                               {'hooks': [{'type': 'command', 'command': 'echo mine'}]}],
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
        self.assertEqual(settings['permissions'], {'allow': ['Bash(ls)'], 'deny': []})
        self.assertEqual(settings['hooks'], {'Stop': [{'hooks': [{'type': 'command', 'command': 'echo mine'}]}]})
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
    def test_the_environment_is_read_from_the_session_process(self):
        with tempfile.TemporaryDirectory() as directory:
            proc = Path(directory)
            for pid, comm, environ in ((10, 'bash', 'DISPLAY=:9'),
                                       (11, 'xfce4-session',
                                        'DISPLAY=:1\0DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/dbus-Z\0'
                                        'XDG_RUNTIME_DIR=/run/silo-desktop/user\0XAUTHORITY=/run/x\0PATH=/bin')):
                (proc / str(pid)).mkdir()
                (proc / str(pid) / 'comm').write_text(comm + '\n')
                (proc / str(pid) / 'environ').write_bytes(environ.encode())
            with mock.patch.object(cu.pwd, 'getpwnam', side_effect=KeyError):
                found = cu.session_env(proc)
        self.assertEqual(found, {'DISPLAY': ':1', 'XDG_RUNTIME_DIR': '/run/silo-desktop/user',
                                 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/tmp/dbus-Z', 'XAUTHORITY': '/run/x'})

    def test_without_a_session_process_defaults_apply_and_strict_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(cu.session_env(Path(directory)),
                             {'DISPLAY': ':1', 'XDG_RUNTIME_DIR': '/run/silo-desktop/user'})
            with self.assertRaises(cu.Failure) as caught:
                cu.session_env(Path(directory), strict=True)
        self.assertEqual(caught.exception.reason, 'session-unavailable')


class Launcher(unittest.TestCase):
    def load(self):
        spec = importlib.util.spec_from_file_location('launcher', SCRIPT.parent / 'silo-cua-driver-mcp',
                                                      loader=importlib.machinery.SourceFileLoader(
                                                          'launcher', str(SCRIPT.parent / 'silo-cua-driver-mcp')))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def proc(self, directory, pid, comm, environ, uid=None):
        path = Path(directory) / str(pid)
        path.mkdir()
        (path / 'comm').write_text(comm + '\n')
        (path / 'environ').write_bytes(environ.encode())

    def test_it_reads_the_current_session_of_its_own_user(self):
        launcher = self.load()
        with tempfile.TemporaryDirectory() as directory:
            self.proc(directory, 5, 'xfce4-session', 'DISPLAY=:1\0DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/dbus-NEW\0'
                      'XDG_RUNTIME_DIR=/run/silo-desktop/user\0XAUTHORITY=/run/xauth\0PATH=/bin')
            self.proc(directory, 6, 'bash', 'DISPLAY=:9')
            found = launcher.session_env(directory, uid=os.getuid())
            self.assertEqual(found, {'DISPLAY': ':1', 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/tmp/dbus-NEW',
                                     'XDG_RUNTIME_DIR': '/run/silo-desktop/user', 'XAUTHORITY': '/run/xauth'})
            # Another user's session is not used.
            other = launcher.session_env(directory, uid=os.getuid() + 1)
            self.assertNotIn('XAUTHORITY', other)
            self.assertEqual(other['DISPLAY'], ':1')

    def test_without_a_session_it_falls_back_to_the_standard_addresses(self):
        launcher = self.load()
        with tempfile.TemporaryDirectory() as directory:
            found = launcher.session_env(directory, uid=1234)
        self.assertEqual(found['DISPLAY'], ':1')
        self.assertTrue(found['DBUS_SESSION_BUS_ADDRESS'].endswith('/bus'))

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


if __name__ == '__main__':
    unittest.main()
