"""Workflow action pins and literal handling of reusable shell inputs."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

GITHUB = Path(__file__).resolve().parents[3] / '.github'
USES = re.compile(r'^\s*(?:-\s+)?uses:\s*(\S+)(.*)$', re.M)


class WorkflowPinTests(unittest.TestCase):
    def test_qemu_installer_images_are_pinned_to_digests(self):
        checked = 0
        for path in GITHUB.glob('workflows/*.yml'):
            steps = re.split(r'^\s{6}- ', path.read_text(), flags=re.M)
            for step in steps:
                if not re.search(r'^uses: docker/setup-qemu-action@', step):
                    continue
                checked += 1
                with self.subTest(file=path.name):
                    self.assertRegex(step, r'(?m)^\s+image: docker\.io/tonistiigi/binfmt@sha256:[0-9a-f]{64}\s*$')
        self.assertGreater(checked, 0)

    def test_actions_are_pinned_to_commit_shas_with_version_comments(self):
        files = sorted([*GITHUB.glob('workflows/*.yml'), *GITHUB.glob('actions/*/action.yml')])
        self.assertTrue(files)
        checked = 0
        for path in files:
            for reference, rest in USES.findall(path.read_text()):
                if reference.startswith('./'):
                    continue  # Local actions and reusable workflows come from this checkout.
                checked += 1
                with self.subTest(file=path.name, action=reference):
                    self.assertRegex(reference, r'^[\w.-]+/[\w./-]+@[0-9a-f]{40}$')
                    self.assertRegex(rest, r'^\s+# v\d+(\.\d+)*$')
        self.assertGreater(checked, 0)

    def test_checkout_does_not_persist_repository_credentials(self):
        checked = 0
        for path in GITHUB.glob('workflows/*.yml'):
            for step in re.split(r'^ {6}- ', path.read_text(), flags=re.M):
                if not re.search(r'(?:^|\n +)uses: actions/checkout@', step):
                    continue
                checked += 1
                with self.subTest(file=path.name):
                    self.assertRegex(step, r'(?m)^ {10}persist-credentials: false$')
        self.assertGreater(checked, 0)

    def test_dependabot_updates_workflow_and_composite_actions(self):
        config = (GITHUB / 'dependabot.yml').read_text()
        self.assertIn('package-ecosystem: github-actions', config)
        for directory in ['/'] + [f'/.github/actions/{path.name}' for path in GITHUB.glob('actions/*')]:
            self.assertIn(f"'{directory}'", config)


class WorkflowConcurrencyTests(unittest.TestCase):
    def test_verification_supersedes_only_the_same_pull_request(self):
        for name in ['release-tooling.yml']:
            with self.subTest(workflow=name):
                workflow = (GITHUB / 'workflows' / name).read_text()
                concurrency = re.search(r'(?m)^concurrency:\n((?: +[^\n]*\n)+)', workflow)
                self.assertIsNotNone(concurrency, 'obsolete PR checks run without a concurrency limit')
                config = concurrency[1]
                group = re.search(r'(?m)^ +group: (.+)$', config)[1]
                self.assertIn('github.workflow', group)
                self.assertRegex(group, r"github.event_name == 'pull_request' && github.ref \|\| github.run_id")
                cancel = re.search(r'(?m)^ +cancel-in-progress: (.+)$', config)[1]
                self.assertEqual(cancel, "${{ github.event_name == 'pull_request' }}")


class WorkflowInputTests(unittest.TestCase):
    def test_host_build_ci_targets_macos_only(self):
        ci = (GITHUB / 'workflows/ci.yml').read_text()
        rust = ci.split('\n  rust:', 1)[1]
        release = (GITHUB / 'workflows/release.yml').read_text()
        warm = (GITHUB / 'workflows/warm-release-caches.yml').read_text()
        benchmark = (GITHUB / 'workflows/benchmark-dependency-cache.yml').read_text()
        platform = (GITHUB / 'workflows/release-platform.yml').read_text()
        self.assertIn('runs-on: macos-15', rust)
        self.assertNotIn('unknown-linux-gnu', rust)
        self.assertIn('runner: macos-15', release)
        self.assertNotIn('unknown-linux-gnu', release)
        self.assertNotIn('unknown-linux-gnu', warm)
        self.assertNotIn('unknown-linux-gnu', benchmark)
        self.assertNotIn('Collect and verify Linux assets', platform)
        self.assertFalse((GITHUB / 'workflows/linux-verification.yml').exists())
        self.assertFalse((GITHUB / 'workflows/linux-packaging.yml').exists())

    def test_reusable_target_is_a_literal_command_argument(self):
        cases = [
            ('actions/prepare-release-runtime/action.yml', 'Warm application dependency downloads', 'cargo',
             ['fetch', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--target']),
            ('workflows/release-platform.yml', 'Resolve public dependency identity before test signing configuration', 'python3',
             ['scripts/release-dependency-cache.py', 'prepare', '--target']),
            ('workflows/benchmark-dependency-platform.yml', 'Measure cold production compilation with synthetic configuration', 'python3',
             ['scripts/dependency-cache-benchmark.py', 'build', '--role', 'producer', '--target']),
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            arguments = root / 'arguments.json'
            marker = root / 'injected'
            for filename, name, executable, prefix in cases:
                workflow = (GITHUB / filename).read_text()
                step = re.search(r'(?m)^( +)- name: ' + re.escape(name) + r'\n', workflow)
                self.assertIsNotNone(step, name)
                indent = len(step[1])
                body = re.split(r'(?m)^ {' + str(indent) + r'}- ', workflow[step.end():], maxsplit=1)[0]
                command = re.search(r'(?m)^ +run: (.+)$', body)[1]
                self.assertNotEqual(command, '|')
                fake = root / executable
                fake.write_text(f'#!{sys.executable}\nimport json, os, sys\nfrom pathlib import Path\nPath(os.environ["ARGUMENTS"]).write_text(json.dumps(sys.argv[1:]))\n')
                fake.chmod(0o755)
                if filename.startswith('actions/'):
                    environment = body
                else:
                    job_prefix = re.split(r'(?m)^  [\w-]+:\n', workflow[:step.start()])[-1]
                    environment = job_prefix.split('    steps:', 1)[0]
                binding = re.search(r'(?m)^ +RUNTIME_TARGET: (.+)$', environment)
                for target in ['x86_64-unknown-linux-gnu', f"x'; touch {marker}; #", f'$(touch {marker})']:
                    with self.subTest(file=filename, target=target):
                        marker.unlink(missing_ok=True)
                        # Model Actions expression expansion before Bash parses the script.
                        script = command.replace('${{ inputs.target }}', target)
                        result = subprocess.run(['bash', '-e', '-c', script], cwd=root, env={
                            **os.environ, 'PATH': str(root) + os.pathsep + os.environ['PATH'],
                            'ARGUMENTS': str(arguments),
                            'RUNTIME_TARGET': binding[1].replace('${{ inputs.target }}', target) if binding else '',
                            'RUNNER_TEMP': str(root), 'GITHUB_OUTPUT': str(root / 'output'),
                        }, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertFalse(marker.exists(), 'input executed a shell command')
                        actual = json.loads(arguments.read_text())
                        self.assertEqual(actual[:len(prefix) + 1], [*prefix, target])



if __name__ == '__main__':
    unittest.main()
