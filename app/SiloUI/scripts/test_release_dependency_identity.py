"""Version normalization must not hide dependency or compiler input changes."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('identity', Path(__file__).with_name('cargo-dependency-identity.py'))
IDENTITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IDENTITY)


class SemanticIdentityTests(unittest.TestCase):
    def fixture(self, version='1.0.0'):
        root = 'path+file:///app#silo-ui@' + version
        dependency = 'registry+https://github.com/rust-lang/crates.io-index#itoa@1.0.0'
        return [
            {'workspace_members': [root], 'packages': [{'id': root, 'name': 'silo-ui', 'version': version, 'source': None}],
             'resolve': {'root': root, 'nodes': [{'id': root, 'dependencies': [dependency], 'features': []},
                                                {'id': dependency, 'dependencies': [], 'features': ['std']}]}},
            {'version': 4, 'package': [{'name': 'silo-ui', 'version': version, 'dependencies': ['itoa']},
                                      {'name': 'itoa', 'version': '1.0.0', 'source': 'registry', 'checksum': 'a' * 64}]},
            {'package': {'name': 'silo-ui', 'version': version, 'edition': '2021'},
             'dependencies': {'itoa': '1'}, 'profile': {'release': {'lto': False}}},
            {'version': version, 'bundle': {'macOS': {'minimumSystemVersion': '14.0'}}},
            {'rustc': '1.94.0', 'sdk': 'macOS26', 'targetDir': '/stable/target'}]

    def digest(self, inputs):
        return IDENTITY.identity_digest(IDENTITY.semantic_inputs(*inputs))

    def test_app_version_change_hits_without_mutating_inputs(self):
        before = self.fixture(); snapshot = copy.deepcopy(before)
        self.assertEqual(self.digest(before), self.digest(self.fixture('1.0.1')))
        self.assertEqual(before, snapshot)

    def test_dependency_and_compilation_changes_miss(self):
        original = self.fixture(); expected = self.digest(original)
        changes = [
            lambda x: x[1]['package'][1].update(version='1.0.1'),
            lambda x: x[1]['package'][1].update(checksum='b' * 64),
            lambda x: x[0]['resolve']['nodes'][1]['features'].append('new'),
            lambda x: x[0]['resolve']['nodes'][0]['dependencies'].clear(),
            lambda x: x[2]['profile']['release'].update(lto=True),
            lambda x: x[2]['dependencies'].update(itoa='2'),
            lambda x: x[3]['bundle']['macOS'].update(minimumSystemVersion='15.0'),
            lambda x: x[3].update(app={'macOSPrivateApi': True}),
            lambda x: x[4].update(rustc='1.95.0'),
            lambda x: x[4].update(sdk='macOS27'),
        ]
        for index, change in enumerate(changes):
            with self.subTest(index=index):
                changed = copy.deepcopy(original); change(changed)
                self.assertNotEqual(expected, self.digest(changed))

    def test_mismatched_root_manifest_is_rejected(self):
        inputs = self.fixture(); inputs[2]['package']['version'] = '2.0.0'
        with self.assertRaisesRegex(ValueError, 'does not match'):
            self.digest(inputs)

    def test_registry_root_is_rejected(self):
        inputs = self.fixture(); inputs[0]['packages'][0]['source'] = 'registry'
        with self.assertRaisesRegex(ValueError, 'local application root'):
            self.digest(inputs)


if __name__ == '__main__':
    unittest.main()
