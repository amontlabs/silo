"""Local desktop build routing and failure propagation, without compiling Tauri."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

from build_desktop import build


class DesktopBuildTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='silo-desktop-build-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.calls = []
        self.target = self.root / 'separate cargo output'
        # Embedding and signing are covered by test_embed_engine.py and test_macos_release.py;
        # here they only have to be asked, in order. `self.steps` records that order.
        self.steps = Mock()
        for name in ('sign_helpers', 'embed_engine', 'seal_app', 'verify_bundle', 'verify_engine',
                     'verify_engine_contents'):
            patcher = patch(f'build_desktop.{name}')
            self.steps.attach_mock(patcher.start(), name)
            self.addCleanup(patcher.stop)
        self.embed = self.steps.embed_engine

    def finalization(self):
        return [name for name, *_ in self.steps.mock_calls]

    def command(self, args, **kwargs):
        self.calls.append((args, kwargs))
        output = json.dumps({'target_directory': str(self.target)}) if args[0] == 'cargo' else ''
        return subprocess.CompletedProcess(args, 0, output)

    def test_local_app_is_finalized_and_verified_in_cargos_actual_output(self):
        args = ['--target', 'aarch64-apple-darwin',
                '--config', '{"bundle":{"createUpdaterArtifacts":true}}']
        result = build(args, root=self.root, platform='darwin', run=self.command)
        expected = self.target / 'aarch64-apple-darwin/release/bundle/macos/Silo.app'
        self.assertEqual(result, expected)
        # The tools are signed before the engine copies them, and the app is sealed around it.
        self.assertEqual(self.finalization(),
                         ['sign_helpers', 'embed_engine', 'seal_app', 'verify_bundle', 'verify_engine'])
        self.steps.sign_helpers.assert_called_once_with(expected)
        self.steps.seal_app.assert_called_once_with(expected)
        self.steps.verify_bundle.assert_called_once_with(expected)
        self.steps.verify_engine.assert_called_once_with(expected, 'org.silo.preview')
        command = next(args for args, _ in self.calls if args[0] == 'node')
        self.assertIn('--bundles', command)
        self.assertIn((['cargo', 'build', '--locked', '--release', '-p', 'silod', '--target', 'aarch64-apple-darwin']),
                      [args for args, _ in self.calls])
        self.embed.assert_called_once()
        self.assertEqual(self.embed.call_args.args[:2], (expected, self.target / 'aarch64-apple-darwin/release/silod'))
        self.assertEqual(self.embed.call_args.kwargs['identifier'], 'org.silo.preview')
        self.assertEqual(command[-2], '--config')
        self.assertEqual(json.loads(command[-1])['bundle'], {
            'active': True, 'createUpdaterArtifacts': False, 'macOS': {'hardenedRuntime': True},
        })

    def test_failed_compilation_never_signs_a_stale_bundle(self):
        def fail(args, **kwargs):
            if args[0] == 'node':
                raise subprocess.CalledProcessError(7, args)
            return self.command(args, **kwargs)
        for args in ([], ['--debug']):
            with self.subTest(args=args), self.assertRaises(subprocess.CalledProcessError):
                build(args, root=self.root, platform='darwin', run=fail)
        self.assertEqual(self.finalization(), [])

    def test_local_configuration_overlay_cannot_redirect_the_bundle_before_signing(self):
        names = {'identifier': 'org.example.production', 'productName': 'Fixture Production'}
        output = self.target / 'release/bundle/macos'
        previous = output / 'Fixture Production.app'
        previous.mkdir(parents=True)
        (previous / 'build-marker').write_text('previous build')

        def builder(args, **kwargs):
            result = self.command(args, **kwargs)
            if args[0] == 'node':
                identity = dict(names)
                for index, argument in enumerate(args):
                    if argument == '--config':
                        overlay = json.loads(args[index + 1])
                        identity.update({key: overlay[key] for key in names if key in overlay})
                bundle = output / (identity['productName'] + '.app')
                bundle.mkdir(parents=True, exist_ok=True)
                (bundle / 'build-marker').write_text(identity['identifier'])
            return result

        def sign_current(bundle):
            self.assertEqual((bundle / 'build-marker').read_text(), names['identifier'])

        overlay = json.dumps({'productName': 'Redirected App', 'identifier': 'org.example.other'})
        self.steps.sign_helpers.side_effect = sign_current
        with patch('build_desktop.channel_names', return_value={'production': names}):
            self.assertEqual(build(['--config', overlay], root=self.root,
                                   platform='darwin', run=builder), previous)
        self.steps.sign_helpers.assert_called_once_with(previous)
        self.steps.verify_bundle.assert_called_once_with(previous)

    def test_signature_or_policy_failure_fails_the_build(self):
        for args, failing in ((['--debug'], 'embed_engine'), (['--debug'], 'seal_app'),
                              (['--debug'], 'verify_engine_contents'), ([], 'sign_helpers'),
                              ([], 'embed_engine'), ([], 'seal_app'), ([], 'verify_bundle'), ([], 'verify_engine')):
            with self.subTest(args=args, failing=failing), \
                    patch(f'build_desktop.{failing}', side_effect=RuntimeError('bad signature')):
                with self.assertRaisesRegex(RuntimeError, 'bad signature'):
                    build(args, root=self.root, platform='darwin', run=self.command)

    def test_a_debug_app_embeds_a_debug_development_engine_and_is_resealed(self):
        for args in (['--debug'], ['-d', '--bundles', 'app']):
            self.calls.clear()
            self.steps.reset_mock()
            with self.subTest(args=args):
                result = build(args, root=self.root, platform='darwin', run=self.command)
                expected = self.target / 'debug/bundle/macos/Silo Dev.app'
                self.assertEqual(result, expected)
                tauri = next(command for command, _ in self.calls if command[0] == 'node')
                self.assertEqual(tauri[3:], args + ['--config', 'src-tauri/tauri.dev.conf.json']
                                 + ([] if '--bundles' in args else ['--bundles', 'app']))
                self.assertIn(['cargo', 'build', '--locked', '-p', 'silod'], [command for command, _ in self.calls])
                self.assertEqual(self.embed.call_args.args[:2], (expected, self.target / 'debug/silod'))
                self.assertEqual(self.embed.call_args.kwargs['identifier'], 'org.silo.dev')
                # Development signatures stay as Tauri made them: no helper re-signing and no
                # hardened runtime on the outer seal.
                self.assertEqual(self.finalization(), ['embed_engine', 'seal_app', 'verify_engine_contents'])
                self.steps.seal_app.assert_called_once_with(expected, hardened=False)
                self.assertIn(['codesign', '--verify', '--deep', '--strict', str(expected)],
                              [command for command, _ in self.calls])

    def test_a_dev_release_app_is_a_release_build_with_the_development_identity(self):
        result = build(['--dev-release', '--bundles', 'app'], root=self.root, platform='darwin', run=self.command)
        expected = self.target / 'release/bundle/macos/Silo Dev.app'
        self.assertEqual(result, expected)
        tauri = next(command for command, _ in self.calls if command[0] == 'node')
        self.assertEqual(tauri[3:], ['--bundles', 'app', '--config', 'src-tauri/tauri.dev.conf.json'])
        self.assertNotIn('--debug', tauri)
        self.assertIn(['cargo', 'build', '--locked', '--release', '-p', 'silod'], [command for command, _ in self.calls])
        self.assertEqual(self.embed.call_args.args[:2], (expected, self.target / 'release/silod'))
        self.assertEqual(self.embed.call_args.kwargs['identifier'], 'org.silo.dev')
        self.assertEqual(self.finalization(), ['embed_engine', 'seal_app', 'verify_engine_contents'])
        self.steps.seal_app.assert_called_once_with(expected, hardened=False)
        with self.assertRaises(ValueError):
            build(['--dev-release', '--debug'], root=self.root, platform='darwin', run=self.command)

    def test_debug_unbundled_help_and_other_platforms_pass_through(self):
        cases = [('darwin', ['--debug', '--no-bundle']), ('darwin', ['--no-bundle', '--ci']),
                 ('darwin', ['--help']), ('darwin', ['--version']),
                 ('linux', ['--no-bundle', '--ci']), ('linux', ['--help']),
                 ('darwin', ['--target', 'aarch64-unknown-linux-gnu', '--bundles', 'deb'])]
        for platform, args in cases:
            self.calls.clear()
            with self.subTest(platform=platform, args=args):
                self.assertIsNone(build(args, root=self.root, platform=platform, run=self.command))
                self.assertEqual(len(self.calls), 1)
                debug = '--debug' in args or '-d' in args
                expected = args + ['--config', 'src-tauri/tauri.dev.conf.json'] if debug else args
                self.assertEqual(self.calls[0][0][3:], expected)
                self.assertEqual(self.finalization(), [])

    def test_only_debug_builds_use_the_development_identity(self):
        dev = 'src-tauri/tauri.dev.conf.json'
        for platform, args in [('darwin', ['--debug', '--bundles', 'app']), ('linux', ['-d', '--no-bundle']),
                               ('linux', ['--debug', '--bundles', 'deb'])]:
            self.calls.clear()
            with self.subTest(platform=platform, args=args):
                build(args, root=self.root, platform=platform, run=self.command)
                tauri = next(command for command, _ in self.calls if command[0] == 'node')
                self.assertIn(dev, tauri)
        for platform, args in [('linux', ['--bundles', 'deb']), ('linux', ['--no-bundle', '--ci']), ('darwin', [])]:
            self.calls.clear()
            with self.subTest(platform=platform, args=args):
                build(args, root=self.root, platform=platform, run=self.command)
                for command, _ in self.calls:
                    self.assertNotIn(dev, command)
                    self.assertFalse(any('org.silo.dev' in part for part in command))

    def test_linux_local_bundle_uses_private_tools_layout_without_updater_artifacts(self):
        args = ['--bundles', 'deb', 'appimage']
        self.assertIsNone(build(args, root=self.root, platform='linux', run=self.command))
        self.assertEqual(len(self.calls), 1)
        command = self.calls[0][0]
        self.assertEqual(command[3:6], args)
        self.assertEqual(command[-3], 'src-tauri/tauri.linux.package.conf.json')
        self.assertEqual(json.loads(command[-1]), {'bundle': {'createUpdaterArtifacts': False}})

    def test_tauri_configuration_precedes_raw_cargo_arguments(self):
        cases = [('darwin', ['--debug', '--no-bundle']),
                 ('linux', ['--debug', '--bundles', 'deb']),
                 ('linux', ['--bundles', 'deb'])]
        for platform, options in cases:
            self.calls.clear()
            with self.subTest(platform=platform, options=options):
                build(options + ['--', '--locked'], root=self.root, platform=platform, run=self.command)
                command = self.calls[0][0]
                separator = command.index('--')
                self.assertEqual(command[separator + 1:], ['--locked'])
                tauri_arguments = command[3:separator]
                if '--debug' in options:
                    self.assertIn('src-tauri/tauri.dev.conf.json', tauri_arguments)
                if platform == 'linux':
                    self.assertIn('src-tauri/tauri.linux.package.conf.json', tauri_arguments)
                    self.assertEqual(json.loads(tauri_arguments[-1]), {
                        'bundle': {'createUpdaterArtifacts': False},
                    })

    def test_distribution_artifacts_cannot_be_made_before_finalization(self):
        for args in (['--bundles', 'dmg'], ['--bundles=all'], ['-b', 'app', 'dmg'], ['--debug', '--bundles', 'dmg'],
                     ['--target', 'universal-apple-darwin'], ['--', '--target-dir', 'elsewhere']):
            with self.subTest(args=args), self.assertRaises(ValueError):
                build(args, root=self.root, platform='darwin', run=self.command)
        self.assertEqual(self.calls, [])


if __name__ == '__main__':
    unittest.main()
