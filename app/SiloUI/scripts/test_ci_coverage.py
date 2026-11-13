"""CI test selection must include new ordinary suites and platform-specific tests."""
import json
import os
import re
import shlex
import tomllib
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / 'app/SiloUI'
CI = APP / 'scripts/ci'


def ci_commands():
    """The text of CI's test scripts, with the shared nextest flags written out."""
    flags = re.search(r'(?m)^SILO_NEXTEST_FLAGS=\((.*)\)$', (CI / 'common.sh').read_text())[1]
    return '\n'.join(path.read_text().replace('"${SILO_NEXTEST_FLAGS[@]}"', flags)
                     for path in sorted(CI.glob('*.sh')))


class ReleaseDiscoveryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        package = json.loads((APP / 'package.json').read_text())
        self.command = package['scripts']['test:release']
        (self.root / 'package.json').write_text(json.dumps({'scripts': {'test:release': self.command}}))
        # Existing explicit entries must remain valid so the regression reaches discovery.
        for source in (APP / 'scripts').glob('*.test.mjs'):
            path = self.root / 'scripts' / source.name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("import test from 'node:test'; test('existing fixture', () => {});\n")
        for filename in ['new-ci-suite.test.mjs', 'nested/new-ci-suite.test.mjs']:
            path = self.root / 'scripts' / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("import test from 'node:test'; import {writeFileSync} from 'node:fs';\n"
                            "test('new CI suite', () => {writeFileSync(new URL('./picked-up', import.meta.url), 'yes');"
                            "if (process.env.FAIL_DISCOVERY) throw new Error('new suite failed');});\n")

    def run_script(self, *, fail=False):
        environment = dict(os.environ)
        environment.pop('FAIL_DISCOVERY', None)
        if fail:
            environment['FAIL_DISCOVERY'] = '1'
        return subprocess.run(['npm', 'run', '--silent', 'test:release'], cwd=self.root,
                              env=environment, capture_output=True, text=True)

    def test_new_release_suites_are_discovered(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for marker in ['scripts/picked-up', 'scripts/nested/picked-up']:
            self.assertTrue((self.root / marker).exists(), marker)

    def test_new_release_suite_failure_fails_the_command(self):
        result = self.run_script(fail=True)
        self.assertNotEqual(result.returncode, 0, 'the newly added failing suites were ignored')
        self.assertIn('new suite failed', result.stdout + result.stderr)


class CargoCoverageTests(unittest.TestCase):
    def test_ci_selects_local_patched_packages_with_unit_tests(self):
        manifest = tomllib.loads((APP / 'Cargo.toml').read_text())
        package_manifest = tomllib.loads((APP / 'src-tauri/Cargo.toml').read_text())
        selected = set()
        for command in re.findall(r'(?m)^(?:step \S+ )?(cargo (?:test|nextest run) .+)$', ci_commands()):
            arguments = shlex.split(command)
            self.assertIn('--locked', arguments)
            if '-p' in arguments:
                selected.add(arguments[arguments.index('-p') + 1])
            else:
                selected.add(package_manifest['package']['name'])
        self.assertIn(package_manifest['package']['name'], selected)
        for name, patch in manifest['patch']['crates-io'].items():
            # A vendored copy without its dev-dependencies cannot build its upstream unit tests (cargo
            # refuses `-p` for a non-member that needs them); russh's patch is tested through
            # silo-engine's SSH server tests (SILO-PATCH.md).
            if name == 'russh':
                continue
            directory = APP / patch['path']
            if any('#[test]' in source.read_text() for source in directory.rglob('*.rs')):
                with self.subTest(package=name):
                    self.assertIn(name, selected, 'dependency unit tests are not run by root cargo test')


class NextestCoverageTests(unittest.TestCase):
    def test_nextest_profile_exists_and_no_doctests_are_skipped(self):
        config = tomllib.loads((APP / '.config/nextest.toml').read_text())
        profiles = re.findall(r'cargo nextest run .*--profile (\S+)', ci_commands())
        self.assertTrue(profiles)
        for profile in profiles:
            self.assertIn(profile, config['profile'])
        # nextest does not run doctests; add a `cargo test --doc` step before adding one.
        tracked = subprocess.check_output(['git', 'ls-files', 'app/SiloUI/*.rs'], cwd=ROOT, text=True).splitlines()
        for name in tracked:
            if '/vendor/' in name:
                continue
            with self.subTest(file=name):
                self.assertNotRegex((ROOT / name).read_text(errors='replace'), r'(?m)^\s*//[/!]\s*```')


class PlatformCoverageTests(unittest.TestCase):
    def test_ci_runs_host_rust_and_signing_suites_on_macos(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        rust = re.search(r'(?ms)^  rust:\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow)[1]
        self.assertIn('runs-on: macos-15', rust)
        self.assertNotIn('matrix.runner', rust)
        signing = [step for step in re.split(r'(?m)^ {6}- ', rust) if 'run: scripts/ci/macos.sh' in step]
        self.assertEqual(len(signing), 1, 'the macOS signing suite must execute on macOS')
        self.assertNotIn("if: runner.os == 'macOS'", signing[0])
        self.assertRegex((CI / 'macos.sh').read_text(), r"(?m)^step \S+ python3 -m unittest discover -s scripts -p 'test_macos_release.py'$")


class DocumentationCoverageTests(unittest.TestCase):
    def test_ci_link_check_includes_tracked_first_party_markdown(self):
        script = (CI / 'links.sh').read_text()
        patterns = re.search(r'(?ms)^patterns=\(\n(.*?)^\)$', script)[1]
        covered = set()
        for pattern in shlex.split(patterns):
            if pattern.endswith('.md'):
                covered.update(str(path.relative_to(ROOT)) for path in ROOT.glob(pattern) if path.is_file())
        tracked = subprocess.check_output(['git', 'ls-files', '*.md'], cwd=ROOT, text=True).splitlines()
        # The vendor tree contains upstream documentation for an intentionally partial crate copy.
        expected = {name for name in tracked if '/vendor/' not in name}
        missing = sorted(expected - covered)
        self.assertEqual(missing, [], f'{len(missing)} Markdown files omitted: {missing[:12]}')


if __name__ == '__main__':
    unittest.main()
