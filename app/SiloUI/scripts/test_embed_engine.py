"""The engine embedded in the app is what Service Management and silod expect."""
from pathlib import Path
import json
import plistlib
import tempfile
import unittest

from embed_engine import DISTRIBUTION, SILOD, embed_engine


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

    def embed(self, identifier='org.silo.preview'):
        return embed_engine(self.bundle, self.binary, identifier=identifier, build='silo-test',
                            sign=lambda engine, entitlements: self.signed.append((engine, entitlements)))

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

    def test_the_nested_engine_is_signed_with_the_virtualization_entitlement_before_the_app(self):
        engine, _ = self.embed()
        self.assertEqual(self.signed, [(engine, SILOD / 'macos/Entitlements.plist')])
        entitlements = plistlib.loads((SILOD / 'macos/Entitlements.plist').read_bytes())
        self.assertIs(entitlements['com.apple.security.virtualization'], True)

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
