#!/usr/bin/env python3
"""Focused regression tests for the pinned Selkies 2.0.0 display scaling patch."""
import asyncio
import hashlib
import importlib.util
import logging
import os
from pathlib import Path
import tempfile
import types
import unittest


ROOT = Path(__file__).resolve().parents[1]
PATCH_PATH = ROOT / 'src-tauri/guest/patch-selkies-display-scaling.py'
UPSTREAM = Path('/Users/polarzero/code/projects/selkies/src/selkies/display_utils.py')
SPEC = importlib.util.spec_from_file_location('selkies_display_patch', PATCH_PATH)
PATCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PATCH)

# The minimal pinned fragments the patch rewrites, taken from upstream 2.0.0.
SYNTHETIC = b'\n'.join(old for old, _ in PATCH.REPLACEMENTS)


class SelkiesDisplayPatchTests(unittest.TestCase):
    def test_patch_file_is_idempotent_and_rejects_changed_hash_without_write(self):
        original = SYNTHETIC
        patched = PATCH.transform_source(original)
        digest = lambda value: hashlib.sha256(value).hexdigest()
        PATCH.SOURCE_SHA256['test'] = digest(original)
        PATCH.PATCHED_SHA256['test'] = digest(patched)
        try:
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'display_utils.py'
                path.write_bytes(original)
                path.chmod(0o640)
                self.assertEqual(PATCH.patch_file(path, 'test'), 'patched')
                self.assertEqual(path.read_bytes(), patched)
                self.assertEqual(path.stat().st_mode & 0o777, 0o640)
                self.assertEqual(PATCH.patch_file(path, 'test'), 'already patched')
                changed = path.with_name('changed.py')
                changed.write_bytes(original + b'# changed\n')
                before = changed.read_bytes()
                with self.assertRaisesRegex(ValueError, 'source hash'):
                    PATCH.patch_file(changed, 'test')
                self.assertEqual(changed.read_bytes(), before)
                self.assertEqual(set(os.listdir(directory)), {'display_utils.py', 'changed.py'})
        finally:
            PATCH.SOURCE_SHA256.pop('test', None)
            PATCH.PATCHED_SHA256.pop('test', None)

    def test_transform_requires_every_exact_fragment(self):
        with self.assertRaisesRegex(ValueError, 'expected source fragments'):
            PATCH.transform_source(b'changed source')
        with self.assertRaisesRegex(ValueError, 'expected source fragments'):
            PATCH.transform_source(SYNTHETIC.replace(PATCH.REGEX_OLD, b''))

    def test_unsupported_architecture_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'architecture'):
            PATCH.patch_file(PATCH_PATH, 'riscv')

    def test_transform_result_replaces_the_fragments(self):
        patched = PATCH.transform_source(SYNTHETIC)
        for old, new in PATCH.REPLACEMENTS:
            if old not in new:
                self.assertEqual(patched.count(old), 0)
            self.assertEqual(patched.count(new), 1)


@unittest.skipUnless(UPSTREAM.is_file(), 'pinned Selkies source checkout is not available')
class PinnedUpstreamTests(unittest.TestCase):
    def setUp(self):
        self.source = UPSTREAM.read_bytes()

    def test_pinned_source_patches_to_the_recorded_hash(self):
        self.assertEqual(hashlib.sha256(self.source).hexdigest(), PATCH.SOURCE_SHA256['amd64'])
        patched = PATCH.transform_source(self.source)
        self.assertEqual(hashlib.sha256(patched).hexdigest(), PATCH.PATCHED_SHA256['amd64'])
        compile(patched, 'display_utils.py', 'exec')

    def patched_module(self):
        """The patched _run_xfconf and desktop_dpi, extracted without importing Selkies."""
        text = PATCH.transform_source(self.source).decode()
        namespace = {
            'asyncio': asyncio, 'logging': logging, 'os': os, 're': __import__('re'),
            'List': list, 'Optional': __import__('typing').Optional,
            'which': lambda name: '/usr/bin/' + name,
            'subprocess': types.SimpleNamespace(PIPE=-1),
            '_running_desktop': lambda *names: True,
            '_x11_lock': __import__('contextlib').nullcontext(),
        }
        def block(start, end):
            first = text.index(start)
            return text[first:text.index(end, first)]
        code = (block('async def _run_xfconf', 'async def _run_mate_gsettings') +
                block('_XFT_DPI = ', 'def applied_dpi') +
                block('def desktop_dpi', 'async def restore_dpi'))
        exec(code, namespace)
        return namespace

    def run_xfconf(self, dpi, store):
        namespace = self.patched_module()
        commands = []

        async def session_env(_logger):
            return {'DBUS_SESSION_BUS_ADDRESS': 'unix:fake'}

        class Process:
            def __init__(self, returncode, stdout=b''):
                self.returncode, self.stdout = returncode, stdout

        async def create(*cmd, **_kwargs):
            commands.append(list(cmd))
            channel, prop = cmd[cmd.index('-c') + 1], cmd[cmd.index('-p') + 1]
            if '-s' in cmd:
                store[(channel, prop)] = cmd[cmd.index('-s') + 1]
                return Process(0)
            if (channel, prop) in store:
                return Process(0, store[(channel, prop)].encode())
            return Process(1)

        async def communicate(process):
            return process.stdout, b''

        namespace['_get_xfce_session_env'] = session_env
        namespace['_communicate_or_kill'] = communicate
        namespace['subprocess'] = types.SimpleNamespace(
            PIPE=-1, create_subprocess_exec=create)
        result = asyncio.run(namespace['_run_xfconf'](dpi, logging.getLogger('test')))
        return result, commands

    def settings(self, commands):
        return [(c[c.index('-p') + 1], c[c.index('-s') + 1]) for c in commands if '-s' in c]

    def test_whole_multiple_becomes_a_window_scale_with_96_font_dpi(self):
        store = {('xfwm4', '/general/theme'): 'Default'}
        result, commands = self.run_xfconf(192, store)
        self.assertTrue(result)
        self.assertEqual(self.settings(commands), [
            ('/Xft/DPI', '96'), ('/Gdk/WindowScalingFactor', '2'),
            ('/Gtk/CursorThemeSize', '32'), ('/general/theme', 'Default-xhdpi')])

    def test_other_densities_ride_on_the_font_dpi(self):
        for dpi, cursor in ((96, '32'), (144, '48'), (200, '67')):
            store = {('xfwm4', '/general/theme'): 'Default'}
            result, commands = self.run_xfconf(dpi, store)
            self.assertTrue(result)
            self.assertEqual(store[('xsettings', '/Gdk/WindowScalingFactor')], '1')
            self.assertEqual(store[('xsettings', '/Xft/DPI')], str(dpi))
            self.assertEqual(store[('xsettings', '/Gtk/CursorThemeSize')], cursor)
            self.assertEqual(store[('xfwm4', '/general/theme')], 'Default')

    def test_decreasing_scale_sets_the_scale_before_the_dpi(self):
        store = {('xsettings', '/Gdk/WindowScalingFactor'): '2',
                 ('xfwm4', '/general/theme'): 'Default-xhdpi'}
        _result, commands = self.run_xfconf(96, store)
        order = [prop for prop, _ in self.settings(commands)]
        self.assertLess(order.index('/Gdk/WindowScalingFactor'), order.index('/Xft/DPI'))
        self.assertEqual(store[('xfwm4', '/general/theme')], 'Default')

    def test_custom_window_theme_is_left_alone(self):
        store = {('xfwm4', '/general/theme'): 'Greybird'}
        result, commands = self.run_xfconf(192, store)
        self.assertTrue(result)
        self.assertNotIn('/general/theme', [prop for prop, _ in self.settings(commands)])

    def test_desktop_dpi_multiplies_the_font_dpi_by_the_window_scale(self):
        namespace = self.patched_module()
        with tempfile.TemporaryDirectory() as home:
            channel = Path(home) / 'xfce4/xfconf/xfce-perchannel-xml/xsettings.xml'
            channel.parent.mkdir(parents=True)
            for body, expected in (
                    ('<property name="DPI" type="int" value="96"/>'
                     '<property name="WindowScalingFactor" type="int" value="2"/>', 192),
                    ('<property name="DPI" type="int" value="144"/>', 144)):
                channel.write_text(f'<channel>{body}</channel>')
                namespace['os'] = types.SimpleNamespace(
                    environ={'XDG_CONFIG_HOME': home}, path=os.path)
                namespace['_module_display'] = lambda: (_ for _ in ()).throw(OSError())
                namespace['_drop_module_display'] = lambda: None
                namespace['x11_error'] = types.SimpleNamespace(XError=type('XError', (Exception,), {}))
                namespace['x11_Xatom'] = types.SimpleNamespace(STRING=0)
                self.assertEqual(namespace['desktop_dpi'](), expected)


if __name__ == '__main__':
    unittest.main()
