"""Exercise the release signing gate against real disposable macOS signatures."""
from pathlib import Path
import importlib.util
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import tarfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('verify-macos-signing.py')
TOOLS = ('silo-ui', 'msb', 'git', 'git-lfs', 'git-remote-http', 'git-remote-https')


@unittest.skipUnless(sys.platform == 'darwin', 'Requires macOS codesign and clang')
class MacOSReleaseSigningTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix='silo-release-signing-')
        cls.root = Path(cls.temporary.name)
        source = cls.root / 'main.c'
        source.write_text('''#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char **argv) {
    if (argc != 2) return 0;
    void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!library) { fprintf(stderr, "%s\\n", dlerror()); return 1; }
    int (*value)(void) = dlsym(library, "value");
    return value && value() == 7 ? 0 : 2;
}''')
        cls.run_command('clang', '-arch', 'arm64', source, '-o', cls.root / 'executable')
        source.write_text('int value(void){return 7;}')
        cls.run_command('clang', '-arch', 'arm64', '-dynamiclib', source, '-o', cls.root / 'engine.dylib')
        source.write_text('int value(void){return 9;}')
        cls.run_command('clang', '-arch', 'arm64', '-dynamiclib', source, '-o', cls.root / 'replacement.dylib')

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    @staticmethod
    def run_command(*args):
        return subprocess.run(list(map(str, args)), capture_output=True, check=True)

    def setUp(self):
        self.case = Path(tempfile.mkdtemp(dir=self.root))
        self.app = self.case / 'target/release/bundle/macos/Silo.app'
        self.binaries = self.app / 'Contents/MacOS'
        self.frameworks = self.app / 'Contents/Frameworks'
        self.binaries.mkdir(parents=True)
        self.frameworks.mkdir()
        (self.app / 'Contents/Info.plist').write_bytes(plistlib.dumps({
            'CFBundleExecutable': 'silo-ui', 'CFBundleIdentifier': 'org.silo.test',
            'CFBundleName': 'Silo', 'CFBundlePackageType': 'APPL',
            'CFBundleVersion': '1', 'CFBundleShortVersionString': '0.1.1',
            'LSMinimumSystemVersion': '14.0',
        }))
        for name in TOOLS:
            shutil.copy2(self.root / 'executable', self.binaries / name)
            self.sign(self.binaries / name)
        self.engine = self.frameworks / 'libkrunfw.5.dylib'
        shutil.copy2(self.root / 'engine.dylib', self.engine)
        self.sign(self.engine)
        output = self.run_command('codesign', '-d', '--verbose=4', self.engine).stderr
        self.digest = bytes.fromhex(re.search(rb'^CDHash=(\w+)$', output, re.M)[1].decode())
        self.sign_helper({'cdhash': self.digest})
        self.seal()

    def sign(self, target, *, entitlements=None, constraint=None, hardened=True):
        args = ['codesign', '--force', '--sign', '-']
        if hardened:
            args += ['--options', 'runtime']
        if entitlements is not None:
            path = self.case / 'entitlements.plist'
            path.write_bytes(plistlib.dumps(entitlements))
            args += ['--entitlements', path]
        if constraint is not None:
            path = self.case / 'constraint.plist'
            path.write_bytes(plistlib.dumps(constraint))
            args += ['--enforce-constraint-validity', '--library-constraint', path]
        self.run_command(*args, target)

    def sign_helper(self, constraint):
        self.sign(self.binaries / 'msb', entitlements={
            'com.apple.security.hypervisor': True,
            'com.apple.security.cs.disable-library-validation': True,
        }, constraint=constraint)

    def seal(self, virtualization=True):
        self.sign(self.app, entitlements={
            'com.apple.security.hypervisor': True,
            **({'com.apple.security.virtualization': True} if virtualization else {}),
            'com.apple.security.automation.apple-events': True,
        })

    def verify(self, expected):
        result = subprocess.run([sys.executable, str(SCRIPT), str(self.app)], capture_output=True, text=True)
        if expected:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, 'Unsafe bundle passed signing gate')

    def test_exact_constraint_accepted(self):
        self.verify(True)

    def test_local_build_repairs_tauri_signature_before_reporting_success(self):
        # Tauri gives the helper the app's entitlements. Its signature is valid,
        # but dyld rejects the engine until local packaging finalizes the helper.
        self.sign(self.binaries / 'msb', entitlements={
            'com.apple.security.hypervisor': True,
            'com.apple.security.automation.apple-events': True,
        })
        self.seal()
        self.run_command('codesign', '--verify', '--deep', '--strict', self.app)

        # Exercise the public npm command with only compilation replaced. This
        # also fails if package.json is ever changed back to plain tauri build.
        scripts = self.case / 'scripts'
        scripts.mkdir()
        for name in ('build_desktop.py', 'macos_release_signing.py', 'channel_names.py', 'channel_names.rs'):
            shutil.copy2(SCRIPT.with_name(name), scripts / name)
        shutil.copy2(SCRIPT.parent.parent / 'package.json', self.case / 'package.json')
        (self.case / 'src-tauri/src').mkdir(parents=True)
        shutil.copy2(SCRIPT.parent.parent / 'src-tauri/src/channel.rs',
                     self.case / 'src-tauri/src/channel.rs')
        shutil.copy2(SCRIPT.parent.parent / 'src-tauri/Entitlements.plist',
                     self.case / 'src-tauri/Entitlements.plist')
        cli = self.case / 'node_modules/@tauri-apps/cli/tauri.js'
        cli.parent.mkdir(parents=True)
        cli.write_text('#!/usr/bin/env node\nprocess.exit(0)\n')
        cli.chmod(0o755)
        (self.case / 'node_modules/.bin').mkdir()
        (self.case / 'node_modules/.bin/tauri').symlink_to(cli)
        tools = self.case / 'tools'
        tools.mkdir()
        cargo = tools / 'cargo'
        metadata = json.dumps({'target_directory': str(self.case / 'target')})
        cargo.write_text(f'#!{sys.executable}\nprint({metadata!r})\n')
        cargo.chmod(0o755)
        result = subprocess.run(['npm', 'run', 'desktop:build'], cwd=self.case,
                                env=dict(os.environ, PATH=f'{tools}{os.pathsep}{os.environ["PATH"]}'),
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        loaded = subprocess.run([str(self.binaries / 'msb'), str(self.engine)], capture_output=True, text=True)
        self.assertEqual(loaded.returncode, 0, loaded.stderr)
        self.verify(True)

    def test_broad_exception_without_constraint_rejected(self):
        self.sign_helper(None)
        self.seal()
        self.verify(False)

    def test_additional_permitted_library_rejected(self):
        self.sign_helper({'cdhash': {'$in': [self.digest, bytes(20)]}})
        self.seal()
        self.verify(False)

    def test_signed_replacement_engine_rejected(self):
        replacement = self.case / 'swapped.dylib'
        shutil.copy2(self.root / 'replacement.dylib', replacement)
        self.sign(replacement)
        replacement.replace(self.engine)
        self.seal()
        self.verify(False)

    def test_git_library_exception_rejected(self):
        self.sign(self.binaries / 'git', entitlements={'com.apple.security.cs.disable-library-validation': True})
        self.seal()
        self.verify(False)

    def test_app_without_the_virtualization_entitlement_rejected(self):
        self.seal(virtualization=False)
        self.verify(False)

    def test_non_hardened_helper_rejected(self):
        self.sign(self.binaries / 'git', hardened=False)
        self.seal()
        self.verify(False)

    def test_tampered_engine_rejected(self):
        data = bytearray(self.engine.read_bytes())
        data[4096] ^= 1
        self.engine.write_bytes(data)
        self.verify(False)


class MacOSReleaseSignerEnvironmentTests(unittest.TestCase):
    def run_packager(self, file_key, signer_exit=0, product_name="Silo"):
        from channel_names import channel_names
        names = channel_names()
        names = {**names, "production": {**names["production"], "productName": product_name}}
        with tempfile.TemporaryDirectory(prefix='silo-signer-env-') as temporary:
            root = Path(temporary)
            app = root / f'{product_name}.app'
            (app / 'Contents/MacOS').mkdir(parents=True)
            (app / 'Contents/MacOS/msb').write_bytes(b'fixture runtime')
            key = 'disposable-inline-fixture'
            if file_key:
                key_file = root / 'fixture.key'
                key_file.write_text('disposable-key-fixture')
                key = str(key_file)
            script = SCRIPT.with_name('package-macos-release.py')
            with patch.object(sys, 'path', [str(script.parent), *sys.path]):
                spec = importlib.util.spec_from_file_location('package_macos_release_test', script)
                module = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(module)
            invocations = []

            def command(args, **kwargs):
                invocations.append((args, kwargs))
                if args[0] == 'npx' and signer_exit:
                    raise subprocess.CalledProcessError(signer_exit, args)
                return subprocess.CompletedProcess(args, 0)

            environment = {'TAURI_SIGNING_PRIVATE_KEY': key,
                           'TAURI_SIGNING_PRIVATE_KEY_PATH': 'stale-path',
                           'TAURI_SIGNING_PRIVATE_KEY_PASSWORD': 'disposable-password-sentinel'}
            with patch.dict(os.environ, environment), \
                    patch.object(sys, 'argv', [str(script), str(app), str(root / 'output')]), \
                    patch.object(module, 'sign_runtime'), patch.object(module, 'verify_bundle'), \
                    patch.object(module, 'channel_names', return_value=names, create=True), \
                    patch.object(module.subprocess, 'run', side_effect=command):
                if signer_exit:
                    with self.assertRaisesRegex(RuntimeError, 'Updater signing failed.*exit code 17') as failure:
                        module.main()
                    self.assertNotIn(environment['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'], str(failure.exception))
                else:
                    module.main()
            if not signer_exit:
                archive = root / 'output' / f'{product_name}-macos-arm64.app.tar.gz'
                with tarfile.open(archive) as packaged:
                    self.assertTrue(all(entry.name.split('/')[0] == f'{product_name}.app'
                                        for entry in packaged))
                ditto = next(args for args, _ in invocations if args[0] == 'ditto')
                self.assertEqual(Path(ditto[-1]).name, f'{product_name}.app')
                image = next(args for args, _ in invocations if args[0] == 'hdiutil')
                self.assertEqual(image[image.index('-volname') + 1], product_name)
            signer = [(args, kwargs) for args, kwargs in invocations if args[0] == 'npx']
            self.assertEqual(len(signer), 1)
            args, kwargs = signer[0]
            self.assertNotIn(environment['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'], args)
            self.assertNotIn('-p', args)
            self.assertEqual(kwargs['env']['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'], environment['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'])
            self.assertNotIn('TAURI_SIGNING_PRIVATE_KEY_PATH', kwargs['env'])
            if file_key:
                self.assertEqual(args[args.index('-f') + 1], key)
                self.assertNotIn('TAURI_SIGNING_PRIVATE_KEY', kwargs['env'])
            else:
                self.assertNotIn('-f', args)
                self.assertEqual(kwargs['env']['TAURI_SIGNING_PRIVATE_KEY'], key)
                self.assertNotIn(key, args)

    def test_bundle_name_comes_from_the_production_channel(self):
        self.run_packager(file_key=False, product_name="Fixture Channel")

    def test_file_key_removes_conflicting_inline_environment(self):
        self.run_packager(file_key=True)

    def test_inline_key_remains_in_environment_only(self):
        self.run_packager(file_key=False)

    def test_signer_failure_does_not_disclose_password(self):
        self.run_packager(file_key=False, signer_exit=17)


if __name__ == '__main__':
    unittest.main()
