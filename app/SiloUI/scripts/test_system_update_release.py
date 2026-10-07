"""Privilege boundary and APT sequence tests; no host package state is changed."""
import importlib.machinery
import importlib.util
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

loader = importlib.machinery.SourceFileLoader('system_update', str(Path(__file__).with_name('debian') / 'silo-system-update'))
spec = importlib.util.spec_from_loader(loader.name, loader)
helper = importlib.util.module_from_spec(spec)
loader.exec_module(helper)


class SystemUpdateTests(unittest.TestCase):
    def test_rejects_arguments_that_could_choose_commands_or_other_packages(self):
        for value in ('--help', 'silo=1.0.0', '1.0.0;id', '1.0', '1.0.0-rc1', '01.0.0', ''):
            with self.assertRaises(ValueError):
                helper.version(value)
        self.assertEqual(helper.version('0.5.1'), '0.5.1')

    def test_refreshes_before_download_and_installs_only_the_requested_silo_version(self):
        calls = []
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp, 'silo.sources')
            source.write_text('Types: deb\nEnabled: yes\n')
            with patch.object(helper, 'SOURCE', source):
                helper.upgrade('0.5.1', run=lambda args: calls.append(args), stage=lambda name: calls.append(name), confirm=lambda: calls.append('confirmed'))
        # Silo stops computers between 'ready' and its go-ahead: only after refresh and download succeeded.
        self.assertEqual([call for call in calls if isinstance(call, str)], ['refreshing', 'downloading', 'ready', 'confirmed', 'installing'])
        calls = [call for call in calls if not isinstance(call, str)]
        self.assertIn('update', calls[0])
        self.assertIn('APT::Update::Error-Mode=any', calls[0])
        self.assertIn('--download-only', calls[1])
        self.assertIn('--no-download', calls[2])
        for call in calls[1:]:
            self.assertIn('silo=0.5.1', call)
            self.assertIn('--only-upgrade', call)
            self.assertIn('--no-remove', call)
            self.assertNotIn('--allow-unauthenticated', call)
            self.assertNotIn('--allow-downgrades', call)

    def test_refresh_failure_never_installs_from_stale_indexes(self):
        calls = []
        def fail(args):
            calls.append(args)
            raise RuntimeError('offline')
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp, 'silo.sources')
            source.write_text('Types: deb\n')
            with patch.object(helper, 'SOURCE', source), self.assertRaisesRegex(RuntimeError, 'offline'):
                helper.upgrade('0.5.1', run=fail, stage=lambda _: None)
        self.assertEqual(len(calls), 1)

    def test_nothing_is_installed_without_the_go_ahead(self):
        calls = []
        def cancelled():
            raise RuntimeError('Silo cancelled the update before installation. Nothing was installed.')
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp, 'silo.sources')
            source.write_text('Types: deb\n')
            with patch.object(helper, 'SOURCE', source), self.assertRaisesRegex(RuntimeError, 'Nothing was installed'):
                helper.upgrade('0.5.1', run=calls.append, stage=lambda _: None, confirm=cancelled)
        self.assertEqual(len(calls), 2)
        self.assertIn('--download-only', calls[1])

    def test_go_ahead_requires_an_explicit_install_line_before_the_timeout(self):
        for sent, accepted in ((b'install\n', True), (b'cancel\n', False), (b'', False)):
            read, write = os.pipe()
            os.write(write, sent)
            os.close(write)
            with os.fdopen(read) as stream:
                if accepted:
                    helper.confirm(stream, timeout=5)
                else:
                    with self.assertRaisesRegex(RuntimeError, 'Nothing was installed'):
                        helper.confirm(stream, timeout=5)
        read, write = os.pipe()
        with os.fdopen(read) as stream, os.fdopen(write, 'w'):
            with self.assertRaisesRegex(RuntimeError, 'Nothing was installed'):
                helper.confirm(stream, timeout=0.05)

    def test_missing_or_disabled_source_is_not_silently_reenabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp, 'silo.sources')
            with patch.object(helper, 'SOURCE', source):
                for content in (None, 'Types: deb\nEnabled: no\n'):
                    if content:
                        source.write_text(content)
                    with self.assertRaisesRegex(RuntimeError, 'Software & Updates'):
                        helper.upgrade('0.5.1', run=lambda _: self.fail('must not run apt'), stage=lambda _: None)

    def test_partial_confirmation_cannot_outlive_the_install_deadline(self):
        read, write = os.pipe()
        os.write(write, b'install')

        def finish_after_deadline():
            try:
                time.sleep(0.3)
                os.write(write, b'\n')
            except BrokenPipeError:
                pass
            finally:
                os.close(write)

        sender = threading.Thread(target=finish_after_deadline)
        sender.start()
        try:
            with os.fdopen(read) as stream:
                with self.assertRaisesRegex(RuntimeError, 'Nothing was installed'):
                    helper.confirm(stream, timeout=0.02)
        finally:
            sender.join(timeout=2)
            self.assertFalse(sender.is_alive())

    def test_update_must_be_requested_by_the_process_that_started_the_helper(self):
        # pkexec execs the helper, so the helper (300) is a direct child of silo-ui (100).
        parents = {300: 100, 301: 200, 200: 100, 400: 50}
        exes = {'/proc/200/exe': '/usr/bin/pkexec', '/proc/50/exe': '/usr/bin/python3'}
        times = {100: 'a', 101: 'c', 50: 'b'}

        def requested(pid, helper_pid, started=times.__getitem__):
            return helper.requested_by(pid, helper_pid, parents.__getitem__, lambda path: exes.get(path, '/usr/bin/silo-ui'), started)

        self.assertTrue(requested(100, 300))
        self.assertTrue(requested(100, 301))
        self.assertFalse(requested(101, 300))
        self.assertTrue(requested(50, 400))
        self.assertFalse(requested(100, 400))
        def vanished(pid):
            raise FileNotFoundError(pid)

        self.assertFalse(requested(100, 300, started=vanished))
        reused = iter(['a', 'z'])
        self.assertFalse(requested(100, 300, started=lambda pid: next(reused)))

    def test_running_process_exception_requires_exact_live_processes_and_root_owned_marker(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp, 'permit')
            with patch.object(helper, 'MARKER', marker):
                self.assertFalse(helper.allows_running(100))
                marker.write_text('{"app": [100, "123"], "helper": [200, "456"]}')
                with patch.object(helper, 'root_private_file', return_value=True), patch.object(helper, 'start_time', side_effect=lambda pid: {100: '123', 200: '456'}[pid]):
                    self.assertTrue(helper.allows_running(100))
                    self.assertFalse(helper.allows_running(101))
                with patch.object(helper, 'root_private_file', return_value=True), patch.object(helper, 'start_time', return_value='different'):
                    self.assertFalse(helper.allows_running(100))
                with patch.object(helper, 'root_private_file', return_value=False):
                    self.assertFalse(helper.allows_running(100))

if __name__ == '__main__':
    unittest.main()
