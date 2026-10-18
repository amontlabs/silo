"""The engine embedded in the app is what Service Management and silod expect."""
from pathlib import Path
import json
import base64
import hashlib
import plistlib
import subprocess
import tempfile
import unittest

from embed_engine import (
    DISTRIBUTION, REQUIREMENT_KEY, SILOD, codesign_engine, embed_engine, engine_requirement, signing_identity,
)
from macos_release_signing import verify_engine_requirement

CERTIFICATE_DER = b'not a real certificate, only bytes to hash'
CERTIFICATE_PEM = ('-----BEGIN CERTIFICATE-----\n' + base64.b64encode(CERTIFICATE_DER).decode()
                   + '\n-----END CERTIFICATE-----\n')


def fake_tools(subject):
    """Stands in for `security find-certificate` and `openssl x509`."""
    calls = []

    def run(command, **kwargs):
        calls.append(command)
        output = CERTIFICATE_PEM if command[0] == 'security' else f'subject={subject}\n'
        return subprocess.CompletedProcess(command, 0, output)
    run.calls = calls
    return run


class EmbedEngineTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bundle = self.root / 'Silo.app'
        (self.bundle / 'Contents').mkdir(parents=True)
        self.binary = self.root / 'silod'
        self.binary.write_bytes(b'#!/bin/sh\n')
        self.signed = []

    def embed(self, identifier='org.silo.preview', **kwargs):
        return embed_engine(self.bundle, self.binary, identifier=identifier, build='silo-test',
                            sign=lambda engine, entitlements: self.signed.append((engine, entitlements)),
                            environ=kwargs.pop('environ', {}), **kwargs)

    def test_the_engine_bundle_and_the_calling_bundles_agent_plist_are_wired_together(self):
        engine, outer = self.embed()
        self.assertEqual(engine, self.bundle / DISTRIBUTION)
        self.assertTrue((engine / 'Contents/MacOS/silod').stat().st_mode & 0o111)
        info = plistlib.loads((engine / 'Contents/Info.plist').read_bytes())
        self.assertEqual(info['CFBundleIdentifier'], 'org.silo.preview.engine')
        manifest = json.loads((engine / 'Contents/Resources/engine-manifest.json').read_text())
        self.assertEqual(manifest['engineBuild'], 'silo-test')
        self.assertEqual(manifest['channelIdentifier'], 'org.silo.preview')
        plist = plistlib.loads(outer.read_bytes())
        self.assertEqual(outer, self.bundle / 'Contents/Library/LaunchAgents/org.silo.preview.engine.plist')
        self.assertEqual(plist['Label'], 'org.silo.preview.engine')
        self.assertEqual(plist['BundleProgram'], f'{DISTRIBUTION}/Contents/MacOS/silod')
        self.assertTrue((self.bundle / plist['BundleProgram']).is_file())
        # The engine's own copy keeps its relative BundleProgram.
        inner = plistlib.loads((engine / 'Contents/Library/LaunchAgents/org.silo.preview.engine.plist').read_bytes())
        self.assertEqual(inner['BundleProgram'], 'Contents/MacOS/silod')

    def test_neither_channel_marks_the_engine_as_an_agent_app(self):
        # silod sets its own activation policy (accessory, regular while a VM window is open).
        # LSUIElement made AppKit refuse that call, and silod exited at startup.
        for identifier in ('org.silo.preview', 'org.silo.dev'):
            engine, _ = self.embed(identifier)
            info = plistlib.loads((engine / 'Contents/Info.plist').read_bytes())
            self.assertNotIn('LSUIElement', info)
        for channel in ('development', 'production'):
            info = plistlib.loads((SILOD / f'macos/Info.{channel}.plist').read_bytes())
            self.assertNotIn('LSUIElement', info)

    def test_the_nested_engine_is_signed_with_the_virtualization_entitlement_before_the_app(self):
        engine, _ = self.embed()
        self.assertEqual(self.signed, [(engine, SILOD / 'macos/Entitlements.plist')])
        entitlements = plistlib.loads((SILOD / 'macos/Entitlements.plist').read_bytes())
        self.assertIs(entitlements['com.apple.security.virtualization'], True)

    def test_the_signing_identity_is_ad_hoc_unless_the_environment_names_one(self):
        self.assertEqual(signing_identity({}), '-')
        self.assertEqual(signing_identity({'SILO_ENGINE_SIGNING_IDENTITY': '  '}), '-')
        self.assertEqual(signing_identity({'SILO_ENGINE_SIGNING_IDENTITY': 'Silo Dev Signing'}), 'Silo Dev Signing')
        for environ, expected in (({}, '-'), ({'SILO_ENGINE_SIGNING_IDENTITY': 'Silo Dev Signing'}, 'Silo Dev Signing')):
            calls = []
            codesign_engine(self.root / 'e.app', self.root / 'ent.plist',
                            run=lambda command, **_: calls.append(command), environ=environ)
            sign = calls[0]
            self.assertEqual(sign[sign.index('--sign') + 1], expected)
            self.assertIn('runtime', sign)

    def test_the_engine_names_its_signer_in_its_info_plist_before_it_is_signed(self):
        engine, _ = self.embed()
        info = plistlib.loads((engine / 'Contents/Info.plist').read_bytes())
        self.assertEqual(info[REQUIREMENT_KEY], 'identifier "org.silo.preview.engine"')
        self.assertEqual(info['CFBundleIdentifier'], 'org.silo.preview.engine')
        engine, _ = self.embed(requirement='anchor apple generic')
        info = plistlib.loads((engine / 'Contents/Info.plist').read_bytes())
        self.assertEqual(info[REQUIREMENT_KEY], 'anchor apple generic')
        # The checked-in plists carry no requirement: packagers derive it from their signer.
        for channel in ('development', 'production'):
            source = plistlib.loads((SILOD / f'macos/Info.{channel}.plist').read_bytes())
            self.assertNotIn(REQUIREMENT_KEY, source)

    def test_the_requirement_follows_the_signing_identity(self):
        name = {'SILO_ENGINE_SIGNING_IDENTITY': 'Developer ID Application: Amont (ABCDE12345)'}
        engine = 'org.silo.preview.engine'
        self.assertEqual(engine_requirement(engine, {}), 'identifier "org.silo.preview.engine"')
        developer_id = fake_tools('UID=ABCDE12345,CN=Developer ID Application: Amont (ABCDE12345),OU=ABCDE12345,O=Amont,C=US')
        self.assertEqual(engine_requirement(engine, name, developer_id),
                         'anchor apple generic and certificate leaf[subject.OU] = "ABCDE12345"')
        self.assertEqual(developer_id.calls[0], ['security', 'find-certificate', '-c', name['SILO_ENGINE_SIGNING_IDENTITY'], '-p'])
        local = {'SILO_ENGINE_SIGNING_IDENTITY': 'Silo Dev Signing'}
        pinned = f'certificate leaf = H"{hashlib.sha1(CERTIFICATE_DER).hexdigest()}"'
        self.assertEqual(engine_requirement(engine, local, fake_tools('CN=Silo Dev Signing')), pinned)
        # A team on a non-Developer-ID certificate is not an Apple-anchored identity.
        self.assertEqual(engine_requirement(engine, local, fake_tools('CN=Silo Dev Signing,OU=ABCDE12345')), pinned)
        digest = 'AB' * 20
        self.assertEqual(engine_requirement(engine, {'SILO_ENGINE_SIGNING_IDENTITY': digest}),
                         f'certificate leaf = H"{digest.lower()}"')

    def test_the_embedded_requirement_comes_from_the_identity_that_signs(self):
        environ = {'SILO_ENGINE_SIGNING_IDENTITY': 'Silo Dev Signing'}
        engine, _ = self.embed(environ=environ, run=fake_tools('CN=Silo Dev Signing'))
        info = plistlib.loads((engine / 'Contents/Info.plist').read_bytes())
        self.assertRegex(info[REQUIREMENT_KEY], r'^certificate leaf = H"[0-9a-f]{40}"$')

    def test_verification_rejects_an_engine_without_a_requirement(self):
        engine, _ = self.embed()
        verify_engine_requirement(engine)
        info = engine / 'Contents/Info.plist'
        for value in (None, '', '  ', 1):
            plist = plistlib.loads(info.read_bytes())
            plist.pop(REQUIREMENT_KEY, None)
            if value is not None:
                plist[REQUIREMENT_KEY] = value
            info.write_bytes(plistlib.dumps(plist))
            with self.subTest(value=value), self.assertRaisesRegex(RuntimeError, REQUIREMENT_KEY):
                verify_engine_requirement(engine)

    def test_embedding_again_replaces_the_previous_engine(self):
        engine, _ = self.embed()
        (engine / 'Contents/stale').write_text('old')
        self.embed()
        self.assertFalse((engine / 'Contents/stale').exists())

    def test_a_missing_packaging_input_is_reported(self):
        with self.assertRaises(FileNotFoundError):
            embed_engine(self.bundle, self.root / 'missing', build='x', sign=lambda *_: None)

    def test_the_development_channel_uses_its_own_names(self):
        engine, outer = self.embed('org.silo.dev')
        self.assertEqual(plistlib.loads((engine / 'Contents/Info.plist').read_bytes())['CFBundleIdentifier'], 'org.silo.dev.engine')
        self.assertEqual(outer.name, 'org.silo.dev.engine.plist')


if __name__ == '__main__':
    unittest.main()
