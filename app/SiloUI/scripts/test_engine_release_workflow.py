"""The engine release workflow publishes only the silod version, from an engine tag or main, without widening permissions."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
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
    def test_only_engine_tags_and_manual_runs_trigger_it(self):
        trigger = WORKFLOW.split('\njobs:', 1)[0]
        self.assertRegex(trigger, r"(?m)^on:\n  push:\n    tags: \['engine-v\*'\]\n  workflow_dispatch:\n\npermissions:")
        self.assertNotIn('branches', trigger)
        self.assertNotIn('pull_request', trigger)

    def test_write_access_belongs_to_the_publishing_job_alone(self):
        self.assertRegex(WORKFLOW, r'(?m)^permissions:\n  contents: read\n')
        self.assertEqual(WORKFLOW.count('contents: write'), 1)
        self.assertIn('contents: write', job('publish'))
        for name in ('tag', 'build', 'build-linux'):
            self.assertNotIn('permissions:', job(name))
            self.assertNotIn('secrets.GITHUB_TOKEN', job(name))
        # The publishing job never runs repository code.
        self.assertNotIn('actions/checkout', job('publish'))
        self.assertNotIn('cargo', job('publish'))

    def test_checkout_does_not_persist_credentials_and_builds_on_macos_15(self):
        build = job('build')
        self.assertIn('runs-on: macos-15', build)
        self.assertRegex(build, r'(?s)uses: actions/checkout@[0-9a-f]{40} # v[\d.]+\n        with:\n          persist-credentials: false')

    def test_github_app_configuration_reaches_only_the_engine_build_steps(self):
        for name in ('SILO_GITHUB_APP_SLUG', 'SILO_GITHUB_CLIENT_ID', 'SILO_GITHUB_CLIENT_SECRET'):
            self.assertEqual(WORKFLOW.count(f'secrets.{name}'), 2, name)
            self.assertIn(f'secrets.{name}', step(job('build'), 'Build and package the engine'))
            self.assertIn(f'secrets.{name}', step(job('build-linux'), 'Build and package the Linux engine'))

    def test_the_pinned_toolchain_and_only_the_apple_silicon_target_are_installed(self):
        installation = step(job('build'), 'Install pinned Rust toolchain for Apple Silicon')
        self.assertIn('runtime-inputs.json', installation)
        self.assertIn('--target aarch64-apple-darwin', installation)
        # The macOS engine is Apple Silicon only; x86-64 is a Linux host platform.
        for forbidden in ('x86_64', 'universal', '--universal'):
            self.assertNotIn(forbidden, job('build'))
            self.assertNotIn(forbidden, job('publish'))
        self.assertIn('package-engine.py --out-dir', WORKFLOW)

    def test_the_tag_is_checked_once_before_every_build(self):
        self.assertEqual(WORKFLOW.count('engine-v$version'), 1)
        self.assertIn('engine-v$version', step(job('tag'), 'Determine the engine tag'))
        self.assertIn('tag: ${{ steps.tag.outputs.tag }}', job('tag'))
        self.assertRegex(job('tag'), r'(?s)uses: actions/checkout@[0-9a-f]{40} # v[\d.]+\n        with:\n          persist-credentials: false')
        for name in ('build', 'build-linux'):
            self.assertIn('    needs: tag\n', job(name).split('steps:', 1)[0], name)
            self.assertIn('TAG: ${{ needs.tag.outputs.tag }}', job(name), name)
        self.assertIn('TAG: ${{ needs.tag.outputs.tag }}', job('publish'))
        self.assertNotIn('github.ref_name', job('publish'))
        self.assertNotIn('needs.build.outputs.tag', WORKFLOW)

    def test_the_release_does_not_displace_the_apps_latest_release(self):
        publish = step(job('publish'), 'Create the engine release')
        self.assertIn('--latest=false', publish)
        self.assertIn('"$ARCHIVE" "$ARCHIVE.sha256"', publish)
        self.assertIn('sha256sum -c', publish)

    def test_a_tag_push_verifies_the_tag_and_a_dispatch_creates_it_at_the_built_commit(self):
        publish = step(job('publish'), 'Create the engine release')
        self.assertRegex(publish, r'(?s)== workflow_dispatch \]\]; then\n\s+tag_flag=\(--target "\$GITHUB_SHA"\)\n\s+else\n\s+tag_flag=\(--verify-tag\)\n\s+fi')
        self.assertEqual(publish.count('--verify-tag'), 1)
        self.assertEqual(publish.count('--target'), 1)
        self.assertIn('"${tag_flag[@]}"', publish)

    def test_the_verification_requires_arm64_and_the_checksum(self):
        verification = step(job('build'), 'Verify the archive')
        for needle in ('lipo -archs', '"$archs" == arm64', 'shasum -a 256 -c', 'macos-arm64.tar.gz'):
            self.assertIn(needle, verification)
        self.assertIn("['aarch64-apple-darwin']", verification)

    def test_linux_engines_build_natively_for_both_architectures(self):
        linux = job('build-linux')
        # It waits for the tag check only, not for the macOS build.
        self.assertIn('    needs: tag\n', linux.split('steps:', 1)[0])
        self.assertIn('runs-on: ${{ matrix.runner }}', linux)
        matrix = re.findall(r'- runner: (\S+)\n\s+platform: (\S+)\n\s+target: (\S+)\n\s+machine: (\S+)', linux)
        self.assertEqual(matrix, [
            ('ubuntu-24.04', 'linux-x64', 'x86_64-unknown-linux-gnu', 'X86-64'),
            ('ubuntu-24.04-arm', 'linux-arm64', 'aarch64-unknown-linux-gnu', 'AArch64'),
        ])
        self.assertRegex(linux, r'(?s)uses: actions/checkout@[0-9a-f]{40} # v[\d.]+\n        with:\n          persist-credentials: false')
        for dependency in ('libclang-dev', 'libcap-ng-dev', 'libdbus-1-dev'):
            self.assertIn(dependency, step(linux, 'Install Linux build dependencies'))
        # Nothing links OpenSSL: reqwest uses rustls, and Cargo.lock has no openssl-sys.
        self.assertNotIn('libssl-dev', WORKFLOW)
        self.assertNotIn('name = "openssl-sys"', (ROOT / 'app/SiloUI/Cargo.lock').read_text())
        self.assertIn('runtime-inputs.json', step(linux, 'Install pinned Rust toolchain'))
        runtime = step(linux, 'Prepare the runtime')
        self.assertIn('uses: ./.github/actions/prepare-release-runtime', runtime)
        self.assertNotIn('save:', runtime)
        self.assertIn('package-engine.py --platform "$PLATFORM" --out-dir', step(linux, 'Build and package the Linux engine'))

    def test_the_linux_verification_checks_architecture_channel_and_checksum(self):
        verification = step(job('build-linux'), 'Verify the Linux archive')
        for needle in ('sha256sum -c', 'silo-engine-$version-$PLATFORM.tar.gz', 'readelf -h', 'Machine:.*$MACHINE',
                       '--print-channel-identifier)" = org.silo.preview', "info['targets'] == [target]",
                       "manifest['channelIdentifier'] == 'org.silo.preview'"):
            self.assertIn(needle, verification)

    def test_the_release_carries_every_archive_and_checksum(self):
        publish = job('publish')
        # Every build, both Linux architectures included, must succeed before anything is published.
        self.assertIn('needs: [tag, build, build-linux]', publish)
        self.assertIn('fail-fast: true', job('build-linux'))
        self.assertNotIn('continue-on-error', WORKFLOW)
        self.assertNotIn('if:', publish.split('steps:', 1)[0])
        self.assertIn('pattern: silo-engine*\n          merge-multiple: true', publish)
        create = step(publish, 'Create the engine release')
        self.assertIn('linux=("silo-engine-$version-linux-x64.tar.gz" "silo-engine-$version-linux-arm64.tar.gz")', create)
        self.assertIn('test "$(ls | wc -l)" -eq 6', create)
        self.assertIn('"${linux[0]}" "${linux[0]}.sha256" "${linux[1]}" "${linux[1]}.sha256"', create)

    def run_tag_step(self, event, ref, existing=False):
        script = textwrap.dedent(step(job('tag'), 'Determine the engine tag').split('        run: |\n', 1)[1])
        with tempfile.TemporaryDirectory() as temporary:
            bin_dir, output = Path(temporary, 'bin'), Path(temporary, 'output')
            bin_dir.mkdir()
            fake = bin_dir / 'gh'
            fake.write_text(f'#!/bin/sh\nexit {0 if existing else 1}\n')
            fake.chmod(0o755)
            env = {**os.environ, 'PATH': f'{bin_dir}:{os.environ["PATH"]}', 'EVENT': event, 'REF': ref,
                   'REF_NAME': ref.rsplit('/', 1)[-1], 'GH_REPO': 'o/r', 'GITHUB_OUTPUT': str(output)}
            result = subprocess.run(['bash', '-c', script], cwd=ROOT, env=env, capture_output=True, text=True)
            return result, output.read_text() if output.exists() else ''

    def test_a_pushed_tag_must_equal_the_silod_version(self):
        for tag, accepted in ((f'engine-v{VERSION}', True), ('engine-v9.9.9', False), (f'v{VERSION}', False),
                              (f'engine-v{VERSION}-rc1', False)):
            with self.subTest(tag=tag):
                result, output = self.run_tag_step('push', f'refs/tags/{tag}')
                self.assertEqual(result.returncode == 0, accepted, result.stdout + result.stderr)
                self.assertEqual(output, f'tag=engine-v{VERSION}\n' if accepted else '')

    def test_a_manual_run_needs_main_and_a_tag_that_does_not_exist(self):
        result, output = self.run_tag_step('workflow_dispatch', 'refs/heads/main')
        self.assertEqual((result.returncode, output), (0, f'tag=engine-v{VERSION}\n'), result.stderr)
        for ref in ('refs/heads/feature', 'refs/tags/engine-v9.9.9'):
            with self.subTest(ref=ref):
                result, output = self.run_tag_step('workflow_dispatch', ref)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('main', result.stdout)
        result, output = self.run_tag_step('workflow_dispatch', 'refs/heads/main', existing=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('already exists', result.stdout)
        self.assertEqual(output, '')


if __name__ == '__main__':
    unittest.main()
