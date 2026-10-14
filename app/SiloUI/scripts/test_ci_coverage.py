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
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        selected = set()
        for command in re.findall(r'(?m)^ +(?:- )?run: (cargo test .+)$', workflow):
            arguments = shlex.split(command)
            self.assertIn('--locked', arguments)
            if '-p' in arguments:
                selected.add(arguments[arguments.index('-p') + 1])
            else:
                selected.add(package_manifest['package']['name'])
        self.assertIn(package_manifest['package']['name'], selected)
        for name, patch in manifest['patch']['crates-io'].items():
            directory = APP / patch['path']
            if any('#[test]' in source.read_text() for source in directory.rglob('*.rs')):
                with self.subTest(package=name):
                    self.assertIn(name, selected, 'dependency unit tests are not run by root cargo test')


class PlatformCoverageTests(unittest.TestCase):
    def test_ci_runs_host_rust_and_signing_suites_on_macos(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        rust = re.search(r'(?ms)^  rust:\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow)[1]
        self.assertIn('runs-on: macos-15', rust)
        self.assertNotIn('matrix.runner', rust)
        signing = [step for step in re.split(r'(?m)^ {6}- ', rust) if "-p 'test_macos_release.py'" in step]
        self.assertEqual(len(signing), 1, 'the macOS signing suite must execute on macOS')
        self.assertNotIn("if: runner.os == 'macOS'", signing[0])


class LinuxHostCoverageTests(unittest.TestCase):
    def test_ci_does_not_build_or_install_linux_host_packages(self):
        for name in ('ci.yml', 'release-tooling.yml'):
            workflow = (ROOT / '.github/workflows' / name).read_text()
            self.assertNotIn('SILO_APT_LIFECYCLE_TEST=1', workflow, name)
            self.assertNotIn('test_debian_installation.py', workflow, name)


class DocumentationCoverageTests(unittest.TestCase):
    def test_ci_link_check_includes_tracked_first_party_markdown(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        step = workflow.split('      - name: Check relative links in documentation\n', 1)[1].split('\n  rust:', 1)[0]
        command = ' '.join(step.split('        run: >-\n', 1)[1].splitlines())
        covered = set()
        for pattern in shlex.split(command):
            if pattern.endswith('.md'):
                covered.update(str(path.relative_to(ROOT)) for path in ROOT.glob(pattern) if path.is_file())
        tracked = subprocess.check_output(['git', 'ls-files', '*.md'], cwd=ROOT, text=True).splitlines()
        # The vendor tree contains upstream documentation for an intentionally partial crate copy.
        expected = {name for name in tracked if '/vendor/' not in name}
        missing = sorted(expected - covered)
        self.assertEqual(missing, [], f'{len(missing)} Markdown files omitted: {missing[:12]}')


if __name__ == '__main__':
    unittest.main()
