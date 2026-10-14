"""Local desktop build routing and failure propagation, without compiling Tauri."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from build_desktop import build


class DesktopBuildTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='silo-desktop-build-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.calls = []
        self.target = self.root / 'separate cargo output'
        # Embedding the engine is covered by test_embed_engine.py; here it only has to be asked.
        embedding = patch('build_desktop.embed_engine')
        self.embed = embedding.start()
        self.addCleanup(embedding.stop)

    def command(self, args, **kwargs):
        self.calls.append((args, kwargs))
        output = json.dumps({'target_directory': str(self.target)}) if args[0] == 'cargo' else ''
        return subprocess.CompletedProcess(args, 0, output)

    def test_local_app_is_finalized_and_verified_in_cargos_actual_output(self):
        args = ['--target', 'aarch64-apple-darwin',
                '--config', '{"bundle":{"createUpdaterArtifacts":true}}']
        with patch('build_desktop.sign_runtime') as sign, patch('build_desktop.verify_bundle') as verify:
            result = build(args, root=self.root, platform='darwin', run=self.command)
        expected = self.target / 'aarch64-apple-darwin/release/bundle/macos/Silo.app'
        self.assertEqual(result, expected)
        sign.assert_called_once_with(expected)
        verify.assert_called_once_with(expected)
        command = next(args for args, _ in self.calls if args[0] == 'node')
        self.assertIn('--bundles', command)
        # The engine is built and embedded before the app is signed, so its signature seals.
        self.assertIn((['cargo', 'build', '--locked', '--release', '-p', 'silod', '--target', 'aarch64-apple-darwin']),
                      [args for args, _ in self.calls])
        self.embed.assert_called_once()
        self.assertEqual(self.embed.call_args.args[0], expected)
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
        with patch('build_desktop.sign_runtime') as sign, patch('build_desktop.verify_bundle') as verify:
            with self.assertRaises(subprocess.CalledProcessError):
                build([], root=self.root, platform='darwin', run=fail)
        sign.assert_not_called()
        verify.assert_not_called()

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
        with patch('build_desktop.channel_names', return_value={'production': names}), \
                patch('build_desktop.sign_runtime', side_effect=sign_current) as sign, \
                patch('build_desktop.verify_bundle') as verify:
            self.assertEqual(build(['--config', overlay], root=self.root,
                                   platform='darwin', run=builder), previous)
        sign.assert_called_once_with(previous)
        verify.assert_called_once_with(previous)

    def test_signature_or_policy_failure_fails_the_build(self):
        for failing in ('sign_runtime', 'verify_bundle'):
            with self.subTest(failing=failing), patch('build_desktop.sign_runtime'), \
                    patch('build_desktop.verify_bundle'), \
                    patch(f'build_desktop.{failing}', side_effect=RuntimeError('bad signature')):
                with self.assertRaisesRegex(RuntimeError, 'bad signature'):
                    build([], root=self.root, platform='darwin', run=self.command)

    def test_debug_unbundled_help_and_other_platforms_pass_through(self):
        cases = [('darwin', ['--debug']), ('darwin', ['-d']), ('darwin', ['--no-bundle', '--ci']),
                 ('darwin', ['--help']), ('darwin', ['--version']),
                 ('linux', ['--no-bundle', '--ci']), ('linux', ['--help']),
                 ('darwin', ['--target', 'aarch64-unknown-linux-gnu', '--bundles', 'deb'])]
        for platform, args in cases:
            self.calls.clear()
            with self.subTest(platform=platform, args=args), patch('build_desktop.sign_runtime') as sign:
                self.assertIsNone(build(args, root=self.root, platform=platform, run=self.command))
                self.assertEqual(len(self.calls), 1)
                debug = '--debug' in args or '-d' in args
                expected = args + ['--config', 'src-tauri/tauri.dev.conf.json'] if debug else args
                self.assertEqual(self.calls[0][0][3:], expected)
                sign.assert_not_called()

    def test_only_debug_builds_use_the_development_identity(self):
        dev = 'src-tauri/tauri.dev.conf.json'
        for platform, args in [('darwin', ['--debug', '--bundles', 'app']), ('linux', ['-d', '--no-bundle']),
                               ('linux', ['--debug', '--bundles', 'deb'])]:
            self.calls.clear()
            with self.subTest(platform=platform, args=args):
                build(args, root=self.root, platform=platform, run=self.command)
                self.assertIn(dev, self.calls[0][0])
        for platform, args in [('linux', ['--bundles', 'deb']), ('linux', ['--no-bundle', '--ci']), ('darwin', [])]:
            self.calls.clear()
            with self.subTest(platform=platform, args=args), patch('build_desktop.sign_runtime'), \
                    patch('build_desktop.verify_bundle'):
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
        for args in (['--bundles', 'dmg'], ['--bundles=all'], ['-b', 'app', 'dmg'],
                     ['--target', 'universal-apple-darwin'], ['--', '--target-dir', 'elsewhere']):
            with self.subTest(args=args), self.assertRaises(ValueError):
                build(args, root=self.root, platform='darwin', run=self.command)
        self.assertEqual(self.calls, [])


if __name__ == '__main__':
    unittest.main()
