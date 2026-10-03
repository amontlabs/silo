"""Protect the release cache boundary using representative filesystem contents."""
from pathlib import Path
import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
ACTION = (ROOT / '.github/actions/prepare-release-runtime/action.yml').read_text()
WORKFLOW = (ROOT / '.github/workflows/release.yml').read_text()
PLATFORM = (ROOT / '.github/workflows/release-platform.yml').read_text()


class ReleaseCacheTests(unittest.TestCase):
    def test_runtime_cache_key_uses_selected_go_version_before_lookup(self):
        self.assertLess(ACTION.index('actions/setup-go@'), ACTION.index('actions/cache/restore@'))
        block = ACTION.split('      id: go-compiler\n', 1)[1].split('    - name:', 1)[0]
        self.assertIn('GOTOOLCHAIN: local', block)
        command = block.split('      run: |\n', 1)[1]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            go = root / 'go'
            go.write_text('#!/bin/sh\ntest "$*" = "env GOVERSION" || exit 1\nprintf "%s\\n" "$TEST_GO_VERSION"\n')
            go.chmod(0o755)
            output = root / 'output'
            for version in ('go1.25.1', 'go1.25.2'):
                output.unlink(missing_ok=True)
                subprocess.run(['/bin/bash', '-eu', '-c', command], check=True, env={
                    **os.environ, 'PATH': str(root), 'GITHUB_OUTPUT': str(output), 'TEST_GO_VERSION': version,
                })
                self.assertEqual(output.read_text(), f"version={version.removeprefix('go')}\n")
        self.assertIn('-go${{ steps.go-compiler.outputs.version }}-', ACTION)
        self.assertIn("'.github/actions/prepare-release-runtime/action.yml'", ACTION)

    def test_jobs_that_prepare_and_build_share_one_go_toolchain_policy(self):
        for name in ('release-platform', 'linux-packaging', 'warm-release-caches', 'benchmark-dependency-platform'):
            text = (ROOT / f'.github/workflows/{name}.yml').read_text()
            jobs = re.split(r'^  [a-z-]+:\n', text.split('\njobs:\n', 1)[1], flags=re.M)
            for job in jobs:
                if 'prepare-release-runtime' not in job:
                    continue
                env = re.search(r'^    env:\n((?:      .*\n|\n)+)', job, re.M)
                self.assertIsNotNone(env, name)
                self.assertIn('      GOTOOLCHAIN: local\n', env.group(1), name)

    def test_ci_installs_the_manifest_toolchain(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / 'app/SiloUI/runtime-inputs.json'
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({'toolchain': '9.8.7'}))
            commands = root / 'bin'
            commands.mkdir()
            (commands / 'node').symlink_to(shutil.which('node'))
            rustup = commands / 'rustup'
            rustup.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$SILO_TOOLCHAIN_LOG"\n')
            rustup.chmod(0o755)
            log = root / 'calls'
            env = dict(os.environ, PATH=str(commands), GITHUB_WORKSPACE=str(root), SILO_TOOLCHAIN_LOG=str(log))
            for name in ['release-platform.yml', 'warm-release-caches.yml', 'linux-verification.yml']:
                workflow = (ROOT / '.github/workflows' / name).read_text()
                blocks = re.findall(r'          runtime_toolchain=.*\n          rustup toolchain install .*\n          rustup default .*', workflow)
                self.assertEqual(len(blocks), 3 if name == 'release-platform.yml' else 1)
                for block in blocks:
                    log.unlink(missing_ok=True)
                    subprocess.run(['/bin/bash', '-eu', '-c', block], env=env, check=True, capture_output=True)
                    self.assertEqual(log.read_text().splitlines(), ['toolchain install 9.8.7 --profile minimal', 'default 9.8.7'])
            self.assertIn('-rust${{ steps.runtime-inputs.outputs.toolchain }}-', ACTION)
            output = root / 'github-output'
            env['GITHUB_OUTPUT'] = str(output)
            preflight_block = re.search(r'      run: \|\n((?:        .+\n)+)', ACTION).group(1)
            subprocess.run(['/bin/bash', '-eu', '-c', preflight_block], cwd=ROOT, env=env, check=True, capture_output=True)
            approved = json.loads((ROOT / 'app/SiloUI/runtime-inputs.json').read_text())
            self.assertEqual(output.read_text(), f"toolchain={approved['toolchain']}\n")

    def test_cache_allowlist_excludes_compiled_application_and_build_work(self):
        blocks = re.findall(r'        path: \|\n((?:          .+\n)+)', ACTION)
        paths = [block.split() for block in blocks]
        self.assertEqual(len(paths), 4)
        self.assertEqual(paths[0], paths[2], 'Runtime restore/save paths must match')
        self.assertEqual(paths[1], paths[3], 'Cargo restore/save paths must match')
        runtime = 'app/SiloUI/src-tauri/target/runtime-cache/'
        allowed = [runtime + 'v0.7.6/patched-builds/key/msb',
                   runtime + 'v0.7.6/patched-builds/key/msb.sha256',
                   runtime + 'v0.7.6/microsandbox-09df3d4b9d832adaede1fb9a198cfc660bfab8cd.tar.gz',
                   runtime + 'v0.7.6/agentd-aarch64',
                   runtime + 'v0.7.6/libkrunfw-darwin-aarch64.dylib',
                   runtime + 'v0.7.6/licenses/microsandbox-Apache-2.0.txt',
                   runtime + 'dugite/version/archive.tar.gz',
                   runtime + 'git-lfs-transfer/pin/source.tar.gz',
                   runtime + 'git-lfs-transfer/pin/builds/linux-arm64/git-lfs-transfer',
                   '.cargo/registry/cache/index/crate.tar.gz']
        forbidden = [runtime + 'v0.7.6/patched-builds/key/cargo-target/release/msb',
                     runtime + 'v0.7.6/patched-builds/key/work/source.rs',
                     runtime + 'git-lfs-transfer/pin/source-123/main.go',
                     'app/SiloUI/src-tauri/target/release/silo-ui',
                     'app/SiloUI/src-tauri/target/debug/build/silo/output',
                     'app/SiloUI/github-build.local.json',
                     '.cargo/credentials.toml', '.cargo/config.toml']
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in allowed + forbidden:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('fixture')
            selected = set()
            for pattern in paths[0] + paths[1]:
                for match in root.glob(pattern.removeprefix('~/')):
                    selected.update([match] if match.is_file() else match.rglob('*'))
            for name in allowed:
                self.assertIn(root / name, selected)
            for name in forbidden:
                self.assertNotIn(root / name, selected)

    def test_preflight_precedes_runtime_downloads(self):
        self.assertLess(ACTION.index('node app/SiloUI/scripts/preflight.mjs'), ACTION.index('actions/cache/restore'))
        self.assertIn("'app/SiloUI/runtime-inputs.json'", ACTION)
        self.assertIn('restore-keys: silo-runtime-v1-', ACTION)
        validate = WORKFLOW.split('\n  validate:', 1)[1].split('\n  macos-minimum-constraints:', 1)[0]
        self.assertIn('node app/SiloUI/scripts/preflight.mjs', validate)

    def test_shared_frontend_checks_gate_publication(self):
        frontend = WORKFLOW.split('\n  frontend:', 1)[1].split('\n  platforms:', 1)[0]
        build = PLATFORM.split('\n  build:', 1)[1]
        for command in ['npm test --', 'npm run lint', 'npm run test:release']:
            self.assertIn(command, frontend)
            self.assertNotIn(command, build)
        self.assertIn('npm run typecheck', frontend)
        self.assertIn('needs: [validate, frontend, platforms, macos-minimum-constraints]', WORKFLOW)
        native = PLATFORM.split('\n  native-tests:', 1)[1].split('\n  build:', 1)[0]
        self.assertIn('cargo test --manifest-path', native)
        self.assertIn('Test updater transport and interrupted installation', native)
        self.assertNotIn('secrets.', native)
        self.assertNotIn('TAURI_SIGNING_PRIVATE_KEY', native)
        self.assertIn("inputs.benchmark-schedule != 'sequential'", native)
        self.assertIn("if: inputs.benchmark-schedule == 'sequential'", build)
        self.assertIn('path: ${{ runner.temp }}/build-phases.jsonl', native)
        self.assertNotIn('path: app/SiloUI/src-tauri/target', native)
        self.assertIn("test_*release*.py", build)
        self.assertIn('Test updater transport and interrupted installation', build)


if __name__ == '__main__':
    unittest.main()
