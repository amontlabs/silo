import fcntl
import hashlib
import importlib.util
import io
import json
import os
import stat
import subprocess
import tarfile
import tempfile
import time
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
SPEC = importlib.util.spec_from_file_location('silo_computer_use', SCRIPT)
cu = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cu)
REAL_RUN = cu.run

APP_DIR = '26.928.31416-arm64'
ARCHIVE = 'lcu-0.8.0-linux-arm64.tar.gz'


def make_archive(directory):
    """A release archive with the one file the installer step needs."""
    path = Path(directory) / ARCHIVE
    root = 'lcu-0.8.0-linux-arm64'
    with tarfile.open(path, 'w:gz') as archive:
        for name, body, mode in ((f'{root}/scripts/install.sh', b'#!/bin/sh\n', 0o755),
                                 (f'{root}/README.md', b'lcu\n', 0o644)):
            info = tarfile.TarInfo(name)
            info.size = len(body)
            info.mode = mode
            archive.addfile(info, io.BytesIO(body))
    return path, hashlib.sha256(path.read_bytes()).hexdigest()


class Guest(unittest.TestCase):
    """A fake guest: temp state/mount/image directories and scripted lcu commands."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.state = root / 'state'
        self.mount = root / 'mount'
        self.image = root / 'image'
        # The host's read-only LCU folder; absent unless a test creates it.
        self.host_lcu = root / 'host-lcu'
        for directory in (self.state, self.mount / APP_DIR, self.image):
            directory.mkdir(parents=True)
        self.archive, self.sha = make_archive(self.image)
        # What the pinned URL serves (kept outside the image directory).
        self.served = root / 'served.tar.gz'
        self.served.write_bytes(self.archive.read_bytes())
        self.commands = []
        self.lcu_status = {'lcu_version': '0.8.0', 'setup': {'approval': 'ask'},
                           'cross_turn': {'enabled': True, 'source': 'unattended'},
                           'compatibility': {'status': 'tested', 'warning': None},
                           'app': {'path': str(self.mount / APP_DIR), 'version': '26.928.31416',
                                   'runtime': '0.0.27/20260927214556-b77d38801cca'}}
        self.setup_output = 'Claude Code: MCP registered.\nCodex: MCP registered.\nCodex: hooks registered.\n'
        self.failures = {}
        # The next N curl runs fail (an empty reply), then it works.
        self.curl_failures_left = 0
        self.sleeps = []
        # What `lcu setup` exits with (it prints `setup_output` either way).
        self.setup_code = 0
        # The desktop session: True (running), False (still starting), a state name, or
        # a list of states reported in turn (the last one repeats).
        self.session = True
        # What a `silo-desktop start` does to the session (None: nothing).
        self.start_effect = None
        self.autostart = True
        self.installed = False
        # Whether this LCU documents `setup --allow-missing` and `--reconcile` (0.8.8), what
        # `lcu status --json` lists as pending, and what the reconcile finds installed.
        self.new_lcu = True
        self.pending = []
        self.reconciled = []
        self.spawned = []
        self.hook = root / 'profile-hook.sh'
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
            mock.patch.object(cu, 'IMAGE_DIR', self.image),
            mock.patch.object(cu, 'HOST_DIR', self.host_lcu),
            mock.patch.object(cu, 'MOUNT', self.mount),
            mock.patch.object(cu, 'PREFIX', root / 'opt-lcu'),
            mock.patch.object(cu, 'mount_state', lambda *a, **k: self.mount_state),
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
        self.mount_state = 'ok'
        self.write_pinned()
        (root / 'desktop-config.json').write_text(json.dumps({'autoStart': True}))

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

    def write_pinned(self, **lcu):
        pinned = {'schemaVersion': 1,
                  'app': {'dir': APP_DIR, 'version': '26.928.31416', 'runtime': '0.0.27/20260927214556-b77d38801cca'},
                  'lcu': {'version': '0.8.0', 'archive': ARCHIVE, 'sha256': self.sha,
                          'url': 'https://example.invalid/' + ARCHIVE}}
        pinned['lcu'].update(lcu)
        (self.state / 'pinned.json').write_text(json.dumps(pinned))

    def pinned_lcu(self):
        return json.loads((self.state / 'pinned.json').read_text())

    def fake_run(self, argv, *, user=False, timeout=900, check=True, cwd=None, extra_env=None, quiet=False):
        self.commands.append((list(argv), bool(user), cwd))
        name = Path(argv[0]).name
        if name == 'tar':
            return REAL_RUN(argv, timeout=timeout, check=check, quiet=quiet)
        key = ' '.join(argv[1:3]) if name == 'lcu' else name
        failure = self.failures.get(name) or self.failures.get(key)
        if failure:
            if check and failure != 'soft':
                raise cu.Failure('command-failed', name)
            return subprocess.CompletedProcess(argv, 1, stdout='', stderr='')
        if name == 'silo-desktop':
            if argv[1:2] == ['start'] and self.start_effect is not None:
                self.session = self.start_effect
            return subprocess.CompletedProcess(argv, 0, stdout='', stderr='')
        if name == 'install.sh':
            self.installed = True
            (cu.PREFIX / 'current/bin').mkdir(parents=True, exist_ok=True)
            (cu.PREFIX / 'current/bin/lcu').write_text('#!/bin/sh\n')
        if name == 'lcu' and argv[1:3] == ['status', '--json']:
            if not self.installed:
                return subprocess.CompletedProcess(argv, 1, stdout='', stderr='')
            report = dict(self.lcu_status, pending=list(self.pending)) if self.new_lcu else self.lcu_status
            return subprocess.CompletedProcess(argv, 0, stdout=json.dumps(report), stderr='')
        if name == 'lcu' and argv[1:3] == ['setup', '--help']:
            usage = 'usage: lcu setup [--agent ID] [--allow-missing] [--cross-turn on|off] [--reconcile]\n' if self.new_lcu else 'usage: lcu setup [--agent ID]\n'
            return subprocess.CompletedProcess(argv, 0, stdout=usage, stderr='')
        if name == 'lcu' and argv[1:3] == ['setup', '--reconcile']:
            self.pending = [a for a in self.pending if a not in self.reconciled]
            return subprocess.CompletedProcess(argv, 0, stdout='', stderr='')
        if name == 'lcu' and argv[1:2] == ['setup']:
            return subprocess.CompletedProcess(argv, self.setup_code, stdout=self.setup_output, stderr='')
        if name == 'curl' and self.curl_failures_left > 0:
            self.curl_failures_left -= 1
            raise cu.Failure('command-failed', 'curl exited 52')
        if name == 'curl':
            Path(argv[argv.index('--output') + 1]).write_bytes(self.served.read_bytes())
        return subprocess.CompletedProcess(argv, 0, stdout='', stderr='')

    def lcu_commands(self):
        return [argv for argv, *_ in self.commands if Path(argv[0]).name in ('lcu', 'lcu-session')
                or Path(argv[0]).name == 'install.sh' or 'install.sh' in argv[0]]

    def receipt(self):
        return json.loads((self.state / 'receipt.json').read_text())


class Status(Guest):
    def test_without_the_pinned_pair_nothing_is_configured(self):
        (self.state / 'pinned.json').unlink()
        self.assertEqual(cu.status()['state'], 'not-set-up')
        self.assertEqual(cu.status()['reason'], 'not-configured')

    def test_a_malformed_pair_is_ignored(self):
        for lcu in ({'sha256': 'abc'}, {'archive': '../x.tar.gz'}, {'version': '$(id)'}):
            self.write_pinned(**lcu)
            self.assertIsNone(cu.load_pinned(), lcu)
        self.write_pinned()
        pinned = json.loads((self.state / 'pinned.json').read_text())
        pinned['app']['dir'] = '../../etc'
        (self.state / 'pinned.json').write_text(json.dumps(pinned))
        self.assertIsNone(cu.load_pinned())

    def test_a_missing_app_folder_needs_the_app(self):
        (self.mount / APP_DIR).rmdir()
        result = cu.status()
        self.assertEqual((result['state'], result['reason']), ('needs-app', 'app-missing'))

    def test_a_symlinked_app_folder_is_not_present(self):
        (self.mount / APP_DIR).rmdir()
        (self.mount / APP_DIR).symlink_to(self.image)
        self.assertEqual(cu.status()['state'], 'needs-app')

    def test_status_reads_the_receipt_only(self):
        cu.apply('ask')
        self.commands.clear()
        result = cu.status()
        self.assertEqual(self.commands, [], 'status never runs LCU')
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(result['compatibility'], 'tested')
        self.assertEqual(result['appVersion'], '26.928.31416')
        self.assertEqual(result['lcuVersion'], '0.8.0')
        self.assertEqual(result['agents'], ['claude-code', 'codex'])

    def test_a_receipt_for_another_pair_does_not_count(self):
        cu.apply('ask')
        self.write_pinned(version='0.8.1')
        self.assertEqual(cu.status()['state'], 'not-set-up')

    def test_a_held_lock_means_installing_and_a_stale_installing_receipt_failed(self):
        cu.write_receipt(cu.load_pinned(), 'ask', 'installing')
        self.assertEqual(cu.status()['reason'], 'interrupted')
        with open(self.state / 'lock', 'a') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX)
            self.assertEqual(cu.status()['state'], 'installing')

    def test_the_mount_must_be_read_only_and_present(self):
        cu.apply('ask')
        for mount in ('missing', 'writable'):
            self.mount_state = mount
            result = cu.status()
            self.assertEqual((result['state'], result['reason']), ('failed', 'mount-' + mount))

    def test_mount_state_parses_proc_mounts(self):
        mounts = Path(self.tmp.name) / 'mounts'
        target = '/opt/silo/chatgpt'
        mounts.write_text(f'virtiofs {target} virtiofs ro,relatime 0 0\n')
        self.assertEqual(REAL_MOUNT_STATE(Path(target), mounts), 'ok')
        mounts.write_text(f'virtiofs {target} virtiofs rw,relatime 0 0\n')
        self.assertEqual(REAL_MOUNT_STATE(Path(target), mounts), 'writable')
        mounts.write_text('tmpfs /tmp tmpfs rw 0 0\n')
        self.assertEqual(REAL_MOUNT_STATE(Path(target), mounts), 'missing')
        self.assertEqual(REAL_MOUNT_STATE(Path(target), Path(self.tmp.name) / 'absent'), 'missing')


REAL_MOUNT_STATE = cu.mount_state


class Apply(Guest):
    def test_first_sync_installs_in_place_sets_up_and_checks_the_doctor(self):
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'ready')
        names = [Path(argv[0]).name for argv in self.lcu_commands()]
        self.assertEqual(names, ['install.sh', 'lcu', 'lcu', 'lcu', 'lcu', 'lcu', 'lcu', 'lcu-session'])
        install = next(argv for argv in self.lcu_commands() if argv[0].endswith('install.sh'))
        self.assertEqual(install[1:], ['--user', 'silo', '--runtime-only', '--skip-system', '--offline',
                                       '--existing-app', str(self.mount / APP_DIR), '--yes'])
        setup, = [(argv, user) for argv, user, _ in self.commands
                  if argv[1:2] == ['setup'] and '--session' in argv]
        self.assertEqual(setup[0][1:], ['setup', '--agent', 'all', '--allow-missing', '--cross-turn', 'on',
                                        '--unattended', '--session', 'direct', '--yes', '--approval', 'ask'])
        self.assertTrue(setup[1], 'setup runs as the working account')
        doctor = self.lcu_commands()[-1]
        self.assertTrue([user for argv, user, _ in self.commands if argv == doctor][0],
                        'the session launcher runs as the desktop account')
        self.assertEqual(doctor[1:], ['--user', 'silo', '--', str(cu.PREFIX / 'current/bin/lcu'),
                                      'doctor', '--non-interactive', '--require-ready'])
        receipt = self.receipt()
        self.assertEqual((receipt['state'], receipt['readiness'], receipt['approval']), ('ready', 'ready', 'ask'))
        self.assertEqual(receipt['archiveSha256'], self.sha)

    def test_the_archive_is_extracted_on_local_disk_and_cleaned_up(self):
        cu.apply('ask')
        install_cwd = next(cwd for argv, _, cwd in self.commands if argv[0].endswith('install.sh'))
        self.assertTrue(str(install_cwd).startswith(str(self.state / 'stage')))
        self.assertFalse((self.state / 'stage').exists())

    def test_an_up_to_date_vm_runs_nothing_between_boots(self):
        cu.apply('ask')
        self.commands.clear()
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(self.commands, [])

    def test_a_ready_receipt_is_reverified_on_a_healthy_boot_without_install_or_setup(self):
        cu.apply('ask')
        self.commands.clear()
        with mock.patch.object(cu, 'now', return_value=123456):
            result = cu.apply('ask', boot=True)
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(result['agents'], ['claude-code', 'codex'])
        self.assertEqual(result['apply']['outcome'], 'applied')
        self.assertTrue(any(argv[1:3] == ['status', '--json'] for argv, *_ in self.commands))
        self.assertTrue(any(argv[0].endswith('lcu-session') for argv, *_ in self.commands))
        self.assertFalse(any('--approval' in argv or argv[0].endswith('install.sh')
                             for argv, *_ in self.commands))
        # The boot asks LCU to register agents installed since (a quiet no-op otherwise).
        self.assertTrue(any(argv[1:3] == ['setup', '--reconcile'] for argv, *_ in self.commands))
        self.assertEqual(self.receipt()['verifiedAt'], 123456)

    def test_a_ready_receipt_does_not_hide_a_failed_new_boot(self):
        cu.apply('ask')
        self.commands.clear()
        self.session = 'failed'
        result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['readiness'], result['reason']),
                         ('failed', 'failed', 'desktop-session-not-running'))
        self.assertEqual(len(self.desktop_starts()), cu.SESSION_REPAIR_ATTEMPTS)
        self.assertEqual(result['apply']['outcome'], 'applied')
        self.assertEqual(cu.status()['state'], 'failed')

    def test_a_ready_receipt_does_not_hide_a_failed_doctor_on_a_new_boot(self):
        cu.apply('ask')
        self.commands.clear()
        self.failures['lcu-session'] = 'soft'
        result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['readiness'], result['reason']),
                         ('failed', 'failed', 'doctor-failed'))
        self.assertEqual(result['apply']['outcome'], 'applied')

    def test_a_ready_receipt_can_repair_the_new_boot_before_rechecking_readiness(self):
        cu.apply('ask')
        self.commands.clear()
        self.session = 'failed'
        self.start_effect = 'running'
        result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['readiness']), ('ready', 'ready'))
        self.assertEqual(len(self.desktop_starts()), 1)
        self.assertTrue(any(argv[0].endswith('lcu-session') for argv, *_ in self.commands))

    def test_a_changed_approval_reruns_setup_only(self):
        cu.apply('ask')
        self.commands.clear()
        result = cu.apply('auto')
        self.assertEqual(result['apply'], {'approval': 'auto', 'outcome': 'applied', 'reason': None})
        names = [Path(argv[0]).name for argv in self.lcu_commands()]
        self.assertNotIn('install.sh', names)
        setup = next(argv for argv, *_ in self.commands if argv[1:2] == ['setup'] and '--approval' in argv)
        self.assertEqual(setup[-2:], ['--approval', 'auto'])
        # An explicit ask is applied too (it removes the entries).
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'applied')
        self.assertEqual(self.receipt()['approval'], 'ask')

    def test_every_request_is_executed_in_the_order_it_arrives(self):
        # The helper keeps no ordering record: the host serializes runs and decides the mode,
        # so a request is never ignored as older than something remembered.
        for mode in ('auto', 'ask', 'auto', 'auto', 'ask'):
            result = cu.apply(mode)
            self.assertEqual(result['apply'], {'approval': mode, 'outcome': 'applied', 'reason': None})
            self.assertEqual(self.receipt()['approval'], mode)
        self.assertFalse((self.state / 'approval.json').exists())

    def test_status_reports_no_approval_state(self):
        cu.apply('auto')
        result = cu.status()
        for field in ('approval', 'approvalRevision', 'approvalGeneration', 'approvalConfirmed', 'apply'):
            self.assertNotIn(field, result)

    def test_a_forged_record_on_the_guest_disk_changes_nothing(self):
        # Agents in the computer have root: the approval switch is not a boundary, and the helper
        # no longer reads any record of it. A stale or forged file is simply ignored.
        (self.state / 'approval.json').write_text(json.dumps({'approval': 'auto', 'revision': 2 ** 62}))
        result = cu.apply('ask')
        self.assertEqual(result['apply']['outcome'], 'applied')
        self.assertEqual(self.receipt()['approval'], 'ask')

    def test_a_failed_setup_is_reported_failed_and_retried(self):
        cu.apply('auto')
        self.setup_code = 1
        self.setup_output = 'Codex: approval failed: boom\n'
        result = cu.apply('ask')
        self.assertEqual(result['apply'], {'approval': 'ask', 'outcome': 'failed', 'reason': 'setup-failed'})
        self.assertEqual((result['state'], result['reason']), ('failed', 'setup-failed'))
        # The same request again runs setup again and succeeds.
        self.setup_code = 0
        self.setup_output = 'Codex: MCP registered.\n'
        self.commands.clear()
        result = cu.apply('ask')
        self.assertTrue(any(argv[1:2] == ['setup'] for argv, *_ in self.commands))
        self.assertEqual((result['state'], result['apply']['outcome']), ('ready', 'applied'))

    def test_a_timed_out_setup_is_reported_failed(self):
        original = self.fake_run
        def timing_out(argv, **kwargs):
            if argv[1:2] == ['setup']:
                raise cu.Failure('timed-out', 'lcu')
            return original(argv, **kwargs)
        with mock.patch.object(cu, 'run', timing_out):
            result = cu.apply('auto')
        self.assertEqual(result['apply'], {'approval': 'auto', 'outcome': 'failed', 'reason': 'timed-out'})

    def test_some_agents_configured_and_others_failed_is_partial(self):
        self.setup_code = 1
        self.setup_output = ('Claude Code: MCP registered.\nClaude Code: approval auto: added.\n'
                             'Codex: MCP registered.\nCodex: approval failed: permission denied\n')
        result = cu.apply('auto')
        self.assertEqual(result['apply'], {'approval': 'auto', 'outcome': 'partial', 'reason': 'setup-partial'})
        # The readiness check still ran and the state follows it.
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(self.receipt()['approvalOutcome'], 'partial')
        # A partial run is never skipped as already applied.
        self.commands.clear()
        cu.apply('auto')
        self.assertTrue(any(argv[1:2] == ['setup'] for argv, *_ in self.commands))

    def test_an_unexplained_failure_after_configuring_some_agents_is_partial(self):
        # When unsure, partial: `failed` would claim nothing changed.
        self.setup_code = 1
        self.setup_output = 'Codex: MCP registered.\nTraceback (most recent call last):\n'
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'partial')

    def test_a_failure_line_with_a_zero_exit_is_not_applied(self):
        self.setup_output = 'Pi: extension registered.\nCodex: MCP failed: boom\n'
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'partial')
        self.setup_output = 'Codex: MCP failed: boom\n'
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'failed')

    def test_an_unrelated_phase_failing_keeps_the_explicit_approval_success(self):
        # LCU v0.8.1 prints a failed cleanup and carries on with the registration and approval.
        self.setup_code = 1
        self.setup_output = ('Codex: old skill cleanup failed: skill installer exited 1: EACCES\n'
                             'Codex: MCP registered.\nCodex: approval auto: added.\n'
                             'Error: 1 registration step(s) failed. Completed steps remain installed.\n')
        result = cu.apply('auto')
        self.assertEqual(result['apply'], {'approval': 'auto', 'outcome': 'applied', 'reason': None})
        self.assertEqual(result['agents'], ['codex'])
        # The readiness check still ran.
        self.assertTrue(any(argv[0].endswith('lcu-session') for argv, *_ in self.commands))
        self.assertEqual(self.receipt()['approvalOutcome'], 'applied')

    def test_a_failure_after_the_configuration_was_saved_keeps_the_approval_success(self):
        # `lcu setup` exits 2 when its own desktop readiness check fails after saving.
        self.setup_code = 2
        self.setup_output = ('Claude Code: MCP registered.\nClaude Code: approval ask: removed.\n'
                             'Codex: MCP registered.\nCodex: approval ask: removed.\n')
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'applied')

    def test_a_cleanup_failure_beside_a_real_failure_is_partial_or_failed_by_what_was_configured(self):
        self.setup_code = 1
        self.setup_output = ('Claude Code: old skill cleanup failed: boom\nClaude Code: MCP registered.\n'
                             'Claude Code: approval auto: added.\n'
                             'Codex: MCP failed: installer exited 1\n')
        result = cu.apply('auto')
        self.assertEqual(result['apply'], {'approval': 'auto', 'outcome': 'partial', 'reason': 'setup-partial'})
        self.assertTrue(any(argv[0].endswith('lcu-session') for argv, *_ in self.commands))
        self.setup_output = 'Codex: old skill cleanup failed: boom\nCodex: MCP failed: installer exited 1\n'
        self.assertEqual(cu.apply('auto')['apply']['outcome'], 'failed')

    def test_nothing_configured_and_a_failing_exit_is_failed(self):
        self.setup_code = 2
        self.setup_output = 'No agents detected\n'
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'failed')

    def test_a_cross_turn_setting_lcu_did_not_store_fails_the_setup_and_is_retried(self):
        # `lcu setup` warns and exits 0 when it cannot write the setting.
        self.lcu_status['cross_turn'] = {'enabled': False, 'source': None}
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'cross-turn-failed'))
        self.assertEqual(self.receipt()['state'], 'failed')
        self.lcu_status['cross_turn'] = {'enabled': True, 'source': 'unattended'}
        self.commands.clear()
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertTrue(any(argv[1:2] == ['setup'] and '--cross-turn' in argv for argv, *_ in self.commands))

    def test_an_lcu_without_cross_turn_is_not_asked_for_it(self):
        self.new_lcu = False
        self.lcu_status['cross_turn'] = {'enabled': False}
        self.assertEqual(cu.apply('ask')['state'], 'ready')

    def test_a_failed_readiness_check_does_not_change_the_approval_outcome(self):
        self.failures['lcu-session'] = 'soft'
        result = cu.apply('auto')
        self.assertEqual((result['state'], result['reason']), ('failed', 'doctor-failed'))
        self.assertEqual(result['apply'], {'approval': 'auto', 'outcome': 'applied', 'reason': None})

    def test_without_the_app_the_run_reports_the_reason_and_runs_nothing(self):
        (self.mount / APP_DIR).rmdir()
        self.assertEqual(cu.apply('auto')['apply'],
                         {'approval': 'auto', 'outcome': 'failed', 'reason': 'app-missing'})
        (self.mount / APP_DIR).mkdir()
        self.mount_state = 'writable'
        self.assertEqual(cu.apply('auto')['apply']['reason'], 'mount-writable')
        (self.state / 'pinned.json').unlink()
        self.assertEqual(cu.apply('auto')['apply']['reason'], 'not-configured')
        self.assertEqual(self.commands, [])

    def test_force_reruns_setup_for_agents_installed_later(self):
        cu.apply('ask')
        self.commands.clear()
        cu.apply('ask', force=True)
        self.assertTrue(any(argv[1:2] == ['setup'] for argv, *_ in self.commands))
        self.assertFalse(any(argv[0].endswith('install.sh') for argv, *_ in self.commands))

    def test_agents_are_the_ones_setup_confirmed(self):
        self.assertEqual(cu.registered_agents(self.setup_output), ['claude-code', 'codex'])
        self.assertEqual(cu.registered_agents('Pi: extension registered.\nOh My Pi: plugin registered.\n'),
                         ['oh-my-pi', 'pi'])
        self.assertEqual(cu.registered_agents('Codex: MCP failed: boom\nNo agents detected\n'), [])
        self.setup_output = ''
        self.assertEqual(cu.apply('ask')['agents'], [])

    def test_lcu_installed_for_another_app_folder_is_installed_again(self):
        cu.apply('ask')
        self.lcu_status['app']['path'] = '/somewhere/else'
        self.commands.clear()
        cu.apply('ask', force=True)
        self.assertTrue(any(argv[0].endswith('install.sh') for argv, *_ in self.commands))

    def test_a_new_app_folder_installs_again_against_it(self):
        cu.apply('ask')
        (self.mount / '26.929.1-arm64').mkdir()
        pinned = json.loads((self.state / 'pinned.json').read_text())
        pinned['app']['dir'] = '26.929.1-arm64'
        (self.state / 'pinned.json').write_text(json.dumps(pinned))
        self.commands.clear()
        cu.apply('ask')
        install = next(argv for argv, *_ in self.commands if argv[0].endswith('install.sh'))
        self.assertEqual(install[install.index('--existing-app') + 1], str(self.mount / '26.929.1-arm64'))

    def test_a_missing_app_folder_installs_nothing(self):
        (self.mount / APP_DIR).rmdir()
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'needs-app')
        self.assertEqual(self.commands, [])

    def test_a_missing_mount_installs_nothing(self):
        self.mount_state = 'missing'
        self.assertEqual(cu.apply('ask')['reason'], 'mount-missing')
        self.assertEqual(self.commands, [])

    def test_untested_pairs_are_reported_not_blocked(self):
        self.lcu_status['compatibility'] = {'status': 'untested', 'warning': 'Not tested with this app.'}
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['compatibility']), ('ready', 'untested'))
        self.assertEqual(result['warning'], 'Not tested with this app.')

    def test_a_failed_install_is_recorded_and_retried(self):
        self.failures['install.sh'] = True
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'command-failed'))
        self.failures.clear()
        self.assertEqual(cu.apply('ask')['state'], 'ready')

    def test_a_failed_doctor_fails_the_setup(self):
        self.failures['lcu-session'] = 'soft'
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'doctor-failed'))

    def test_a_desktop_that_never_starts_fails_the_setup(self):
        self.session = False
        times = iter(range(0, 10_000, 100))
        with mock.patch.object(cu.time, 'monotonic', lambda: next(times)):
            result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['reason']), ('failed', 'desktop-session-not-running'))

    def desktop_starts(self):
        return [argv for argv, *_ in self.commands if argv[:2] == [cu.DESKTOP_COMMAND, 'start']]

    def test_a_session_that_failed_during_boot_is_started_again_before_the_doctor(self):
        # The boot hook's desktop start failed (a stale PulseAudio pid file in the log):
        # the session is `failed` when the helper looks, and `start` brings it back.
        self.session = 'failed'
        self.start_effect = 'running'
        result = cu.apply('ask', boot=True)
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(len(self.desktop_starts()), 1)
        self.assertLess(self.commands.index(next(c for c in self.commands if c[0] == self.desktop_starts()[0])),
                        self.commands.index(next(c for c in self.commands if c[0][0].endswith('lcu-session'))),
                        'the doctor runs only once the session exists')

    def test_a_session_still_starting_is_waited_for_without_starting_it_again(self):
        self.session = ['starting', 'starting', 'starting', 'running']
        result = cu.apply('ask', boot=True)
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(self.desktop_starts(), [])

    def test_repair_waits_with_backoff_and_gives_up_after_three_starts(self):
        self.session = 'failed'
        slept = []
        with mock.patch.object(cu.time, 'sleep', slept.append):
            result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['reason']), ('failed', 'desktop-session-not-running'))
        self.assertEqual(len(self.desktop_starts()), cu.SESSION_REPAIR_ATTEMPTS)
        self.assertEqual([n for n in slept if n >= cu.SESSION_REPAIR_BASE][:3], [2, 4, 8])
        self.assertFalse(any(c[0][0].endswith('lcu-session') for c in self.commands), 'no doctor without a session')

    def test_session_repair_does_not_restart_after_the_wait_budget_expires(self):
        self.session = 'failed'
        now = [0.0]
        starts = []

        def sleep(delay):
            now[0] += delay

        def start():
            starts.append(now[0])
            now[0] += 4

        with mock.patch.object(cu.time, 'monotonic', lambda: now[0]), \
             mock.patch.object(cu.time, 'sleep', sleep), \
             mock.patch.object(cu, 'start_desktop', start):
            with self.assertRaises(cu.Failure):
                cu.wait_for_session(3, repair=True)
        self.assertEqual(starts, [2])

    def test_session_repair_backoff_respects_the_remaining_wait_budget(self):
        self.session = 'failed'
        now = [0.0]
        slept = []

        def sleep(delay):
            slept.append(delay)
            now[0] += delay

        with mock.patch.object(cu.time, 'monotonic', lambda: now[0]), \
             mock.patch.object(cu.time, 'sleep', sleep):
            with self.assertRaises(cu.Failure):
                cu.wait_for_session(1, repair=True)
        self.assertEqual(slept, [1])
        self.assertEqual(self.desktop_starts(), [])

    def test_a_stopped_session_is_not_restarted_when_the_desktop_is_manual(self):
        (Path(self.tmp.name) / 'desktop-config.json').write_text(json.dumps({'autoStart': False}))
        self.session = 'stopped'
        times = iter(range(0, 10_000, 100))
        with mock.patch.object(cu.time, 'monotonic', lambda: next(times)):
            result = cu.apply('ask', boot=True)
        self.assertEqual((result['state'], result['reason']), ('failed', 'desktop-session-not-running'))
        self.assertEqual(self.desktop_starts(), [])

    def test_a_later_sync_never_restarts_a_desktop_the_user_stopped(self):
        self.session = 'stopped'
        times = iter(range(0, 10_000, 100))
        with mock.patch.object(cu.time, 'monotonic', lambda: next(times)):
            result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'desktop-session-not-running'))
        self.assertEqual(self.desktop_starts(), [])

    def test_the_hosts_archive_is_preferred_over_the_staged_one_and_the_download(self):
        self.host_lcu.mkdir()
        (self.host_lcu / ARCHIVE).write_bytes(self.archive.read_bytes())
        # The staged archive is stale; the host's matching copy is used and nothing is fetched.
        (self.image / ARCHIVE).write_bytes(b'stale staged archive')
        with mock.patch.object(cu, 'sha256_file', wraps=cu.sha256_file) as hashed:
            used = cu.archive_path(self.pinned_lcu(), self.state / 'stage')
        self.assertEqual(used, self.host_lcu / ARCHIVE)
        self.assertEqual(hashed.call_count, 1)
        self.assertEqual(self.curls(), [])

    def test_a_host_archive_that_does_not_match_falls_back_to_the_staged_one(self):
        self.host_lcu.mkdir()
        (self.host_lcu / ARCHIVE).write_bytes(b'tampered')
        used = cu.archive_path(self.pinned_lcu(), self.state / 'stage')
        self.assertEqual(used, self.image / ARCHIVE)
        self.assertEqual(self.curls(), [])

    def test_a_host_archive_that_is_a_symlink_is_ignored(self):
        self.host_lcu.mkdir()
        (self.host_lcu / ARCHIVE).symlink_to(self.image / ARCHIVE)
        used = cu.archive_path(self.pinned_lcu(), self.state / 'stage')
        self.assertEqual(used, self.image / ARCHIVE)

    def test_a_staged_archive_that_does_not_match_is_downloaded_and_verified(self):
        # The staged file no longer matches the lock, so the pinned URL is used.
        (self.image / ARCHIVE).write_bytes(b'tampered')
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'ready')
        self.assertTrue(any(argv[0] == 'curl' for argv, *_ in self.commands))

    def test_an_existing_install_of_the_previous_release_is_upgraded_in_place_from_the_locked_url(self):
        # A v4 computer: LCU 0.8.1 installed from the image's staged archive (0.8.1), a receipt for it.
        old_sha = hashlib.sha256(b'previous release archive').hexdigest()
        (self.image / ARCHIVE).write_bytes(b'previous release archive')
        self.write_pinned(version='0.8.1', sha256=old_sha)
        self.lcu_status['lcu_version'] = '0.8.1'
        cu.apply('ask')
        self.assertEqual(self.receipt()['archiveSha256'], old_sha)
        # The new lock pins 0.8.3; the staged archive no longer matches it.
        self.write_pinned(version='0.8.3', sha256=self.sha, url='https://example.invalid/' + ARCHIVE)
        self.commands.clear()
        result = cu.apply('ask', boot=True)
        self.assertEqual(result['state'], 'ready')
        curl, = [argv for argv, *_ in self.commands if argv[0] == 'curl']
        self.assertIn('https://example.invalid/' + ARCHIVE, curl)
        install = [argv for argv, *_ in self.commands if argv[0].endswith('install.sh')]
        self.assertEqual(len(install), 1, 'the install replaces the old one at the same prefix')
        self.assertIn('--runtime-only', install[0])
        self.assertEqual(self.receipt()['archiveSha256'], self.sha)
        self.assertFalse((self.state / 'stage').exists())

    def test_an_archive_that_cannot_be_obtained_fails_clearly(self):
        (self.image / ARCHIVE).unlink()
        self.failures['curl'] = True
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'lcu-archive-unavailable'))

    def curls(self):
        return [argv for argv, *_ in self.commands if argv[0] == 'curl']

    def test_a_download_that_fails_a_few_times_is_retried_with_backoff_and_logged(self):
        (self.image / ARCHIVE).unlink()
        self.curl_failures_left = 3
        result = cu.apply('ask')
        self.assertEqual(result['state'], 'ready')
        self.assertEqual(len(self.curls()), 4)
        self.assertEqual(self.sleeps, [5, 10, 20])
        log = cu.LOG.read_text()
        self.assertIn('attempt 4 of 5', log)
        self.assertIn('LCU download attempt 1 failed', log)

    def test_the_download_uses_https_only_and_curls_own_retry_options(self):
        (self.image / ARCHIVE).unlink()
        cu.apply('ask')
        curl, = self.curls()
        for option in ('--retry-all-errors', '--retry-connrefused', '--connect-timeout', '--max-time'):
            self.assertIn(option, curl)
        self.assertEqual(curl[curl.index('--proto') + 1], '=https')

    def test_a_download_that_keeps_failing_gives_up_after_five_attempts(self):
        (self.image / ARCHIVE).unlink()
        self.curl_failures_left = 99
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'lcu-archive-unavailable'))
        self.assertEqual(len(self.curls()), 5)
        self.assertEqual(self.sleeps, [5, 10, 20, 40])
        self.assertFalse(any(argv[0].endswith('install.sh') for argv, *_ in self.commands))

    def test_a_hash_mismatch_is_never_retried(self):
        (self.image / ARCHIVE).unlink()
        self.write_pinned(sha256='0' * 64)
        result = cu.apply('ask')
        self.assertEqual(result['reason'], 'lcu-archive-mismatch')
        self.assertEqual(len(self.curls()), 1)
        self.assertEqual(self.sleeps, [])

    def test_a_downloaded_archive_with_the_wrong_hash_is_refused(self):
        (self.image / ARCHIVE).unlink()
        self.write_pinned(sha256='0' * 64)
        result = cu.apply('ask')
        self.assertEqual((result['state'], result['reason']), ('failed', 'lcu-archive-mismatch'))
        self.assertFalse(any(argv[0].endswith('install.sh') for argv, *_ in self.commands))

    def test_archive_members_outside_the_release_folder_are_refused(self):
        bad = Path(self.tmp.name) / 'bad.tar.gz'
        with tarfile.open(bad, 'w:gz') as archive:
            info = tarfile.TarInfo('../escape')
            info.size = 0
            archive.addfile(info, io.BytesIO(b''))
        with self.assertRaises(cu.Failure) as caught:
            cu.extract(bad, self.state, 'lcu-0.8.0-linux-arm64')
        self.assertEqual(caught.exception.reason, 'lcu-archive-invalid')

    def test_a_concurrent_run_waits_then_finds_it_done(self):
        cu.apply('ask')
        self.commands.clear()
        # Another process finished the work while this one waited for the lock.
        self.assertEqual(cu.apply('ask')['state'], 'ready')
        self.assertEqual(self.commands, [])

    def test_the_digest_reads_the_documented_status_fields(self):
        report = cu.digest(self.lcu_status)
        self.assertEqual(report['compatibility'], 'tested')
        self.assertEqual(report['appVersion'], '26.928.31416')
        self.assertNotIn('agents', report)
        empty = cu.digest(None)
        self.assertEqual(empty['compatibility'], 'unknown')
        hostile = cu.digest({'compatibility': {'status': 'x', 'warning': 'a\x1b[31m' + 'b' * 999},
                             'app': {'version': '$(id)'}})
        self.assertEqual(hostile['compatibility'], 'unknown')
        self.assertIsNone(hostile['appVersion'])
        self.assertLessEqual(len(hostile['warning']), 300)


class Pending(Guest):
    def test_pending_agents_are_recorded_and_are_not_a_partial_outcome(self):
        self.pending = ['hermes', 'omp', 'pi']
        result = cu.apply('ask')
        self.assertEqual(result['apply'], {'approval': 'ask', 'outcome': 'applied', 'reason': None})
        receipt = self.receipt()
        self.assertEqual((receipt['agents'], receipt['pending']), (['claude-code', 'codex'], ['hermes', 'omp', 'pi']))
        self.assertEqual(receipt['approvalOutcome'], 'applied')
        self.assertEqual(self.spawned, [True], 'a watcher starts while agents are pending')
        self.assertIn('silo-computer-use reconcile', self.hook.read_text())

    def test_nothing_pending_starts_no_watcher_but_leaves_the_login_hook(self):
        cu.apply('ask')
        self.assertEqual((self.receipt()['pending'], self.spawned), ([], []))
        self.assertTrue(self.hook.is_file())

    def test_a_boot_registers_an_agent_installed_while_the_vm_was_off(self):
        self.pending = ['pi', 'hermes']
        cu.apply('ask')
        self.commands.clear()
        self.reconciled = ['pi']
        result = cu.apply('ask', boot=True)
        self.assertTrue(any(argv[1:3] == ['setup', '--reconcile'] for argv, *_ in self.commands))
        self.assertFalse(any('--approval' in argv for argv, *_ in self.commands))
        self.assertEqual(result['agents'], ['claude-code', 'codex', 'pi'])
        self.assertEqual(self.receipt()['pending'], ['hermes'])

    def test_the_idempotent_no_op_path_still_reconciles_while_something_is_pending(self):
        self.pending = ['pi']
        cu.apply('ask')
        self.commands.clear()
        self.reconciled = ['pi']
        self.assertEqual(cu.apply('ask')['apply']['outcome'], 'applied')
        self.assertTrue(any(argv[1:3] == ['setup', '--reconcile'] for argv, *_ in self.commands))
        self.assertEqual((self.receipt()['pending'], self.receipt()['agents']), ([], ['claude-code', 'codex', 'pi']))

    def test_the_reconcile_command_updates_the_receipt_and_skips_when_busy_or_not_ready(self):
        self.assertEqual(cu.reconcile(), {'reconcile': 'skipped'})
        self.pending = ['omp']
        cu.apply('ask')
        self.reconciled = ['omp']
        self.assertEqual(cu.reconcile(), {'reconcile': 'done', 'agents': ['claude-code', 'codex', 'omp'], 'pending': []})
        self.assertEqual(self.receipt()['pending'], [])
        with open(self.state / 'lock', 'a') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(cu.reconcile(), {'reconcile': 'busy'})

    def test_the_lcu_commands_run_with_the_agents_install_directories_on_the_path(self):
        with mock.patch.object(cu.subprocess, 'run') as run:
            run.return_value = subprocess.CompletedProcess([], 0, stdout='', stderr='')
            REAL_RUN(['lcu', 'status'], user=True)
        argv = run.call_args.args[0]
        self.assertIn(f'PATH={cu.USER_PATH}', argv)
        for directory in ('/home/silo/.local/bin', '/home/silo/.bun/bin', '/usr/local/bin'):
            self.assertIn(directory, cu.USER_PATH.split(':'))

    def test_an_lcu_without_allow_missing_falls_back_to_auto_and_never_reconciles(self):
        self.new_lcu = False
        result = cu.apply('ask')
        setup = next(argv for argv, *_ in self.commands if '--approval' in argv)
        self.assertEqual(setup[1:4], ['setup', '--agent', 'auto'])
        self.assertNotIn('--allow-missing', setup)
        self.assertEqual(result['apply']['outcome'], 'applied')
        self.assertEqual(self.receipt()['pending'], [])
        cu.apply('ask', boot=True)
        self.assertFalse(any(argv[1:3] == ['setup', '--reconcile'] for argv, *_ in self.commands))

    def test_pending_names_are_read_from_status(self):
        self.assertEqual(cu.pending_agents({'pending': ['pi', {'id': 'omp'}, 'Bad Name', 5]}), ['omp', 'pi'])
        self.assertIsNone(cu.pending_agents({'lcu_version': '0.8.7'}))
        self.assertIsNone(cu.pending_agents(None))

    def test_the_watcher_reconciles_once_when_a_pending_binary_appears_and_ends_when_none_is_pending(self):
        bin_dir = Path(self.tmp.name) / 'bin'
        bin_dir.mkdir()
        self.pending = ['pi']
        cu.apply('ask')
        runs = []
        with mock.patch.object(cu.subprocess, 'run', lambda argv, **k: runs.append(argv)):
            cu.watch(interval=0, directories=[bin_dir], rounds=2)
            self.assertEqual(runs, [], 'nothing installed yet')
            binary = bin_dir / 'pi'
            binary.write_text('#!/bin/sh\n')
            binary.chmod(0o755)
            cu.watch(interval=0, directories=[bin_dir], rounds=3)
            self.assertEqual(runs, [[cu.HELPER, 'reconcile']])
            receipt = self.receipt()
            receipt['pending'] = []
            (self.state / 'receipt.json').write_text(json.dumps(receipt))
            cu.watch(interval=0, directories=[bin_dir])
        self.assertEqual(runs, [[cu.HELPER, 'reconcile']])

    def test_main_runs_reconcile_and_prints_it(self):
        cu.apply('ask')
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('builtins.print') as shown:
            self.assertEqual(cu.main(['reconcile']), 0)
        self.assertEqual(json.loads(shown.call_args.args[0])['reconcile'], 'done')


class CommandLine(Guest):
    def test_main_applies_with_the_requested_mode_and_prints_the_outcome(self):
        with mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('builtins.print') as shown:
            self.assertEqual(cu.main(['apply', '--approval', 'auto', '--boot']), 0)
        printed = json.loads(shown.call_args.args[0])
        self.assertEqual(printed['apply'], {'approval': 'auto', 'outcome': 'applied', 'reason': None})
        with mock.patch.object(cu.os, 'geteuid', return_value=0), self.assertRaises(SystemExit):
            cu.main(['apply'])
        with mock.patch.object(cu.os, 'geteuid', return_value=0), self.assertRaises(SystemExit):
            cu.main(['apply', '--approval', 'maybe'])
        # The removed ordering flags are refused.
        with mock.patch.object(cu.os, 'geteuid', return_value=0), self.assertRaises(SystemExit):
            cu.main(['apply', '--approval', 'ask', '--revision', '3'])

    def test_a_run_that_cannot_start_is_still_a_report(self):
        with mock.patch.object(cu, 'apply', side_effect=cu.Failure('busy')), \
                mock.patch.object(cu.os, 'geteuid', return_value=0), mock.patch('builtins.print') as shown:
            self.assertEqual(cu.main(['apply', '--approval', 'ask']), 0)
        printed = json.loads(shown.call_args.args[0])
        self.assertEqual(printed['apply'], {'approval': 'ask', 'outcome': 'failed', 'reason': 'busy'})

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
        self.pinned = {'app': {'dir': 'fixture-app'},
                       'lcu': {'version': 'fixture-version', 'sha256': 'fixture-digest'}}

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
