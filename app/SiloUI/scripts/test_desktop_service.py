"""Guest lifecycle behavior tests without starting a desktop or mutating host state."""
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import tempfile
from types import SimpleNamespace
import unittest
from contextlib import contextmanager
from unittest.mock import Mock, patch

SOURCE = Path(__file__).resolve().parents[1] / 'src-tauri/guest/desktop-service.py'
spec = importlib.util.spec_from_file_location('desktop_service', SOURCE)
service = importlib.util.module_from_spec(spec)
spec.loader.exec_module(service)


class DesktopLifecycle(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.display_pair_index = 0
        for name in ('STATE', 'RUN'):
            directory = self.root / name
            directory.mkdir()
            mock = patch.object(service, name, directory)
            mock.start()
            self.addCleanup(mock.stop)
        service.write(service.STATE / 'installed.json', {'version': '1'})
        marker_patch = patch.object(service, 'WORKING_ACCOUNT', self.root / 'working-account.json', create=True)
        marker_patch.start()
        self.addCleanup(marker_patch.stop)
        identity_patch = patch.multiple(service, USER='silo', HOME=Path('/home/silo'))
        identity_patch.start()
        self.addCleanup(identity_patch.stop)
        self.unified_policy()

    def command(self, *arguments):
        with patch.object(service.sys, 'argv', ['silo-desktop', *arguments]), \
             patch.object(service.os, 'geteuid', return_value=0), \
             patch.object(service, 'validate_policy_file'), \
             patch.object(service.pwd, 'getpwnam', return_value=SimpleNamespace(pw_uid=1001, pw_gid=1001, pw_dir='/home/silo')), \
             patch('sys.stdout', new_callable=io.StringIO) as output:
            service.main()
            return json.loads(output.getvalue())

    def test_autostart_file_sync_failure_preserves_previous_preference(self):
        start = patch.object(service, 'start')
        start.start()
        self.addCleanup(start.stop)
        self.command('autostart', 'false')
        before = (service.STATE / 'config.json').read_bytes()
        entries = set(service.STATE.iterdir())
        with patch.object(service.os, 'fsync', side_effect=OSError('disk full')):
            with self.assertRaisesRegex(OSError, 'disk full'):
                self.command('autostart', 'true')
        self.assertEqual((service.STATE / 'config.json').read_bytes(), before)
        self.assertEqual(set(service.STATE.iterdir()), entries)
        self.command('autostart', 'true')
        self.assertEqual(service.read('config.json'), {'autoStart': True})

    def test_autostart_syncs_complete_file_before_publishing_and_directory_after(self):
        start = patch.object(service, 'start')
        start.start()
        self.addCleanup(start.stop)
        self.command('autostart', 'false')
        path = service.STATE / 'config.json'
        synced = []
        fsync = os.fsync

        def sync(fd):
            info = os.fstat(fd)
            if stat.S_ISREG(info.st_mode):
                self.assertEqual(service.read('config.json'), {'autoStart': False})
                self.assertEqual(os.pread(fd, info.st_size, 0), b'{"autoStart": true}\n')
                self.assertEqual(stat.S_IMODE(info.st_mode), 0o600)
                synced.append('file')
            else:
                self.assertTrue(stat.S_ISDIR(info.st_mode))
                self.assertEqual(info.st_ino, service.STATE.stat().st_ino)
                self.assertEqual(service.read('config.json'), {'autoStart': True})
                synced.append('directory')
            fsync(fd)

        with patch.object(service.os, 'fsync', side_effect=sync):
            self.command('autostart', 'true')
        self.assertEqual(synced, ['file', 'directory'])
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)

    def test_autostart_directory_sync_failure_does_not_report_success(self):
        start = patch.object(service, 'start')
        start.start()
        self.addCleanup(start.stop)
        self.command('autostart', 'false')
        fsync = os.fsync

        def sync(fd):
            if stat.S_ISDIR(os.fstat(fd).st_mode):
                raise OSError('directory sync failed')
            fsync(fd)

        with patch.object(service.os, 'fsync', side_effect=sync):
            with self.assertRaisesRegex(OSError, 'directory sync failed'):
                self.command('autostart', 'true')
        self.assertEqual(service.read('config.json'), {'autoStart': True})

    def test_absent_account_policy_requires_migration_even_when_installed(self):
        service.WORKING_ACCOUNT.unlink()
        for action in ('status', 'prepare-install', 'start', 'boot'):
            with self.subTest(action=action), self.assertRaisesRegex(RuntimeError, 'migrate this computer or create a new computer'):
                self.command(action)
        self.assertFalse((service.STATE / 'configuration-managed.json').exists())

    def unified_policy(self, **changes):
        policy = dict(schemaVersion=1, user='silo', home='/home/silo')
        policy.update(changes)
        service.WORKING_ACCOUNT.write_text(json.dumps(policy))
        service.WORKING_ACCOUNT.chmod(0o600)

    def test_unified_policy_uses_existing_normal_account(self):
        self.unified_policy()
        account = SimpleNamespace(pw_uid=1001, pw_gid=1001, pw_dir='/home/silo')
        with patch.object(service, 'validate_policy_file'), patch.object(service.pwd, 'getpwnam', return_value=account):
            self.assertEqual(service.desktop_account(), ('silo', Path('/home/silo')))
            self.assertEqual(self.command('status')['user'], 'silo')

    def test_invalid_policy_is_rejected(self):
        for changes in ({'schemaVersion': 2}, {'user': 'root'}, {'home': '/root'}):
            self.unified_policy(**changes)
            with patch.object(service, 'validate_policy_file'), self.assertRaises(RuntimeError):
                service.desktop_account()

    def test_policy_account_must_exist_with_expected_home_uid_and_gid(self):
        self.unified_policy()
        for account in (SimpleNamespace(pw_uid=0, pw_gid=1001, pw_dir='/home/silo'),
                        SimpleNamespace(pw_uid=1001, pw_gid=1000, pw_dir='/home/silo'),
                        SimpleNamespace(pw_uid=1001, pw_gid=1001, pw_dir='/elsewhere')):
            with patch.object(service, 'validate_policy_file'), patch.object(service.pwd, 'getpwnam', return_value=account), self.assertRaises(RuntimeError):
                service.desktop_account()
        with patch.object(service, 'validate_policy_file'), patch.object(service.pwd, 'getpwnam', side_effect=KeyError), self.assertRaises(RuntimeError):
            service.desktop_account()

    def test_policy_permissions_accept_only_root_owned_regular_files(self):
        for owner, mode, allowed in ((0, 0o100644, True), (1000, 0o100600, False),
                                     (0, 0o100620, False), (0, 0o040700, False)):
            with self.subTest(owner=owner, mode=mode):
                path = SimpleNamespace(lstat=lambda: SimpleNamespace(st_uid=owner, st_mode=mode))
                if allowed:
                    service.validate_policy_file(path)
                else:
                    with self.assertRaises(RuntimeError):
                        service.validate_policy_file(path)

    def test_policy_symlink_and_writable_file_are_rejected(self):
        self.unified_policy()
        service.WORKING_ACCOUNT.chmod(0o666)
        with self.assertRaises(RuntimeError):
            service.desktop_account()
        service.WORKING_ACCOUNT.unlink()
        service.WORKING_ACCOUNT.symlink_to(self.root / 'missing')
        with self.assertRaises(RuntimeError):
            service.desktop_account()

    def test_install_preflight_preserves_existing_desktop_configuration(self):
        home = self.root / 'home'
        (home / '.vnc').mkdir(parents=True)
        path = home / '.vnc/xstartup'
        path.write_text('existing session')
        with self.assertRaises(RuntimeError):
            service.prepare_configuration(home)
        self.assertEqual(path.read_text(), 'existing session')
        self.assertFalse((service.STATE / 'configuration-managed.json').exists())

    def test_install_preflight_rejects_existing_password_and_symlink_directory(self):
        home = self.root / 'home'
        home.mkdir()
        password = home / '.kasmpasswd'
        password.write_text('existing password')
        with self.assertRaises(RuntimeError):
            service.prepare_configuration(home)
        password.unlink()
        target = self.root / 'elsewhere'
        target.mkdir()
        (home / '.vnc').symlink_to(target, target_is_directory=True)
        with self.assertRaises(RuntimeError):
            service.prepare_configuration(home)
        self.assertEqual(list(target.iterdir()), [])

    def test_unified_display_stop_targets_its_account_and_home(self):
        with patch.multiple(service, USER='silo', HOME=Path('/home/silo')), patch.object(service.subprocess, 'run') as run:
            service.stop_display()
        arguments = run.call_args.args[0]
        self.assertEqual(arguments[:6], ['runuser', '-u', 'silo', '--', 'env', 'HOME=/home/silo'])
        self.assertEqual(arguments[-3:], ['vncserver', '-kill', ':1'])

    def stale_display_pair(self, real_socket=True):
        self.display_pair_index += 1
        tmp = self.root / f'tmp-{self.display_pair_index}'
        tmp.mkdir()
        tmp.chmod(0o1777)
        socket_dir = tmp / '.X11-unix'
        socket_dir.mkdir()
        lock = tmp / '.X1-lock'
        lock.write_text('99999999\n')
        display_socket = socket_dir / 'X1'
        display_socket.write_text('socket placeholder' if real_socket else 'not a socket')
        return tmp, socket_dir, lock, display_socket

    @contextmanager
    def root_owned_tmp_simulation(self, pair, directory_owner=1001):
        tmp, socket_dir, lock, display_socket = pair
        account = SimpleNamespace(pw_uid=1001, pw_gid=1001, pw_dir='/home/silo')
        state = {'socket_dir_root_owned': False}
        chown_calls = []
        real_lstat = Path.lstat
        real_fstat = os.fstat
        real_fchmod = os.fchmod

        def info(result, uid=None, mode=None):
            return SimpleNamespace(st_mode=result.st_mode if mode is None else mode,
                                   st_uid=result.st_uid if uid is None else uid,
                                   st_gid=result.st_gid, st_dev=result.st_dev,
                                   st_ino=result.st_ino)

        def fake_lstat(path):
            path = Path(path)
            result = real_lstat(path)
            if path == tmp:
                return info(result, uid=0)
            if path == socket_dir:
                owner = 0 if state['socket_dir_root_owned'] else directory_owner
                return info(result, uid=owner)
            if path == lock:
                return info(result, uid=account.pw_uid)
            if (path == display_socket and stat.S_ISREG(result.st_mode) and
                    path.read_text() == 'socket placeholder'):
                return info(result, uid=account.pw_uid, mode=stat.S_IFSOCK | 0o600)
            return result

        def fake_fstat(fd):
            result = real_fstat(fd)
            for path in (socket_dir, lock):
                path_info = real_lstat(path)
                if (result.st_dev, result.st_ino) == (path_info.st_dev, path_info.st_ino):
                    if path == socket_dir:
                        owner = 0 if state['socket_dir_root_owned'] else directory_owner
                    else:
                        owner = account.pw_uid
                    mode = stat.S_IFSOCK | 0o600 if path == display_socket else None
                    return info(result, uid=owner, mode=mode)
            return result

        def fake_fchown(fd, uid, gid):
            result = real_fstat(fd)
            path_info = real_lstat(socket_dir)
            self.assertEqual((result.st_dev, result.st_ino), (path_info.st_dev, path_info.st_ino))
            chown_calls.append((uid, gid))
            state['socket_dir_root_owned'] = uid == 0

        def fake_fchmod(fd, mode):
            result = real_fstat(fd)
            path_info = real_lstat(socket_dir)
            self.assertEqual((result.st_dev, result.st_ino), (path_info.st_dev, path_info.st_ino))
            real_fchmod(fd, mode)

        with patch.multiple(service, TMP=tmp, DISPLAY_LOCK=lock,
                            DISPLAY_SOCKET_DIR=socket_dir, USER='silo'), \
             patch.object(service.os, 'geteuid', return_value=0), \
             patch.object(service.pwd, 'getpwnam', return_value=account), \
             patch.object(service.Path, 'lstat', fake_lstat), \
             patch.object(service.os, 'fstat', fake_fstat), \
             patch.object(service.os, 'fchown', side_effect=fake_fchown), \
             patch.object(service.os, 'fchmod', side_effect=fake_fchmod), \
             patch.object(service, 'process_exists', return_value=False), \
             patch.object(service, 'port_listening', return_value=False), \
             patch.object(service, 'listening', return_value=False):
            yield state, chown_calls

    def test_user_owned_empty_x11_directory_is_normalized_to_root_sticky(self):
        pair = self.stale_display_pair()
        tmp, socket_dir, lock, display_socket = pair
        lock.unlink()
        display_socket.unlink()
        with self.root_owned_tmp_simulation(pair) as (state, chown_calls):
            self.assertTrue(service.prepare_display_socket_directory())
            self.assertTrue(state['socket_dir_root_owned'])
            self.assertEqual(chown_calls, [(0, 0)])
            self.assertEqual(stat.S_IMODE(socket_dir.stat().st_mode), 0o1777)

    def test_user_owned_x11_directory_with_only_dead_display_pair_is_normalized(self):
        pair = self.stale_display_pair()
        _, socket_dir, _, _ = pair
        with self.root_owned_tmp_simulation(pair) as (state, chown_calls):
            self.assertTrue(service.prepare_display_socket_directory())
            self.assertTrue(state['socket_dir_root_owned'])
            self.assertEqual(chown_calls, [(0, 0)])
            self.assertTrue((socket_dir / 'X1').exists())
            # Existing stale cleanup retains its stricter root-owned directory check.
            self.assertTrue(service.remove_stale_display_artifacts())
            self.assertFalse((socket_dir / 'X1').exists())

    def test_display_directory_normalization_refuses_live_or_unexpected_state(self):
        for live_listener, extra_file in ((True, False), (False, True)):
            with self.subTest(live_listener=live_listener, extra_file=extra_file):
                pair = self.stale_display_pair()
                tmp, socket_dir, lock, display_socket = pair
                lock.unlink()
                display_socket.unlink()
                if extra_file:
                    (socket_dir / 'unrelated').write_text('keep')
                with self.root_owned_tmp_simulation(pair) as (state, chown_calls), \
                     patch.object(service, 'port_listening', return_value=live_listener):
                    self.assertFalse(service.prepare_display_socket_directory())
                    self.assertFalse(state['socket_dir_root_owned'])
                    self.assertEqual(chown_calls, [])
                    if extra_file:
                        self.assertEqual((socket_dir / 'unrelated').read_text(), 'keep')

    def test_display_directory_normalization_refuses_untrusted_owner_and_symlink(self):
        pair = self.stale_display_pair()
        tmp, socket_dir, lock, display_socket = pair
        lock.unlink()
        display_socket.unlink()
        with self.root_owned_tmp_simulation(pair, directory_owner=2000) as (state, chown_calls):
            self.assertFalse(service.prepare_display_socket_directory())
            self.assertFalse(state['socket_dir_root_owned'])
            self.assertEqual(chown_calls, [])

    def test_start_keeps_an_existing_healthy_display_untouched(self):
        with patch.object(service, 'supervisor', return_value=True), \
             patch.object(service, 'prepare_display_socket_directory',
                           side_effect=AssertionError('must not normalize a running display')):
            self.assertIsNone(service.start())

        pair = self.stale_display_pair()
        tmp, socket_dir, lock, display_socket = pair
        lock.unlink()
        display_socket.unlink()
        socket_dir.rmdir()
        target = self.root / 'x11-target'
        target.mkdir()
        socket_dir.symlink_to(target, target_is_directory=True)
        with self.root_owned_tmp_simulation(pair) as (state, chown_calls):
            self.assertFalse(service.prepare_display_socket_directory())
            self.assertFalse(state['socket_dir_root_owned'])
            self.assertEqual(chown_calls, [])

    def remove_stale_pair(self, pair, pid_live=False, listening_port=None, account_uid=None):
        tmp, socket_dir, lock, display_socket = pair
        account = SimpleNamespace(pw_uid=os.getuid() if account_uid is None else account_uid)
        original_lstat = Path.lstat

        def lstat(path):
            result = original_lstat(path)
            if Path(path) == display_socket and display_socket.read_text() == 'socket placeholder':
                return SimpleNamespace(st_mode=stat.S_IFSOCK | 0o600, st_uid=account.pw_uid,
                                       st_dev=result.st_dev, st_ino=result.st_ino)
            return result

        with patch.multiple(service, TMP=tmp, DISPLAY_LOCK=lock, DISPLAY_SOCKET_DIR=socket_dir), \
             patch.object(Path, 'lstat', lstat), \
             patch.object(service.os, 'geteuid', return_value=os.getuid()), \
             patch.object(service.pwd, 'getpwnam', return_value=account), \
             patch.object(service, 'process_exists', return_value=pid_live), \
             patch.object(service, 'port_listening', side_effect=lambda port: port == listening_port):
            return service.remove_stale_display_artifacts(), lock, display_socket

    def test_dead_kasm_display_pair_is_removed_for_single_retry(self):
        result, lock, display_socket = self.remove_stale_pair(self.stale_display_pair())
        self.assertTrue(result)
        self.assertFalse(lock.exists())
        self.assertFalse(display_socket.exists())

    def test_live_display_pid_or_listener_preserves_stale_looking_files(self):
        for live_pid, listening in ((True, False), (False, True)):
            with self.subTest(live_pid=live_pid, listening=listening):
                pair = self.stale_display_pair()
                result, lock, display_socket = self.remove_stale_pair(
                    pair, pid_live=live_pid, listening_port=5901 if listening else None)
                self.assertFalse(result)
                self.assertTrue(lock.exists())
                self.assertTrue(display_socket.exists())

    def test_unexpected_owner_or_socket_type_is_preserved(self):
        pair = self.stale_display_pair()
        tmp, socket_dir, lock, display_socket = pair
        result, lock, display_socket = self.remove_stale_pair(pair, account_uid=os.getuid() + 1)
        self.assertFalse(result)
        self.assertTrue(lock.exists())
        self.assertTrue(display_socket.exists())

        pair = self.stale_display_pair(real_socket=False)
        result, lock, display_socket = self.remove_stale_pair(pair)
        self.assertFalse(result)
        self.assertTrue(lock.exists())
        self.assertTrue(display_socket.exists())

    def test_install_preflight_allows_retry_but_not_a_different_home(self):
        home = self.root / 'home'
        home.mkdir()
        service.prepare_configuration(home)
        (home / '.kasmpasswd').write_text('managed partial installation')
        service.prepare_configuration(home)
        with self.assertRaises(RuntimeError):
            service.prepare_configuration(self.root / 'other')

    def test_disabling_autostart_preserves_current_session(self):
        with patch.object(service, 'start') as start, patch.object(service, 'stop') as stop:
            result = self.command('autostart', 'false')
        self.assertFalse(result['autoStart'])
        start.assert_not_called()
        stop.assert_not_called()
        self.assertEqual(service.read('config.json'), {'autoStart': False})

    def test_enabling_autostart_starts_immediately(self):
        with patch.object(service, 'start') as start:
            self.command('autostart', 'true')
        start.assert_called_once_with()

    def test_boot_respects_persisted_manual_preference(self):
        service.write(service.STATE / 'config.json', {'autoStart': False})
        with patch.object(service, 'start') as start:
            self.command('boot')
        start.assert_not_called()

    def test_boot_defaults_to_automatic(self):
        with patch.object(service, 'start') as start:
            self.command('boot')
        start.assert_called_once_with()

    def test_supervise_selkies_action_dispatches_to_managed_worker(self):
        with patch.object(service.sys, 'argv', ['silo-desktop', 'supervise-selkies']), \
             patch.object(service.os, 'geteuid', return_value=0), \
             patch.object(service, 'desktop_account', return_value=('silo', Path('/home/silo'))), \
             patch.object(service, 'supervise_selkies') as worker:
            service.main()
        worker.assert_called_once_with()

    def test_reused_pid_is_not_a_live_supervisor(self):
        service.write(service.RUN / 'supervisor.json', {'pid': 123, 'start': 'old'})
        with patch.object(service, 'identity', return_value='new'):
            self.assertIsNone(service.supervisor())
            with patch.object(service.os, 'kill') as kill:
                service.stop()
                kill.assert_not_called()

    def test_explicit_stop_clears_failure_without_disabling_next_boot(self):
        (service.RUN / 'failed').write_text('failed')
        result = self.command('stop')
        self.assertEqual(result['state'], 'stopped')
        self.assertTrue(result['autoStart'])

    def test_running_requires_service_and_endpoint(self):
        with patch.object(service, 'supervisor', return_value=123), \
             patch.object(service, 'listening', return_value=False):
            self.assertEqual(service.status()['state'], 'starting')
        with patch.object(service, 'supervisor', return_value=123), \
             patch.object(service, 'listening', return_value=True):
            self.assertEqual(service.status()['state'], 'running')

    def test_kasm_status_does_not_report_stopped_when_display_artifacts_remain(self):
        lock = self.root / 'X1-lock'
        lock.write_text('321')
        socket_dir = self.root / 'X11-unix'
        socket_dir.mkdir()
        (socket_dir / 'X1').write_text('live display marker')
        with patch.multiple(service, DISPLAY_LOCK=lock, DISPLAY_SOCKET_DIR=socket_dir), \
             patch.object(service, 'supervisor', return_value=None), \
             patch.object(service, 'listening', return_value=False), \
             patch.object(service, 'port_listening', return_value=False):
            result = service.status()
        self.assertEqual(result['backend'], 'kasm')
        self.assertEqual(result['state'], 'stopped')
        self.assertEqual(result['sessionState'], 'running')

    def test_existing_invalid_streamer_receipt_never_falls_back_to_kasm(self):
        service.write(service.STATE / 'streamer.json', {'backend': 'selkies', 'state': 'installing'})
        with patch.object(service, 'supervisor', return_value=None), \
             patch.object(service, 'listening', return_value=False):
            result = service.status()
        self.assertIsNone(result['backend'])
        self.assertTrue(result['updateRequired'])
        self.assertFalse(result['updateAvailable'])
        self.assertEqual(result['state'], 'failed')
        self.assertEqual(result['sessionState'], 'stopped')
        self.assertEqual(result['streamState'], 'failed')

        processes = [{'name': name, 'pid': pid} for name, pid in
                     (('xvfb', 10), ('pulse', 11), ('xfce', 12))]
        service.write(service.RUN / 'selkies.json', {
            'backend': 'selkies', 'bootId': service.current_boot_id(),
            'sessionState': 'running', 'sessionProcesses': processes,
            'streamState': 'failed', 'streamProcess': None,
        })
        with patch.object(service, 'managed_process_matches', return_value=True), \
             patch.object(service, 'supervisor', return_value=None):
            result = service.status()
        self.assertTrue(result['updateRequired'])
        self.assertEqual(result['sessionState'], 'running')
        self.assertEqual(result['streamState'], 'failed')
        with patch.object(service, 'stop_managed_process') as stop_process:
            self.command('stop')
        self.assertEqual([call.args[0]['name'] for call in stop_process.call_args_list],
                         ['xfce', 'pulse', 'xvfb'])

    def test_older_supported_receipts_preserve_live_state_and_offer_an_update(self):
        executable = self.root / 'selkies'
        executable.write_text('#!/bin/sh\nexit 0\n')
        executable.chmod(0o755)
        receipt = service.STATE / 'streamer.json'
        service.write(receipt, {
            'schemaVersion': 1, 'state': 'ready', 'backend': 'selkies',
            'version': '2.0.0', 'recipeVersion': 1, 'architecture': 'arm64',
            'packageSha256': 'a' * 64, 'resolution': {'width': 1440, 'height': 900},
        })
        real_lstat = Path.lstat

        def root_owned_receipt(path):
            result = real_lstat(path)
            if Path(path) == receipt:
                return SimpleNamespace(st_mode=result.st_mode, st_uid=0)
            return result

        session_processes = [{'pid': 10}, {'pid': 11}, {'pid': 12}]
        current = {'bootId': service.current_boot_id(), 'sessionState': 'running',
                   'sessionProcesses': session_processes, 'streamState': 'running',
                   'streamProcess': {'pid': 13}}
        common_patches = (
            patch.object(service, 'SELKIES_EXECUTABLE', executable),
            patch.object(service.os, 'uname', return_value=SimpleNamespace(machine='aarch64')),
            patch.object(service.Path, 'lstat', autospec=True, side_effect=root_owned_receipt),
            patch.object(service, 'supervisor', return_value=42),
            patch.object(service, 'selkies_state', return_value=current),
            patch.object(service, 'managed_process_matches', return_value=True),
            patch.object(service, 'selkies_http_ready', return_value=True),
            patch.object(service.shutil, 'which', side_effect=lambda command: None if command == 'vncserver' else '/usr/bin/' + command),
        )
        with common_patches[0], common_patches[1], common_patches[2], common_patches[3], \
             common_patches[4], common_patches[5], common_patches[6], common_patches[7]:
            result = service.status()
        self.assertEqual(result['backend'], 'selkies')
        self.assertEqual(result['streamerVersion'], '2.0.0')
        self.assertEqual(result['version'], '1')
        self.assertEqual(result['state'], 'running')
        self.assertEqual(result['sessionState'], 'running')
        self.assertEqual(result['streamState'], 'running')
        self.assertFalse(result['updateRequired'])
        self.assertTrue(result['updateAvailable'])

        receipt_value = json.loads(receipt.read_text())
        receipt_value['recipeVersion'] = 2
        service.write(receipt, receipt_value)
        with common_patches[0], common_patches[1], common_patches[2], common_patches[3], \
             common_patches[4], common_patches[5], common_patches[6], common_patches[7]:
            result = service.status()
        self.assertEqual(result['state'], 'running')
        self.assertFalse(result['updateRequired'])
        self.assertTrue(result['updateAvailable'])

        receipt_value['recipeVersion'] = 3
        service.write(receipt, receipt_value)
        current_patches = (
            patch.object(service, 'SELKIES_EXECUTABLE', executable),
            patch.object(service.os, 'uname', return_value=SimpleNamespace(machine='aarch64')),
            patch.object(service.Path, 'lstat', autospec=True, side_effect=root_owned_receipt),
            patch.object(service, 'supervisor', return_value=42),
            patch.object(service, 'selkies_state', return_value=current),
            patch.object(service, 'managed_process_matches', return_value=True),
            patch.object(service, 'selkies_http_ready', return_value=True),
            patch.object(service.shutil, 'which', side_effect=lambda command: None if command == 'vncserver' else '/usr/bin/' + command),
        )
        with current_patches[0], current_patches[1], current_patches[2], current_patches[3], \
             current_patches[4], current_patches[5], current_patches[6], current_patches[7]:
            result = service.status()
        self.assertEqual(result['state'], 'running')
        self.assertFalse(result['updateRequired'])
        self.assertFalse(result['updateAvailable'])

    def test_current_recipe_matches_the_bundled_streamer_lock(self):
        lock = json.loads((SOURCE.parent / 'desktop-streamer-lock.json').read_text())
        self.assertEqual(lock['recipeVersion'], service.STREAMER_CURRENT_RECIPE)
        self.assertIn(service.STREAMER_CURRENT_RECIPE, service.STREAMER_RECIPE_VERSIONS)

    def test_session_repair_restarts_a_failed_session_with_a_live_supervisor(self):
        cu_spec = importlib.util.spec_from_file_location(
            'session_repair', SOURCE.with_name('silo-computer-use.py'))
        cu = importlib.util.module_from_spec(cu_spec)
        cu_spec.loader.exec_module(cu)
        executable = self.root / 'selkies'
        executable.write_text('#!/bin/sh\n')
        executable.chmod(0o755)
        current = {'bootId': service.current_boot_id(), 'sessionState': 'running',
                   'sessionProcesses': [{'pid': 10}, {'pid': 11}, {'pid': 12}]}
        running = {'supervisor': 42, 'session': False}
        events = []

        def session_state():
            return service.selkies_session_state(current, bool(running['supervisor']))

        def stop():
            events.append('stop')
            running['supervisor'] = None

        def launch(*_args, **_kwargs):
            events.append('launch')
            running.update(supervisor=43, session=True)

        with patch.object(service, 'supervisor', side_effect=lambda: running['supervisor']), \
             patch.object(service, 'selkies_state', return_value=current), \
             patch.object(service, 'managed_process_matches',
                          side_effect=lambda record: running['session'] or record['pid'] != 11), \
             patch.object(service, 'stop', side_effect=stop), \
             patch.object(service, 'SELKIES_EXECUTABLE', executable), \
             patch.object(service.pwd, 'getpwnam', return_value='account'), \
             patch.object(service, 'prepare_selkies_runtime', side_effect=lambda _: events.append('prepare')), \
             patch.object(service.subprocess, 'Popen', side_effect=launch), \
             patch.object(service, 'status', side_effect=lambda: {
                 'sessionState': session_state(), 'streamState': 'running'}), \
             patch.object(cu, 'desktop_session', side_effect=session_state), \
             patch.object(cu, 'desktop_autostart', return_value=True), \
             patch.object(cu, 'start_desktop', side_effect=service.start_selkies), \
             patch.object(cu, 'log'), patch.object(cu.time, 'sleep'):
            cu.wait_for_session(90, repair=True)
        self.assertEqual(events, ['stop', 'prepare', 'launch'])
        self.assertTrue(running['session'])

    def test_start_preserves_a_healthy_or_starting_selkies_session(self):
        for saved in ('starting', 'running'):
            with self.subTest(saved=saved):
                current = {'bootId': service.current_boot_id(), 'sessionState': saved,
                           'sessionProcesses': [{'pid': 10}, {'pid': 11}, {'pid': 12}]}
                with patch.object(service, 'supervisor', return_value=42), \
                     patch.object(service, 'selkies_state', return_value=current), \
                     patch.object(service, 'managed_process_matches', return_value=True), \
                     patch.object(service, 'stop') as stop, \
                     patch.object(service.subprocess, 'Popen') as launch:
                    service.start_selkies()
                stop.assert_not_called()
                launch.assert_not_called()

    def test_stream_supervision_reaps_exited_session_children(self):
        children = [SimpleNamespace(pid=pid, poll=Mock(return_value=1 if pid == 11 else None))
                    for pid in (10, 11, 12)]

        def start(_commands, _environment, _account, state, session_children, _stopping, after_launch=None):
            session_children.extend(children)
            state['sessionProcesses'] = [{'pid': child.pid} for child in children]

        def stream(_state, _account, _environment, stopping, _restart):
            self.assertFalse(stopping())
            for child in children:
                child.poll.assert_called()

        with patch.object(service, 'streamer_backend', return_value='selkies'), \
             patch.object(service, 'LOG', self.root / 'log'), \
             patch.object(service, 'identity', return_value='supervisor-start'), \
             patch.object(service.signal, 'signal'), \
             patch.object(service.pwd, 'getpwnam', return_value='account'), \
             patch.object(service, 'start_session_processes', side_effect=start), \
             patch.object(service, 'supervise_selkies_stream', side_effect=stream), \
             patch.object(service, 'stop_managed_child') as stop_child:
            service.supervise_selkies()
        self.assertFalse((service.RUN / 'failed').exists())
        self.assertEqual([call.args[0].pid for call in stop_child.call_args_list], [12, 11, 10])

    def test_selkies_stream_failure_preserves_the_session_records(self):
        session = [
            {'name': 'xvfb', 'pid': 10},
            {'name': 'pulse', 'pid': 11},
            {'name': 'xfce', 'pid': 12},
        ]
        state = {'sessionState': 'running', 'sessionProcesses': session,
                 'streamState': 'starting', 'streamProcess': None, 'streamAttempts': 0}
        children = [SimpleNamespace(poll=lambda: 1) for _ in range(3)]
        with patch.object(service, 'launch_selkies_streamer',
                           side_effect=[(child, {'name': 'selkies', 'pid': n})
                                        for child, n in zip(children, (13, 14, 15))]), \
             patch.object(service, 'sleep_until_service_event'):
            # The event loop waits in failed state until explicit stop.
            service.supervise_selkies_stream(
                state, {}, {}, lambda: state['streamState'] == 'failed', lambda: False)
        self.assertEqual(state['sessionState'], 'running')
        self.assertEqual(state['sessionProcesses'], session)
        self.assertEqual(state['streamState'], 'failed')
        self.assertEqual(state['streamAttempts'], 3)

    def test_selkies_retries_back_off_between_failed_attempts(self):
        state = {'sessionState': 'running', 'sessionProcesses': [],
                 'streamState': 'starting', 'streamProcess': None, 'streamAttempts': 0}
        children = [SimpleNamespace(poll=lambda: 1) for _ in range(3)]
        slept = []
        with patch.object(service, 'launch_selkies_streamer',
                          side_effect=[(child, {'name': 'selkies', 'pid': n})
                                       for child, n in zip(children, (13, 14, 15))]), \
             patch.object(service, 'sleep_until_service_event', side_effect=slept.append):
            service.supervise_selkies_stream(
                state, {}, {}, lambda: state['streamState'] == 'failed', lambda: False)
        self.assertEqual(state['streamState'], 'failed')
        # Two retries wait 2 s and then 4 s; the first attempt starts immediately.
        self.assertEqual(sum(slept), 6)

    def test_restart_request_during_retry_backoff_starts_a_fresh_attempt(self):
        state = {'sessionState': 'running', 'sessionProcesses': [],
                 'streamState': 'starting', 'streamProcess': None, 'streamAttempts': 0}
        requests = iter((False, False, True))
        launches = []

        def launch(*_args):
            launches.append(state['streamAttempts'])
            return SimpleNamespace(poll=lambda: 1), {'name': 'selkies', 'pid': 13}

        with patch.object(service, 'launch_selkies_streamer', side_effect=launch), \
             patch.object(service, 'sleep_until_service_event'):
            service.supervise_selkies_stream(
                state, {}, {}, lambda: len(launches) == 2,
                lambda: next(requests, False))
        # The request arrived while waiting to retry, so attempt 1 starts again.
        self.assertEqual(launches, [1, 1])

    def test_selkies_stable_stream_resets_retry_budget(self):
        state = {'sessionState': 'running', 'sessionProcesses': [],
                 'streamState': 'starting', 'streamProcess': None, 'streamAttempts': 0}
        clock = {'now': 0.0}
        launches = []
        states = []

        class LongRunningChild:
            def __init__(self):
                self.exited = False

            def poll(self):
                return 1 if self.exited else None

            def wait(self, timeout):
                # Each stream runs well past the stability window, then crashes.
                clock['now'] += service.SELKIES_STABLE_SECONDS + 1
                self.exited = True

        def launch(*_args):
            launches.append(state['streamAttempts'])
            return LongRunningChild(), {'name': 'selkies', 'pid': 13}

        with patch.object(service, 'launch_selkies_streamer', side_effect=launch), \
             patch.object(service, 'selkies_http_ready', return_value=True), \
             patch.object(service, 'sleep_until_service_event'), \
             patch.object(service, 'stop_managed_child'), \
             patch.object(service, 'write_selkies_state',
                          side_effect=lambda value: states.append(value['streamState'])), \
             patch.object(service.time, 'monotonic', side_effect=lambda: clock['now']):
            service.supervise_selkies_stream(
                state, {}, {}, lambda: len(launches) > service.SELKIES_MAX_ATTEMPTS + 1,
                lambda: False)
        self.assertNotIn('failed', states)
        self.assertEqual(launches, [1] * (service.SELKIES_MAX_ATTEMPTS + 2))

    def test_restart_streamer_waits_for_failed_state_to_acknowledge_the_request(self):
        old = {'streamState': 'failed', 'streamAttempts': 3, 'streamProcess': None}
        starting = {'streamState': 'starting', 'streamAttempts': 1, 'streamProcess': None}
        replacement = {'name': 'selkies', 'pid': 42}
        running = {'streamState': 'running', 'streamAttempts': 1,
                   'streamProcess': replacement}
        states = iter((old, old, starting, running))

        with patch.object(service, 'streamer_backend', return_value='selkies'), \
             patch.object(service, 'selkies_state', side_effect=lambda: next(states)), \
             patch.object(service, 'selkies_session_state', return_value='running'), \
             patch.object(service, 'selkies_stream_state', side_effect=lambda state, _running: state['streamState']), \
             patch.object(service, 'supervisor', return_value=7), \
             patch.object(service.os, 'kill') as send_signal, \
             patch.object(service.time, 'sleep'):
            service.restart_selkies_streamer()

        send_signal.assert_called_once_with(7, service.signal.SIGUSR1)

    def test_restart_streamer_reports_failure_after_request_is_acknowledged(self):
        old = {'streamState': 'failed', 'streamAttempts': 3, 'streamProcess': None}
        starting = {'streamState': 'starting', 'streamAttempts': 1, 'streamProcess': None}
        failed = {'streamState': 'failed', 'streamAttempts': 3, 'streamProcess': None}
        states = iter((old, starting, failed))

        with patch.object(service, 'streamer_backend', return_value='selkies'), \
             patch.object(service, 'selkies_state', side_effect=lambda: next(states)), \
             patch.object(service, 'selkies_session_state', return_value='running'), \
             patch.object(service, 'selkies_stream_state', side_effect=lambda state, _running: state['streamState']), \
             patch.object(service, 'supervisor', return_value=7), \
             patch.object(service.os, 'kill'), \
             patch.object(service.time, 'sleep'):
            with self.assertRaisesRegex(RuntimeError, 'Selkies restart failed; desktop session remains running'):
                service.restart_selkies_streamer()

    def test_selkies_supervisor_stop_reaps_stream_child_before_clearing_ownership(self):
        stop_requested = {'value': False}
        child = SimpleNamespace(poll=lambda: None)
        record = {'name': 'selkies', 'pid': 13}
        state = {'sessionState': 'running', 'sessionProcesses': [],
                 'streamState': 'starting', 'streamProcess': None, 'streamAttempts': 0}

        def launch(*_args):
            stop_requested['value'] = True
            return child, record

        with patch.object(service, 'launch_selkies_streamer', side_effect=launch), \
             patch.object(service, 'stop_managed_child') as stop_child:
            service.supervise_selkies_stream(
                state, {}, {}, lambda: stop_requested['value'], lambda: False)
        stop_child.assert_called_once_with(child)
        self.assertIsNone(state['streamProcess'])
        self.assertEqual(state['streamState'], 'stopped')

    def test_stop_after_ready_stream_stops_child_before_dropping_ownership(self):
        stop_requested = {'value': False}
        state = {'sessionState': 'running', 'sessionProcesses': [],
                 'streamState': 'starting', 'streamProcess': None, 'streamAttempts': 0}
        record = {'name': 'selkies', 'pid': 13}

        class ReadyChild:
            def poll(self):
                return None

            def wait(self, timeout):
                # The running monitor observes a stop while waiting on the live streamer.
                stop_requested['value'] = True

        child = ReadyChild()
        stop_observations = []

        def stop_child(candidate):
            stop_observations.append((candidate is child, state['streamProcess'] is record))

        with patch.object(service, 'launch_selkies_streamer', return_value=(child, record)), \
             patch.object(service, 'selkies_http_ready', return_value=True), \
             patch.object(service, 'stop_managed_child', side_effect=stop_child):
            service.supervise_selkies_stream(
                state, {}, {}, lambda: stop_requested['value'], lambda: False)

        self.assertEqual(stop_observations, [(True, True)])
        self.assertIsNone(state['streamProcess'])
        self.assertEqual(state['streamState'], 'stopped')

    def test_explicit_selkies_stop_terminates_stream_then_session(self):
        records = [
            {'name': 'xvfb', 'pid': 10},
            {'name': 'pulse', 'pid': 11},
            {'name': 'xfce', 'pid': 12},
        ]
        # Records from this boot; on Linux a missing bootId means a stale state.
        state = {'bootId': service.current_boot_id(), 'sessionState': 'running', 'sessionProcesses': records,
                 'streamState': 'running', 'streamProcess': {'name': 'selkies', 'pid': 13}}
        with patch.object(service, 'stop_managed_process') as stop_process:
            service.stop_selkies_processes(state)
        self.assertEqual([call.args[0]['name'] for call in stop_process.call_args_list],
                         ['selkies', 'xfce', 'pulse', 'xvfb'])
        self.assertEqual(state['sessionState'], 'stopped')
        self.assertEqual(state['streamState'], 'stopped')

    def test_status_does_not_expose_connection_credentials(self):
        service.write(service.STATE / 'connection.json', {'username': 'silo', 'password': 'secret', 'port': 6901})
        result = self.command('status')
        self.assertNotIn('secret', json.dumps(result))
        self.assertNotIn('password', result)

    def test_selkies_supervision_bounds_logs_with_open_append_descriptors(self):
        log = self.root / 'service.log'
        log.write_bytes(b'old' * (1024 * 1024))
        handlers = {}
        waits = []
        session = [SimpleNamespace(pid=pid, poll=lambda: None) for pid in (10, 11, 12)]

        def start(_commands, _environment, _account, state, children, _stopping, after_launch=None):
            children.extend(session)
            state['sessionProcesses'] = [{'pid': child.pid} for child in session]

        with log.open('ab', buffering=0) as appender:
            def wait(timeout):
                # Every monitor cycle must retain the recent output, even though
                # the children keep their original append descriptors open.
                self.assertLessEqual(log.stat().st_size, 1024 * 1024)
                if waits:
                    self.assertEqual(log.read_bytes(), waits[-1][-256 * 1024:])
                payload = bytes([len(waits) + 65]) * (1280 * 1024)
                appender.write(payload)
                waits.append(payload)
                if len(waits) == 3:
                    handlers[service.signal.SIGTERM](service.signal.SIGTERM, None)

            stream = SimpleNamespace(pid=13, poll=lambda: None, wait=wait)
            with patch.object(service, 'streamer_backend', return_value='selkies'), \
                 patch.object(service, 'HOME', self.root / 'home'), \
                 patch.object(service, 'LOG', log), \
                 patch.object(service, 'identity', return_value='supervisor-start'), \
                 patch.object(service.signal, 'signal', side_effect=lambda sig, fn: handlers.update({sig: fn})), \
                 patch.object(service.pwd, 'getpwnam', return_value='account'), \
                 patch.object(service, 'start_session_processes', side_effect=start), \
                 patch.object(service, 'launch_selkies_streamer', return_value=(stream, {'pid': 13})), \
                 patch.object(service, 'selkies_http_ready', return_value=True), \
                 patch.object(service, 'stop_managed_child'):
                service.supervise_selkies()
        self.assertFalse((service.RUN / 'failed').exists())
        self.assertEqual(len(waits), 3)
        self.assertEqual(log.read_bytes(), waits[-1][-256 * 1024:])

    def test_log_retention_does_not_follow_home_or_vnc_directory_links(self):
        for linked in ('home', 'vnc'):
            with self.subTest(linked=linked):
                case = self.root / linked
                case.mkdir()
                external = case / 'external'
                external.mkdir()
                sentinel = external / 'private.log'
                original = b'outside-desktop' * (128 * 1024)
                sentinel.write_bytes(original)
                home = case / 'home'
                if linked == 'vnc':
                    home.mkdir()
                    (home / '.vnc').symlink_to(external, target_is_directory=True)
                else:
                    actual_home = case / 'actual-home'
                    actual_home.mkdir()
                    (actual_home / '.vnc').mkdir()
                    sentinel.rename(actual_home / '.vnc/private.log')
                    sentinel = actual_home / '.vnc/private.log'
                    home.symlink_to(actual_home, target_is_directory=True)
                with patch.object(service, 'HOME', home), patch.object(service, 'LOG', case / 'service.log'):
                    service.trim_logs()
                self.assertEqual(sentinel.read_bytes(), original)

    def test_log_retention_keeps_the_opened_directory_when_its_path_is_replaced(self):
        home = self.root / 'home'
        directory = home / '.vnc'
        directory.mkdir(parents=True)
        managed = directory / 'desktop.log'
        managed.write_bytes(b'a' * (1280 * 1024))
        external = self.root / 'external'
        external.mkdir()
        sentinel = external / 'desktop.log'
        original = b'private' * (256 * 1024)
        sentinel.write_bytes(original)
        retained = home / 'retained-vnc'
        listdir = os.listdir

        def replace_directory(fd):
            names = listdir(fd)
            directory.rename(retained)
            directory.symlink_to(external, target_is_directory=True)
            return names

        with patch.object(service, 'HOME', home), \
             patch.object(service, 'LOG', self.root / 'service.log'), \
             patch.object(service.os, 'listdir', side_effect=replace_directory):
            service.trim_logs()
        self.assertTrue(directory.is_symlink())
        self.assertEqual((retained / 'desktop.log').read_bytes(), b'a' * (256 * 1024))
        self.assertEqual(sentinel.read_bytes(), original)

    def test_log_bounds_preserve_tail_and_do_not_follow_symlinks(self):
        home = self.root / 'home'
        logs = home / '.vnc'
        logs.mkdir(parents=True)
        large = logs / 'desktop.log'
        large.write_bytes(b'a' * (1024 * 1024) + b'b' * (256 * 1024))
        target = self.root / 'private'
        original = b'private' * 200000
        target.write_bytes(original)
        (logs / 'symlink.log').symlink_to(target)
        with patch.object(service, 'HOME', home), patch.object(service, 'LOG', self.root / 'service.log'):
            service.trim_logs()
        self.assertEqual(large.read_bytes(), b'b' * (256 * 1024))
        self.assertEqual(target.read_bytes(), original)

    def test_lcu_status_requires_the_official_runtime_before_receipt_state(self):
        service.write(service.STATE / 'lcu.json', {
            'schemaVersion': 1, 'status': 'ready', 'version': '0.4.0',
            'appVersion': '1.0.0', 'runtimeVersion': '0.4.0',
            'agents': ['pi', 'codex'], 'readiness': 'ready',
        })
        with patch.object(service, 'LCU_APP', self.root / 'missing-chatgpt'), \
             patch.object(service, 'LCU_RECEIPT', service.STATE / 'lcu.json'), \
             patch.object(service.subprocess, 'run', side_effect=AssertionError('must stay passive')):
            result = service.lcu_status()
        self.assertEqual(result['lcuState'], 'needs-runtime')
        self.assertEqual(result['lcuReason'], 'chatgpt-app-required')
        self.assertEqual(result['lcuReadiness'], 'unverified')
        self.assertIsNone(result['lcuVersion'])

    def test_lcu_ready_projection_filters_agents_and_private_receipt_fields(self):
        app = self.root / 'usr/lib/chatgpt'
        app.mkdir(parents=True)
        prefix = self.root / 'opt/lcu'
        release = prefix / 'releases/0.4.0'
        (release / 'bin').mkdir(parents=True)
        runtime = release / 'bin/lcu'
        runtime.write_text('#!/bin/sh\n')
        runtime.chmod(0o755)
        (release / 'app').mkdir()
        prefix.mkdir(parents=True, exist_ok=True)
        (prefix / 'current').symlink_to(release)
        receipt_path = self.root / 'lcu.json'
        receipt_path.write_text(json.dumps({
            'schemaVersion': 1, 'status': 'ready', 'version': '0.4.0',
            'appVersion': '26.924.22138',
            'runtimeVersion': '0.0.24/20260924074400-f52ea85e2a98',
            'agents': ['pi', 'codex', 'unknown'], 'readiness': 'ready',
            'appPath': '/private/path/must-not-escape',
        }))
        receipt = SimpleNamespace(
            lstat=lambda: SimpleNamespace(st_uid=0, st_mode=receipt_path.lstat().st_mode),
            read_text=receipt_path.read_text,
        )
        with patch.object(service, 'LCU_APP', app), \
             patch.object(service, 'LCU_PREFIX', prefix), \
             patch.object(service, 'LCU_RECEIPT', receipt), \
             patch.object(service.subprocess, 'run', side_effect=AssertionError('must stay passive')):
            result = service.lcu_status()
        self.assertEqual(result['lcuState'], 'ready')
        self.assertEqual(result['lcuAgents'], ['codex', 'pi'])
        self.assertEqual(result['lcuAppVersion'], '26.924.22138')
        self.assertEqual(result['lcuRuntimeVersion'], '0.0.24/20260924074400-f52ea85e2a98')
        self.assertNotIn('appPath', result)
    def test_status_ignores_a_previous_luda_installation(self):
        service.write(service.STATE / 'luda.json', {'state': 'ready', 'version': '0.3.4'})
        result = self.command('status')
        self.assertTrue(result['installed'])
        self.assertNotIn('ludaState', result)
        self.assertNotIn('ludaVersion', result)

    def test_boot_does_not_install_or_repair_tools(self):
        with patch.object(service, 'start'), patch.object(service.subprocess, 'run') as run:
            self.command('boot')
        run.assert_not_called()

    def test_unknown_autostart_value_does_not_modify_preferences(self):
        with self.assertRaises(RuntimeError):
            self.command('autostart', 'yes')
        self.assertIsNone(service.read('config.json'))


class StaleSessionRuntime(unittest.TestCase):
    """/run is on the VM's disk: a restart or an imported disk carries the last session's files."""
    setUp = DesktopLifecycle.setUp
    command = DesktopLifecycle.command
    unified_policy = DesktopLifecycle.unified_policy

    def pulse_dir(self):
        pulse = service.RUN / 'user' / 'pulse'
        pulse.mkdir(parents=True)
        (pulse / 'pid').write_text('224\n')
        (pulse / 'native').write_text('')
        return pulse

    def test_a_stale_pulse_pid_file_is_removed_before_the_session_starts(self):
        # The new boot's PulseAudio often gets the pid the old file names, which is
        # itself: PulseAudio then exits with "Daemon already running".
        pulse = self.pulse_dir()
        account = SimpleNamespace(pw_uid=os.getuid(), pw_gid=os.getgid())
        with patch.object(service, 'prepare_display_socket_directory', return_value=True), \
             patch.object(service, 'DISPLAY_LOCK', self.root / 'no-lock'), \
             patch.object(service, 'DISPLAY_SOCKET_DIR', self.root / 'x11'), \
             patch.object(service.subprocess, 'run'), patch.object(service.os, 'chown'):
            service.prepare_selkies_runtime(account)
        self.assertFalse((pulse / 'pid').exists())
        self.assertFalse((pulse / 'native').exists())

    def test_a_live_pulse_of_this_boot_keeps_its_files(self):
        pulse = self.pulse_dir()
        record = {'name': 'pulse', 'pid': 224}
        with patch.object(service, 'current_boot_id', return_value='boot-1'), \
             patch.object(service, 'managed_process_matches', return_value=True):
            service.write_selkies_state({'bootId': 'boot-1', 'sessionProcesses': [record]})
            service.clear_stale_pulse_runtime(pulse)
        self.assertTrue((pulse / 'pid').exists())

    def test_pulse_of_a_previous_boot_is_stale_even_if_its_pid_is_alive(self):
        pulse = self.pulse_dir()
        record = {'name': 'pulse', 'pid': 224}
        service.write_selkies_state({'bootId': 'old-boot', 'sessionProcesses': [record]})
        with patch.object(service, 'current_boot_id', return_value='new-boot'), \
             patch.object(service, 'managed_process_matches', return_value=True):
            service.clear_stale_pulse_runtime(pulse)
        self.assertFalse((pulse / 'pid').exists())

    def test_a_new_boot_empties_the_previous_session_runtime_once(self):
        self.pulse_dir()
        (service.RUN / 'user' / 'at-spi2-ABC').mkdir()
        (service.RUN / 'user' / 'at-spi2-ABC' / 'socket').write_text('')
        (service.RUN / 'user' / 'ICEauthority').write_text('x')
        (service.RUN / 'boot-id').write_text('old-boot')
        boot = self.root / 'boot_id'
        boot.write_text('new-boot')
        with patch.object(service, 'BOOT_ID', boot):
            self.command('status')
            self.assertEqual(list((service.RUN / 'user').iterdir()), [])
            self.assertTrue((service.RUN / 'user').is_dir())
            # Later calls in the same boot leave the running session's files alone.
            (service.RUN / 'user' / 'bus').write_text('')
            self.command('status')
        self.assertTrue((service.RUN / 'user' / 'bus').exists())
        self.assertEqual((service.RUN / 'boot-id').read_text(), 'new-boot')

    def test_reset_never_follows_a_symlink_planted_in_the_runtime_directory(self):
        outside = self.root / 'outside'
        outside.mkdir()
        (outside / 'keep').write_text('x')
        runtime = service.RUN / 'user'
        runtime.mkdir()
        (runtime / 'link').symlink_to(outside)
        service.reset_session_runtime()
        self.assertEqual(list(runtime.iterdir()), [])
        self.assertTrue((outside / 'keep').exists())
        # A symlinked runtime directory itself is left alone.
        runtime.rmdir()
        runtime.symlink_to(outside)
        service.reset_session_runtime()
        self.assertTrue((outside / 'keep').exists())


class SessionStartRetry(unittest.TestCase):
    COMMANDS = [('xvfb', ['Xvfb']), ('pulse', ['pulseaudio']), ('xfce', ['startxfce4'])]

    def run_start(self, launch, stopping=lambda: False, commands=None):
        state = {'sessionProcesses': []}
        children = []
        slept = []
        with patch.object(service, 'launch_managed_process', side_effect=launch) as launcher, \
             patch.object(service, 'write_selkies_state'), \
             patch.object(service, 'log_line'), \
             patch.object(service.shutil, 'which', return_value='/usr/bin/x'), \
             patch.object(service, 'prepare_selkies_runtime') as prepare, \
             patch.object(service, 'stop_managed_child') as stop_child, \
             patch.object(service, 'sleep_until_service_event', side_effect=slept.append):
            try:
                service.start_session_processes(commands or self.COMMANDS, {}, 'account', state,
                                                children, stopping)
                error = None
            except Exception as caught:  # noqa: BLE001 - asserted by the caller
                error = caught
        return SimpleNamespace(state=state, children=children, slept=slept, launcher=launcher,
                               prepare=prepare, stop_child=stop_child, error=error)

    @staticmethod
    def launches(failures):
        """Launch results: an exception per failing call (in order), else a child."""
        calls = iter(failures)

        def launch(name, *_args):
            outcome = next(calls, None)
            if isinstance(outcome, Exception):
                raise outcome
            return SimpleNamespace(name=name), {'name': name, 'pid': len(name)}
        return launch

    def test_a_clean_start_launches_each_process_once_in_order(self):
        result = self.run_start(self.launches([]))
        self.assertIsNone(result.error)
        self.assertEqual([record['name'] for record in result.state['sessionProcesses']],
                         ['xvfb', 'pulse', 'xfce'])
        self.assertEqual(result.slept, [])
        result.prepare.assert_not_called()

    def test_a_pulse_that_exits_at_once_is_retried_from_a_clean_session(self):
        result = self.run_start(self.launches([None, RuntimeError('pulse exited or failed identity validation')]))
        self.assertIsNone(result.error)
        # The failed attempt stopped Xvfb, which the retry started again with the others.
        self.assertEqual([call.args[0].name for call in result.stop_child.call_args_list], ['xvfb'])
        self.assertEqual([call.args[0] for call in result.launcher.call_args_list],
                         ['xvfb', 'pulse', 'xvfb', 'pulse', 'xfce'])
        self.assertEqual([record['name'] for record in result.state['sessionProcesses']],
                         ['xvfb', 'pulse', 'xfce'])
        self.assertEqual(len(result.children), 3)
        result.prepare.assert_called_once_with('account')
        self.assertEqual(sum(result.slept), service.SESSION_RETRY_BASE_SECONDS)

    def test_the_session_fails_after_three_attempts_with_bounded_backoff(self):
        error = RuntimeError('xfce exited')
        result = self.run_start(self.launches([None, None, error, None, None, error, None, None, error]))
        self.assertIs(result.error, error)
        self.assertEqual(result.launcher.call_count, 9)
        self.assertEqual(result.state['sessionProcesses'], [])
        self.assertEqual(result.children, [])
        self.assertEqual(sum(result.slept), 1 + 2)
        self.assertEqual(result.prepare.call_count, 2)

    def test_a_missing_session_program_is_not_retried(self):
        with patch.object(service.shutil, 'which', return_value=None):
            state = {'sessionProcesses': []}
            with patch.object(service, 'launch_managed_process') as launcher, \
                 patch.object(service, 'sleep_until_service_event') as sleep:
                with self.assertRaisesRegex(RuntimeError, 'Required desktop command is missing: Xvfb'):
                    service.start_session_processes(self.COMMANDS, {}, 'account', state, [], lambda: False)
        launcher.assert_not_called()
        sleep.assert_not_called()

    def test_a_stop_request_ends_the_retry_wait(self):
        stops = iter([False, False, True, True, True, True])
        result = self.run_start(self.launches([None, RuntimeError('pulse exited')]),
                                stopping=lambda: next(stops, True))
        self.assertIsNone(result.error)
        self.assertEqual(result.launcher.call_count, 2)


class StreamerLaunch(unittest.TestCase):
    def launch_argv(self):
        connection = {'username': service.USER, 'port': 6901, 'password': 'a' * 64}
        with patch.object(service, 'read', return_value=connection), \
             patch.object(service, 'launch_managed_process', return_value=('child', {})) as launcher:
            service.launch_selkies_streamer('account', {})
        return launcher.call_args.args[1]

    def test_selkies_flags_enable_clipboard_audio_and_resize_and_lock_the_rest(self):
        argv = self.launch_argv()
        for flag in ('--enable-clipboard=true', '--enable-binary-clipboard=true',
                     '--clipboard-seamless=false', '--file-transfers=none',
                     '--audio-enabled=true', '--audio-bitrate=64000',
                     '--microphone-enabled=false|locked', '--ui-sidebar-show-audio-settings=false',
                     '--enable-resize=true', '--use-css-scaling=true|locked',
                     '--mode=websockets', '--enable-dual-mode=false|locked'):
            self.assertIn(flag, argv)
        self.assertEqual(len({arg.split('=')[0] for arg in argv}), len(argv))


class DesktopStartSize(unittest.TestCase):
    QUERY = ('Screen 0: minimum 1 x 1, current {w} x {h}, maximum 32767 x 32767\n'
             'screen connected primary {w}x{h}+0+0 0mm x 0mm\n   {w}x{h}      60.00*\n'
             '   1024x768      60.00\n')

    def run_resize(self, replies):
        calls = []

        def xrandr(arguments, _environment, _account, timeout=None):
            calls.append(arguments)
            code, out, err = replies(arguments, calls)
            return SimpleNamespace(returncode=code, stdout=out, stderr=err)
        clock = iter(i * 0.2 for i in range(1000))
        with patch.object(service, 'run_xrandr', side_effect=xrandr), \
             patch.object(service.time, 'sleep'), \
             patch.object(service.time, 'monotonic', side_effect=lambda: next(clock)):
            try:
                service.set_desktop_start_size({}, 'account')
                error = None
            except RuntimeError as caught:
                error = caught
        return calls, error

    def test_xvfb_starts_large_and_is_shrunk_to_the_start_size_with_a_new_mode(self):
        self.assertEqual(service.XVFB_SCREEN, '4096x4096x24')

        def replies(arguments, calls):
            if arguments == ['--query']:
                size = (1440, 900) if any('--output' in call for call in calls) else (4096, 4096)
                return 0, self.QUERY.format(w=size[0], h=size[1]), ''
            return 0, '', ''
        calls, error = self.run_resize(replies)
        self.assertIsNone(error)
        self.assertEqual(calls[1], ['--newmode', '1440x900', *service.START_MODELINE])
        self.assertEqual(calls[2], ['--addmode', 'screen', '1440x900'])
        self.assertEqual(calls[3], ['--output', 'screen', '--mode', '1440x900', '--fb', '1440x900'])

    def test_a_listed_mode_is_reused_and_a_correct_size_is_left_alone(self):
        listed = self.QUERY.format(w=4096, h=4096) + '   1440x900      59.90\n'
        calls, error = self.run_resize(lambda arguments, _calls: (0, listed, ''))
        self.assertIsNotNone(error)
        self.assertEqual([call[0] for call in calls if call[0] != '--query'], ['--output'])
        calls, error = self.run_resize(lambda arguments, _calls: (0, self.QUERY.format(w=1440, h=900), ''))
        self.assertIsNone(error)
        self.assertEqual(calls, [['--query']])

    def test_a_failed_resize_is_a_clear_error(self):
        def replies(arguments, calls):
            if arguments == ['--query']:
                return 0, self.QUERY.format(w=4096, h=4096), ''
            return (1, '', 'bad mode') if arguments[0] == '--addmode' else (0, '', '')
        _calls, error = self.run_resize(replies)
        self.assertRegex(str(error), 'could not add the 1440x900 mode: bad mode')
        _calls, error = self.run_resize(lambda arguments, calls: (1, '', 'no display'))
        self.assertRegex(str(error), 'did not report a RandR output')

    def test_a_display_that_is_not_ready_yet_is_polled_within_a_bound(self):
        attempts = []

        def replies(arguments, calls):
            if arguments == ['--query']:
                attempts.append(1)
                if len(attempts) < 4:
                    return 1, '', "can't open display"
                size = (1440, 900) if any('--output' in call for call in calls) else (4096, 4096)
                return 0, self.QUERY.format(w=size[0], h=size[1]), ''
            return 0, '', ''
        calls, error = self.run_resize(replies)
        self.assertIsNone(error)
        self.assertEqual(calls[:4], [['--query']] * 4)
        calls, error = self.run_resize(lambda arguments, _calls: (1, '', 'no display'))
        self.assertLessEqual(len(calls), service.XRANDR_READY_SECONDS * 5 + 2)

    def test_a_hung_xrandr_query_is_retried_until_the_bound(self):
        attempts = []

        def hung_then_ready(arguments, _environment, _account, timeout=None):
            attempts.append(arguments)
            if len(attempts) < 3:
                raise RuntimeError('xrandr could not run: timed out')
            return SimpleNamespace(returncode=0, stdout=self.QUERY.format(w=1440, h=900), stderr='')
        with patch.object(service, 'run_xrandr', side_effect=hung_then_ready), \
             patch.object(service.time, 'sleep'):
            service.set_desktop_start_size({}, 'account')
        self.assertEqual(len(attempts), 3)

    def test_slow_queries_cannot_stretch_the_readiness_window(self):
        now = [0.0]
        timeouts = []

        def hung(arguments, _environment, _account, timeout=None):
            timeouts.append(timeout)
            now[0] += timeout
            raise RuntimeError('xrandr could not run: timed out')
        with patch.object(service, 'run_xrandr', side_effect=hung), \
             patch.object(service.time, 'sleep', side_effect=lambda seconds: now.__setitem__(0, now[0] + seconds)), \
             patch.object(service.time, 'monotonic', side_effect=lambda: now[0]):
            with self.assertRaisesRegex(RuntimeError, 'did not report a RandR output'):
                service.set_desktop_start_size({}, 'account')
        self.assertLessEqual(now[0], service.XRANDR_READY_SECONDS + 0.001)
        self.assertTrue(all(0 < timeout <= service.XRANDR_TIMEOUT_SECONDS for timeout in timeouts))
        self.assertLess(timeouts[-1], service.XRANDR_TIMEOUT_SECONDS + 0.001)
        self.assertEqual(len(timeouts), 1)

    def test_the_last_query_gets_only_the_time_that_remains(self):
        now = [0.0]
        timeouts = []

        def slow(arguments, _environment, _account, timeout=None):
            timeouts.append(timeout)
            now[0] += 4.0
            return SimpleNamespace(returncode=1, stdout='', stderr='no display')
        with patch.object(service, 'run_xrandr', side_effect=slow), \
             patch.object(service.time, 'sleep', side_effect=lambda seconds: now.__setitem__(0, now[0] + seconds)), \
             patch.object(service.time, 'monotonic', side_effect=lambda: now[0]):
            with self.assertRaises(RuntimeError):
                service.set_desktop_start_size({}, 'account')
        self.assertLessEqual(max(timeouts), service.XRANDR_TIMEOUT_SECONDS)
        self.assertAlmostEqual(timeouts[-1], service.XRANDR_READY_SECONDS - sum(4.2 for _ in timeouts[:-1]), places=3)
        self.assertLessEqual(now[0], service.XRANDR_READY_SECONDS + 4.0)

    def test_the_resize_runs_right_after_xvfb_starts_and_not_after_other_processes(self):
        steps = []
        with patch.object(service, 'launch_managed_process',
                          side_effect=lambda name, *_: (SimpleNamespace(name=name), {'name': name})), \
             patch.object(service, 'write_selkies_state'), \
             patch.object(service.shutil, 'which', return_value='/usr/bin/x'):
            service.start_session_processes(
                SessionStartRetry.COMMANDS, {}, 'account', {'sessionProcesses': []}, [], lambda: False,
                after_launch=steps.append)
        self.assertEqual(steps, ['xvfb', 'pulse', 'xfce'])


if __name__ == '__main__':
    unittest.main()
