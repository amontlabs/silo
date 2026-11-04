"""Developer ID signing, notarization and stapling steps, and their dry-run stubs."""
from pathlib import Path
import base64
import importlib.util
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))
from macos_notarization import Release, import_certificate, release_mode  # noqa: E402

REPOSITORY = SCRIPTS.parent.parent.parent
NOTARY = {
    'APPLE_TEAM_ID': 'TEAM123456',
    'APPLE_API_KEY_P8': 'p8-secret-sentinel',
    'APPLE_API_KEY_ID': 'KEYID-sentinel',
    'APPLE_API_ISSUER_ID': 'issuer-sentinel',
}
DEVELOPER_ID = {**NOTARY, 'SILO_RELEASE_SIGNING': 'developer-id',
                'APPLE_DEVELOPER_ID_IDENTITY': 'Developer ID Application: Example (TEAM123456)'}
ACCEPTED = json.dumps({'status': 'Accepted', 'id': 'abc'})


def fail(*args, **kwargs):
    raise AssertionError(f'A dry run must not execute {args}')


class ModeTests(unittest.TestCase):
    def test_default_is_ad_hoc_and_unknown_modes_are_rejected(self):
        self.assertEqual(release_mode({}), 'adhoc')
        self.assertEqual(release_mode({'SILO_RELEASE_SIGNING': 'dry-run'}), 'dry-run')
        with self.assertRaisesRegex(RuntimeError, 'must be one of'):
            release_mode({'SILO_RELEASE_SIGNING': 'yes'})

    def test_developer_id_requires_every_credential_and_a_developer_id_identity(self):
        with self.assertRaisesRegex(RuntimeError, 'APPLE_API_KEY_P8') as error:
            Release.from_environ({**DEVELOPER_ID, 'APPLE_API_KEY_P8': ''})
        self.assertNotIn('p8-secret-sentinel', str(error.exception))
        with self.assertRaisesRegex(RuntimeError, 'APPLE_DEVELOPER_ID_IDENTITY'):
            Release.from_environ({k: v for k, v in DEVELOPER_ID.items() if k != 'APPLE_DEVELOPER_ID_IDENTITY'})
        with self.assertRaisesRegex(RuntimeError, 'Developer ID Application'):
            Release.from_environ({**DEVELOPER_ID, 'APPLE_DEVELOPER_ID_IDENTITY': 'Apple Development: Someone'})

    def test_ad_hoc_and_dry_run_need_no_credentials_and_sign_ad_hoc(self):
        for mode in ('adhoc', 'dry-run'):
            release = Release.from_environ({'SILO_RELEASE_SIGNING': mode,
                                            'APPLE_DEVELOPER_ID_IDENTITY': 'Developer ID Application: Ignored'})
            self.assertEqual(release.identity, '-')


class StepTests(unittest.TestCase):
    app = Path('/build/Silo.app')
    dmg = Path('/out/Silo-macos-arm64.dmg')

    def run_steps(self, release):
        release.notarize_app(self.app, '/work')
        release.notarize_dmg(self.dmg)
        return [' '.join(step) for step in release.steps]

    def test_ad_hoc_runs_nothing(self):
        release = Release('adhoc', run=fail, out=fail)
        self.assertEqual(self.run_steps(release), [])

    def test_dry_run_prints_the_pipeline_in_order_and_executes_nothing(self):
        printed = []
        release = Release('dry-run', environ=NOTARY, run=fail, out=printed.append)
        steps = self.run_steps(release)
        self.assertEqual(len(printed), len(steps))
        self.assertTrue(all(line.startswith('[dry-run] ') for line in printed))
        names = [' '.join(s.split()[:3]) for s in steps]
        self.assertEqual(names, [
            'ditto -c -k',
            'xcrun notarytool submit',
            'xcrun stapler staple',
            'xcrun stapler validate',
            'codesign --verify --deep',
            'spctl --assess --type',
            'codesign --force --sign',
            'codesign --verify --strict',
            'xcrun notarytool submit',
            'xcrun stapler staple',
            'xcrun stapler validate',
            'spctl --assess --type',
        ])
        text = '\n'.join(printed)
        for secret in NOTARY.values():
            self.assertNotIn(secret, text)
        self.assertIn('--wait', text)
        self.assertIn('--sign -', text)

    def test_developer_id_submits_waits_staples_then_assesses_the_app_and_the_image(self):
        calls = []

        def run(args, **kwargs):
            calls.append(args)
            if args[1:3] == ['notarytool', 'submit']:
                key = args[args.index('--key') + 1]
                self.assertEqual(Path(key).read_text(), 'p8-secret-sentinel')
                self.assertEqual(stat.S_IMODE(Path(key).stat().st_mode), 0o600)
                return subprocess.CompletedProcess(args, 0, ACCEPTED, '')
            return subprocess.CompletedProcess(args, 0, '', '')

        release = Release.from_environ(DEVELOPER_ID, run=run)
        self.run_steps(release)
        key = next(c[c.index('--key') + 1] for c in calls if c[1:3] == ['notarytool', 'submit'])
        release.cleanup()
        self.assertFalse(Path(key).exists())
        flat = [' '.join(c[:3]) for c in calls]
        self.assertEqual(flat.index('xcrun notarytool submit') < flat.index('xcrun stapler staple'), True)
        self.assertEqual(sum(1 for c in calls if c[1:3] == ['notarytool', 'submit']), 2)
        self.assertEqual(sum(1 for c in calls if c[1:3] == ['stapler', 'staple']), 2)
        signed = next(c for c in calls if c[:2] == ['codesign', '--force'])
        self.assertEqual(signed[signed.index('--sign') + 1], 'Developer ID Application: Example (TEAM123456)')
        self.assertIn('--timestamp', signed)
        shown = '\n'.join(' '.join(step) for step in release.steps)
        for name in ('APPLE_API_KEY_P8', 'APPLE_API_KEY_ID', 'APPLE_API_ISSUER_ID'):
            self.assertNotIn(NOTARY[name], shown)

    def test_a_rejected_submission_fails_with_its_id(self):
        def run(args, **kwargs):
            out = json.dumps({'status': 'Invalid', 'id': 'bad-id'}) if args[1:3] == ['notarytool', 'submit'] else ''
            return subprocess.CompletedProcess(args, 0, out, '')

        release = Release.from_environ(DEVELOPER_ID, run=run)
        try:
            with self.assertRaisesRegex(RuntimeError, 'Invalid.*bad-id'):
                release.notarize_app(self.app, '/work')
        finally:
            release.cleanup()
        self.assertFalse(any(step[1:3] == ['stapler', 'staple'] for step in release.steps))


class CertificateTests(unittest.TestCase):
    def test_the_p12_goes_into_a_temporary_keychain_and_is_removed(self):
        calls = []

        def run(args, **kwargs):
            calls.append(args)
            return subprocess.CompletedProcess(args, 0, '"/Users/runner/Library/Keychains/login.keychain-db"\n', '')

        with tempfile.TemporaryDirectory() as temporary:
            environ = {'RUNNER_TEMP': temporary, 'APPLE_CERTIFICATE_P12_BASE64': base64.b64encode(b'p12').decode(),
                       'APPLE_CERTIFICATE_PASSWORD': 'p12-password'}
            keychain = import_certificate(environ, run)
            self.assertEqual(sorted(os.listdir(temporary)), [])
        names = [c[1] for c in calls]
        self.assertEqual(names[:5], ['create-keychain', 'set-keychain-settings', 'unlock-keychain', 'import',
                                     'set-key-partition-list'])
        self.assertEqual(calls[-1][:5], ['security', 'list-keychains', '-d', 'user', '-s'])
        self.assertEqual(calls[-1][5], keychain)
        with self.assertRaisesRegex(RuntimeError, 'APPLE_CERTIFICATE_PASSWORD'):
            import_certificate({'APPLE_CERTIFICATE_P12_BASE64': 'x'}, run)


class CertificateFailureTests(unittest.TestCase):
    def test_a_failed_import_deletes_the_keychain_and_reports_no_passwords(self):
        calls = []

        def run(args, **kwargs):
            calls.append(args)
            if args[1] == 'import':
                raise subprocess.CalledProcessError(1, args)
            return subprocess.CompletedProcess(args, 0, '', '')

        with tempfile.TemporaryDirectory() as temporary:
            environ = {'RUNNER_TEMP': temporary, 'APPLE_CERTIFICATE_P12_BASE64': base64.b64encode(b'p12').decode(),
                       'APPLE_CERTIFICATE_PASSWORD': 'p12-password-sentinel'}
            with self.assertRaises(RuntimeError) as failure:
                import_certificate(environ, run)
            self.assertEqual(os.listdir(temporary), [])
        # `from None` keeps the original arguments out of the printed traceback.
        self.assertTrue(failure.exception.__suppress_context__)
        self.assertIsNone(failure.exception.__cause__)
        message = str(failure.exception)
        self.assertIn('security import', message)
        self.assertNotIn('p12-password-sentinel', message)
        created = next(c for c in calls if c[1] == 'create-keychain')
        self.assertNotIn(created[3], message)
        self.assertEqual(calls[-1][:2], ['security', 'delete-keychain'])
        self.assertEqual(calls[-1][2], created[4])


class SignOtherCodeTests(unittest.TestCase):
    def test_a_release_identity_signs_every_other_binary_before_the_constrained_helpers(self):
        import macos_release_signing as signing
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / 'Silo.app'
            (app / 'Contents/MacOS').mkdir(parents=True)
            (app / 'Contents/Frameworks').mkdir()
            (app / 'Contents/Resources').mkdir()
            for name in ('silo-ui', 'msb', 'git', 'git-lfs'):
                (app / 'Contents/MacOS' / name).write_bytes(bytes.fromhex('cffaedfe') + b'x')
            (app / 'Contents/Frameworks/libkrunfw.5.dylib').write_bytes(bytes.fromhex('cffaedfe') + b'x')
            (app / 'Contents/Resources/notes.md').write_text('text')
            calls = []
            with patch.object(signing, 'command', side_effect=lambda *a: calls.append([str(x) for x in a])), \
                    patch.object(signing, 'policy_files', return_value=(Path('e'), Path('c'))):
                signing.sign_helpers(app, 'Developer ID Application: Example (TEAM123456)')
                first = [Path(c[-1]).name for c in calls if '--preserve-metadata=identifier,entitlements' in c]
                self.assertEqual(first, ['libkrunfw.5.dylib', 'git', 'git-lfs'])
                self.assertTrue(all('--timestamp' in c for c in calls))
                self.assertEqual(Path(calls[0][-1]).name, 'libkrunfw.5.dylib')
                calls.clear()
                signing.sign_helpers(app)
                self.assertNotIn('git', [Path(c[-1]).name for c in calls])


class PackagerDryRunTests(unittest.TestCase):
    def load(self):
        script = SCRIPTS / 'package-macos-release.py'
        spec = importlib.util.spec_from_file_location('package_macos_release_dry_run', script)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return script, module

    def test_the_whole_pipeline_runs_in_dry_run_without_apple_credentials(self):
        script, module = self.load()
        with tempfile.TemporaryDirectory(prefix='silo-dry-run-') as temporary:
            root = Path(temporary)
            app = root / 'Silo.app'
            (app / 'Contents/MacOS').mkdir(parents=True)
            (app / 'Contents/MacOS/msb').write_bytes(b'x')
            events = []

            def record(name):
                return lambda *args, **kwargs: events.append(name)

            def command(args, **kwargs):
                events.append(args[0])
                return subprocess.CompletedProcess(args, 0)

            printed = []
            names = module.channel_names()
            environment = {'TAURI_SIGNING_PRIVATE_KEY': 'k', 'SILO_RELEASE_SIGNING': 'dry-run'}
            original = module.Release.from_environ
            with patch.dict(os.environ, environment), \
                    patch.object(sys, 'argv', [str(script), str(app), str(root / 'out'),
                                               '--engine', str(root / 'silod'), '--engine-build', 'silo-abc']), \
                    patch.object(module, 'sign_helpers', side_effect=record('sign_helpers')), \
                    patch.object(module, 'embed_engine', side_effect=record('embed')), \
                    patch.object(module, 'seal_app', side_effect=record('seal')), \
                    patch.object(module, 'verify_bundle', side_effect=record('verify')), \
                    patch.object(module, 'verify_engine', side_effect=record('verify_engine')), \
                    patch.object(module.Release, 'from_environ',
                                 side_effect=lambda: original(os.environ, run=fail, out=lambda line: (
                                     printed.append(line), events.append('stub:' + line.split()[1:4][-1]))[0])), \
                    patch.object(module, 'channel_names', return_value=names), \
                    patch.object(module.subprocess, 'run', side_effect=command):
                module.main()
        order = [e for e in events if e in ('sign_helpers', 'embed', 'seal', 'verify', 'verify_engine', 'npx', 'hdiutil')
                 or e.startswith('stub:')]
        stubs = [e for e in order if e.startswith('stub:')]
        self.assertEqual(order[:5], ['sign_helpers', 'embed', 'seal', 'verify', 'verify_engine'])
        # The app is notarized and stapled before it is archived and signed for updates;
        # the image is built from the stapled app, then notarized.
        self.assertEqual(order.index('npx') > order.index(stubs[0]), True)
        self.assertEqual(order.index('hdiutil') > order.index('npx'), True)
        after_image = order[order.index('hdiutil') + 1:]
        self.assertTrue(all(e.startswith('stub:') for e in after_image) and after_image)
        self.assertTrue(printed and all(line.startswith('[dry-run] ') for line in printed))


class VerificationIdentityTests(unittest.TestCase):
    def test_verification_uses_the_release_identity(self):
        from macos_notarization import release_identity
        self.assertEqual(release_identity({}), '-')
        self.assertEqual(release_identity({'SILO_RELEASE_SIGNING': 'dry-run'}), '-')
        self.assertEqual(release_identity(DEVELOPER_ID), 'Developer ID Application: Example (TEAM123456)')
        # Exactly the environment of the workflow's verification step: no notarization credentials.
        verify_env = {'SILO_RELEASE_SIGNING': 'developer-id',
                      'APPLE_DEVELOPER_ID_IDENTITY': DEVELOPER_ID['APPLE_DEVELOPER_ID_IDENTITY']}
        self.assertEqual(release_identity(verify_env), DEVELOPER_ID['APPLE_DEVELOPER_ID_IDENTITY'])
        with self.assertRaisesRegex(RuntimeError, 'Developer ID Application'):
            release_identity({**verify_env, 'APPLE_DEVELOPER_ID_IDENTITY': 'Apple Development: X'})
        with self.assertRaisesRegex(RuntimeError, 'APPLE_DEVELOPER_ID_IDENTITY'):
            release_identity({'SILO_RELEASE_SIGNING': 'developer-id'})
        script = SCRIPTS / 'verify-macos-signing.py'
        seen = []
        import macos_release_signing
        import runpy
        with patch.object(macos_release_signing, 'verify_bundle', lambda app, identity='-': seen.append(identity)), \
                patch.dict(os.environ, verify_env, clear=True), patch.object(sys, 'argv', [str(script), '/x/Silo.app']):
            runpy.run_path(str(script), run_name='__main__')
        self.assertEqual(seen, ['Developer ID Application: Example (TEAM123456)'])


class WorkflowTests(unittest.TestCase):
    def text(self, name):
        return (REPOSITORY / '.github/workflows' / name).read_text()

    def test_release_workflow_has_a_dry_run_input_and_rejects_it_for_drafts(self):
        workflow = self.text('release.yml')
        self.assertIn('dry_run:', workflow)
        self.assertIn('signing: ${{ steps.release.outputs.signing }}', workflow)
        self.assertIn('DRY_RUN: ${{ inputs.dry_run }}', workflow)
        for name in ('APPLE_CERTIFICATE_P12_BASE64', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_DEVELOPER_ID_IDENTITY',
                     'APPLE_TEAM_ID', 'APPLE_API_KEY_P8', 'APPLE_API_KEY_ID', 'APPLE_API_ISSUER_ID'):
            self.assertIn(f'{name}: ${{{{ secrets.{name} }}}}', workflow)

    def test_credentials_reach_only_the_signing_steps_of_the_platform_workflow(self):
        workflow = self.text('release-platform.yml')
        build = workflow[workflow.index('\n  build:'):]
        header = build[:build.index('    steps:')]
        self.assertNotIn('APPLE_API_KEY', header)
        self.assertNotIn('APPLE_CERTIFICATE', header)
        # The mode is set per step: the tooling tests run the packager before credentials exist.
        self.assertNotIn('SILO_RELEASE_SIGNING', header)
        self.assertEqual(build.count('SILO_RELEASE_SIGNING: ${{ inputs.signing }}'), 2)
        self.assertNotIn('$(python3 scripts/macos_notarization.py import-certificate)" >>', build)
        verify = build[build.index('- name: Collect and verify macOS assets'):
                       build.index('- name: Delete the temporary signing keychain')]
        self.assertIn('APPLE_DEVELOPER_ID_IDENTITY', verify)
        self.assertIn('verify-macos-signing.py', verify)
        step = build[build.index('- name: Enforce exact macOS runtime libraries'):]
        step = step[:step.index('- name: Verify macOS library constraints')]
        for name in ('APPLE_DEVELOPER_ID_IDENTITY', 'APPLE_TEAM_ID', 'APPLE_API_KEY_P8',
                     'APPLE_API_KEY_ID', 'APPLE_API_ISSUER_ID'):
            self.assertIn(name, step)
        self.assertIn('macos_notarization.py import-certificate', build)
        self.assertIn('macos_notarization.py delete-keychain', build)


if __name__ == '__main__':
    unittest.main()
