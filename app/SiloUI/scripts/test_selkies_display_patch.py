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
UPSTREAM_DIR = Path('/Users/polarzero/code/projects/selkies/src/selkies')
UPSTREAM = UPSTREAM_DIR / 'display_utils.py'
UPSTREAM_WEBSOCKETS = UPSTREAM_DIR / 'websockets_mode.py'
SPEC = importlib.util.spec_from_file_location('selkies_display_patch', PATCH_PATH)
PATCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PATCH)

# The minimal pinned fragments each module's patch rewrites, taken from upstream 2.0.0.
SYNTHETIC = b'\n'.join(old for old, _ in PATCH.REPLACEMENTS)
SYNTHETIC_WEBSOCKETS = b'\n'.join(old for old, _ in PATCH.WEBSOCKETS_REPLACEMENTS)


def digest(value):
    return hashlib.sha256(value).hexdigest()


class SelkiesDisplayPatchTests(unittest.TestCase):
    def synthetic_modules(self):
        return {
            'display_utils.py': {
                'source': digest(SYNTHETIC),
                'patched': digest(PATCH.transform_source(SYNTHETIC)),
                'replacements': PATCH.REPLACEMENTS,
            },
            'websockets_mode.py': {
                'source': digest(SYNTHETIC_WEBSOCKETS),
                'patched': digest(PATCH.transform_source(
                    SYNTHETIC_WEBSOCKETS, PATCH.WEBSOCKETS_REPLACEMENTS)),
                'replacements': PATCH.WEBSOCKETS_REPLACEMENTS,
            },
        }

    def test_patch_package_is_idempotent(self):
        modules = self.synthetic_modules()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'display_utils.py').write_bytes(SYNTHETIC)
            (root / 'websockets_mode.py').write_bytes(SYNTHETIC_WEBSOCKETS)
            (root / 'display_utils.py').chmod(0o640)
            self.assertEqual(PATCH.patch_package(root, 'amd64', modules), 'patched')
            for name, spec in modules.items():
                self.assertEqual(digest((root / name).read_bytes()), spec['patched'])
            self.assertEqual((root / 'display_utils.py').stat().st_mode & 0o777, 0o640)
            self.assertEqual(PATCH.patch_package(root, 'amd64', modules), 'already patched')
            self.assertEqual(set(os.listdir(root)), {'display_utils.py', 'websockets_mode.py'})

    def test_each_module_refuses_a_changed_hash_and_nothing_is_written(self):
        modules = self.synthetic_modules()
        for changed in modules:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / 'display_utils.py').write_bytes(SYNTHETIC)
                (root / 'websockets_mode.py').write_bytes(SYNTHETIC_WEBSOCKETS)
                (root / changed).write_bytes((root / changed).read_bytes() + b'# changed\n')
                before = {n: (root / n).read_bytes() for n in modules}
                with self.assertRaisesRegex(ValueError, f'{changed} source hash'):
                    PATCH.patch_package(root, 'amd64', modules)
                self.assertEqual({n: (root / n).read_bytes() for n in modules}, before)
                self.assertEqual(set(os.listdir(root)), set(modules))

    def test_transform_requires_every_exact_fragment(self):
        with self.assertRaisesRegex(ValueError, 'expected source fragments'):
            PATCH.transform_source(b'changed source')
        with self.assertRaisesRegex(ValueError, 'expected source fragments'):
            PATCH.transform_source(SYNTHETIC.replace(PATCH.REGEX_OLD, b''))

    def test_unsupported_architecture_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'architecture'):
            PATCH.patch_package(PATCH_PATH.parent, 'riscv')

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
        spec = PATCH.MODULES['display_utils.py']
        self.assertEqual(digest(self.source), spec['source'])
        patched = PATCH.transform_source(self.source)
        self.assertEqual(digest(patched), spec['patched'])
        compile(patched, 'display_utils.py', 'exec')

    def test_websockets_mode_patches_to_the_recorded_hash_and_fits_the_initial_size(self):
        spec = PATCH.MODULES['websockets_mode.py']
        source = UPSTREAM_WEBSOCKETS.read_bytes()
        self.assertEqual(digest(source), spec['source'])
        patched = PATCH.transform_source(source, spec['replacements'])
        self.assertEqual(digest(patched), spec['patched'])
        compile(patched, 'websockets_mode.py', 'exec')
        text = patched.decode()
        self.assertEqual(text.count('    fit_frame_size,\n'), 1)
        clamp = text.index('target_h = max(1, min(target_h, 4320))')
        fit = text.index('target_w, target_h = fit_frame_size(target_w, target_h)')
        rounding = text.index('if target_w % 2 != 0: target_w -= 1')
        self.assertLess(clamp, fit)
        self.assertLess(fit, rounding)

    def fit_namespace(self):
        text = PATCH.transform_source(self.source).decode()
        first = text.index('MAX_FRAME_MACROBLOCKS = ')
        namespace = {'Optional': __import__('typing').Optional, 'Tuple': tuple}
        exec(text[first:text.index('def cursor_size_for_dpi')], namespace)
        return namespace

    def test_fit_frame_size_keeps_fitting_sizes_and_scales_large_ones(self):
        namespace = self.fit_namespace()
        fit, limit = namespace['fit_frame_size'], namespace['MAX_FRAME_MACROBLOCKS']
        self.assertEqual(limit, 34560)
        self.assertEqual(fit(1440, 900), (1440, 900))
        self.assertEqual(fit(3840, 2160), (3840, 2160))
        for w, h in ((4080, 2508), (4080, 4080), (7680, 4320), (4080, 1000), (4318, 4318)):
            fw, fh = fit(w, h)
            self.assertEqual((fw % 2, fh % 2), (0, 0))
            self.assertLessEqual(-(-fw // 16) * -(-fh // 16), limit)
            self.assertLess(abs(fw / fh - w / h) / (w / h), 0.01)
        self.assertLess(fit(4080, 2508)[0], 4080)

    def test_parse_resize_dims_fits_the_frame_size(self):
        parse = self.fit_namespace()['parse_resize_dims']
        self.assertEqual(parse('1440x900'), (1440, 900))
        self.assertEqual(parse('8000x5000')[0] % 2, 0)
        width, height = parse('4080x2508')
        self.assertLessEqual(-(-width // 16) * -(-height // 16), 34560)
        self.assertIsNone(parse('0x600'))
        self.assertIsNone(parse('bad'))

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
            ('/Gtk/CursorThemeSize', '64'), ('/general/theme', 'Default-xhdpi')])

    def test_cursor_size_follows_the_full_density(self):
        for dpi, cursor in ((192, '64'), (288, '96')):
            store = {}
            self.assertTrue(self.run_xfconf(dpi, store)[0])
            self.assertEqual(store[('xsettings', '/Gtk/CursorThemeSize')], cursor)

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
