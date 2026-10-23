"""Read host names from the native channel API without building the app."""
import json
from pathlib import Path
import shutil
import tempfile
import unittest

from channel_names import channel_names


SCRIPTS = Path(__file__).resolve().parent


class ChannelNamesTests(unittest.TestCase):
    def test_released_names_and_tauri_identities_are_preserved(self):
        names = channel_names()
        self.assertEqual(names['production'], {
            'identifier': 'org.silo.preview', 'productName': 'Silo', 'stateDir': '.silo',
            'remoteBridge': 'silo-remote',
            'keychain': {'github': 'org.silo.Silo.github', 'secrets': 'org.silo.Silo.secrets'},
        })
        for channel, config in [('production', 'tauri.conf.json'),
                                ('development', 'tauri.dev.conf.json')]:
            tauri = json.loads((SCRIPTS.parent / 'src-tauri' / config).read_text())
            self.assertEqual(names[channel]['identifier'], tauri['identifier'])
            self.assertEqual(names[channel]['productName'], tauri['productName'])

    def test_scripts_follow_changes_to_the_rust_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'scripts').mkdir()
            (root / 'host/src').mkdir(parents=True)
            source = root / 'scripts/channel_names.rs'
            shutil.copyfile(SCRIPTS / source.name, source)
            rust = (SCRIPTS.parent / 'host/src/channel.rs').read_text()
            rust = rust.replace('"org.silo.dev"', '"org.example.fixture"')
            rust = rust.replace('".silo-dev"', '".fixture-state"')
            rust = rust.replace('"org.silo.dev.github"', '"fixture.github"')
            (root / 'host/src/channel.rs').write_text(rust)
            development = channel_names(source)['development']
            self.assertEqual(development['identifier'], 'org.example.fixture')
            self.assertEqual(development['stateDir'], '.fixture-state')
            self.assertEqual(development['keychain']['github'], 'fixture.github')


if __name__ == '__main__':
    unittest.main()
