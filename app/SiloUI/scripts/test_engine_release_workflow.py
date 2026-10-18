"""The engine release workflow publishes only a tag that names the silod version, without widening permissions."""
import os
from pathlib import Path
import re
import subprocess
import textwrap
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = (ROOT / '.github/workflows/engine-release.yml').read_text()
VERSION = tomllib.loads((ROOT / 'app/SiloUI/src-tauri/silod/Cargo.toml').read_text())['package']['version']


def job(name):
    return re.search(rf'(?ms)^  {name}:\n(.*?)(?=^  [\w-]+:\n|\Z)', WORKFLOW)[1]


def step(body, name):
    return re.search(rf'(?ms)^      - name: {re.escape(name)}\n(.*?)(?=^      - |\Z)', body)[1]


class EngineReleaseWorkflowTests(unittest.TestCase):
    def test_only_engine_tags_trigger_it(self):
        trigger = WORKFLOW.split('\njobs:', 1)[0]
        self.assertRegex(trigger, r"(?m)^on:\n  push:\n    tags: \['engine-v\*'\]\n\npermissions:")
        self.assertNotIn('branches', trigger)
        self.assertNotIn('pull_request', trigger)

    def test_write_access_belongs_to_the_publishing_job_alone(self):
        self.assertRegex(WORKFLOW, r'(?m)^permissions:\n  contents: read\n')
        self.assertEqual(WORKFLOW.count('contents: write'), 1)
        self.assertIn('contents: write', job('publish'))
        self.assertNotIn('permissions:', job('build'))
        self.assertNotIn('secrets.GITHUB_TOKEN', job('build'))
        # The publishing job never runs repository code.
        self.assertNotIn('actions/checkout', job('publish'))
        self.assertNotIn('cargo', job('publish'))

    def test_checkout_does_not_persist_credentials_and_builds_on_macos_14(self):
        build = job('build')
        self.assertIn('runs-on: macos-14', build)
        self.assertRegex(build, r'(?s)uses: actions/checkout@[0-9a-f]{40} # v[\d.]+\n        with:\n          persist-credentials: false')

    def test_github_app_configuration_reaches_only_the_engine_build_step(self):
        for name in ('SILO_GITHUB_APP_SLUG', 'SILO_GITHUB_CLIENT_ID', 'SILO_GITHUB_CLIENT_SECRET'):
            self.assertEqual(WORKFLOW.count(f'secrets.{name}'), 1, name)
            self.assertIn(f'secrets.{name}', step(job('build'), 'Build and package the universal engine'))

    def test_the_pinned_toolchain_and_both_targets_are_installed(self):
        installation = step(job('build'), 'Install pinned Rust toolchain with both macOS targets')
        self.assertIn('runtime-inputs.json', installation)
        for target in ('aarch64-apple-darwin', 'x86_64-apple-darwin'):
            self.assertIn(f'--target {target}', installation)
        self.assertIn('package-engine.py --universal', WORKFLOW)

    def test_the_release_does_not_displace_the_apps_latest_release(self):
        publish = step(job('publish'), 'Create the engine release')
        self.assertIn('--latest=false', publish)
        self.assertIn('--verify-tag', publish)
        self.assertIn('"$ARCHIVE" "$ARCHIVE.sha256"', publish)
        self.assertIn('sha256sum -c', publish)

    def test_the_verification_requires_both_architectures_and_the_checksum(self):
        verification = step(job('build'), 'Verify the archive')
        for needle in ('lipo -archs', 'arm64', 'x86_64', 'shasum -a 256 -c', 'macos-universal.tar.gz'):
            self.assertIn(needle, verification)

    def test_the_tag_must_equal_the_silod_version(self):
        script = textwrap.dedent(step(job('build'), 'Require the tag to name the engine version').split('        run: |\n', 1)[1])
        for tag, accepted in ((f'engine-v{VERSION}', True), ('engine-v9.9.9', False), (f'v{VERSION}', False),
                               (f'engine-v{VERSION}-rc1', False)):
            with self.subTest(tag=tag):
                result = subprocess.run(['bash', '-c', script], cwd=ROOT, env={**os.environ, 'TAG': tag},
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, accepted, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
