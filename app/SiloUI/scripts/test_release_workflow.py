"""Run the workflow's early release gate without building or contacting GitHub."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = (ROOT / '.github/workflows/release.yml').read_text()
GATE_STEP = WORKFLOW.split('      - name: Validate release inputs before building\n', 1)[1]
GATE = textwrap.dedent(GATE_STEP.split('        run: |\n', 1)[1].split('\n  macos-minimum-constraints:', 1)[0])


class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.app = self.root / 'app/SiloUI'
        (self.app / 'scripts').mkdir(parents=True)
        (self.app / 'src-tauri').mkdir()
        (self.app / '.changeset').mkdir()
        (self.app / '.changeset/README.md').write_text('Changeset instructions')
        shutil.copyfile(ROOT / 'app/SiloUI/scripts/validate-release-version.py', self.app / 'scripts/validate-release-version.py')
        (self.app / 'package.json').write_text(json.dumps({'version': '0.2.3'}))
        (self.app / 'package-lock.json').write_text(json.dumps({'version': '0.2.3', 'packages': {'': {'version': '0.2.3'}}}))
        (self.app / 'src-tauri/Cargo.toml').write_text('[package]\nversion = "0.2.3"\n')
        (self.app / 'Cargo.lock').write_text('[[package]]\nname = "silo-ui"\nversion = "0.2.3"\n')
        (self.root / 'docs/releases').mkdir(parents=True)
        self.notes = self.root / 'docs/releases/0.2.3.md'
        self.notes.write_text('User-facing release notes')
        self.output = self.root / 'output'

    def run_gate(self, *, draft=True, ref='refs/tags/v0.2.3', schedule='parallel', allow_stable=False):
        return subprocess.run(['bash', '-c', GATE], cwd=self.root,
                              env={**os.environ, 'CREATE_DRAFT': str(draft).lower(),
                                   'RELEASE_REF': ref, 'BENCHMARK_SCHEDULE': schedule,
                                   'ALLOW_STABLE': str(allow_stable).lower(), 'GITHUB_OUTPUT': str(self.output)},
                              capture_output=True, text=True)

    def test_sequential_benchmark_cannot_publish(self):
        result = self.run_gate(schedule='sequential')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('artifact-only', result.stdout)
        self.assertFalse(self.output.exists())
        result = self.run_gate(draft=False, ref='refs/heads/benchmark', schedule='sequential')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_text(), 'draft=false\n')

    def test_prepared_tag_produces_draft(self):
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_text(), 'draft=true\n')

    def test_stable_draft_requires_opt_in_before_platform_builds(self):
        for path in [self.app / 'package.json', self.app / 'package-lock.json',
                     self.app / 'src-tauri/Cargo.toml', self.app / 'Cargo.lock']:
            path.write_text(path.read_text().replace('0.2.3', '1.0.0'))
        self.notes.rename(self.notes.with_name('1.0.0.md'))
        result = self.run_gate(ref='refs/tags/v1.0.0')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('below 1.0.0', result.stdout)
        self.assertFalse(self.output.exists())
        result = self.run_gate(ref='refs/tags/v1.0.0', allow_stable=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_text(), 'draft=true\n')

    def test_branch_and_mismatched_tags_fail_before_build(self):
        for ref in ['refs/heads/main', 'refs/tags/v0.2.4', 'refs/tags/v0.2.3-rc.1']:
            with self.subTest(ref=ref):
                result = self.run_gate(ref=ref)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Release ref must be', result.stdout)

    def test_missing_and_empty_release_notes_block_draft(self):
        for contents in [None, '']:
            with self.subTest(contents=contents):
                if contents is None:
                    self.notes.unlink()
                else:
                    self.notes.write_text(contents)
                result = self.run_gate()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Missing release notes', result.stdout)

    def test_pending_changesets_block_draft(self):
        (self.app / '.changeset/new-feature.md').write_text('Pending note')
        result = self.run_gate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Pending changeset', result.stdout)

    def test_version_mismatch_blocks_even_verification(self):
        (self.app / 'src-tauri/Cargo.toml').write_text('[package]\nversion = "0.2.2"\n')
        result = self.run_gate(draft=False, ref='refs/heads/main')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('versions must agree', result.stderr)

    def test_manual_verification_allows_unreleased_work(self):
        self.notes.unlink()
        (self.app / '.changeset/new-feature.md').write_text('Pending note')
        result = self.run_gate(draft=False, ref='refs/heads/main')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_text(), 'draft=false\n')


class PublicationWorkflowTests(unittest.TestCase):
    def test_distinct_publication_requests_are_queued_without_replacement(self):
        publication = (ROOT / '.github/workflows/publish-release.yml').read_text()
        concurrency = publication.split('\nconcurrency:\n', 1)[1].split('\npermissions:', 1)[0]
        self.assertIn('  group: silo-release-publish', concurrency.splitlines())
        self.assertIn('  queue: max', concurrency.splitlines())
        self.assertIn('  cancel-in-progress: false', concurrency.splitlines())

    def test_publication_passes_stable_opt_in_only_when_requested(self):
        publication = (ROOT / '.github/workflows/publish-release.yml').read_text()
        draft = WORKFLOW.split('      - name: Create complete versioned draft\n', 1)[1]
        commands = [(publication, ['--publish']), (draft, [])]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / 'arguments.json'
            python = root / 'python3'
            python.write_text(f'#!{sys.executable}\nimport json, os, sys\nfrom pathlib import Path\nPath(os.environ["ARGUMENTS"]).write_text(json.dumps(sys.argv[1:]))\n')
            python.chmod(0o755)
            for step, action in commands:
                command = textwrap.dedent(step.split('        run: |\n', 1)[1])
                for allowed in ('false', 'true'):
                    with self.subTest(action=action, allowed=allowed):
                        result = subprocess.run(['bash', '-c', command], env={
                            **os.environ, 'PATH': str(root) + os.pathsep + os.environ['PATH'],
                            'RELEASE_VERSION': '1.0.0', 'RELEASE_REF': 'refs/tags/v1.0.0',
                            'ALLOW_STABLE': allowed, 'ARGUMENTS': str(output),
                        }, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        expected = ['app/SiloUI/scripts/publish-release.py', '1.0.0', *action]
                        if allowed == 'true':
                            expected.append('--allow-stable')
                        self.assertEqual(json.loads(output.read_text()), expected)


if __name__ == '__main__':
    unittest.main()
