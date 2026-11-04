"""Native CI work may be skipped only for paths that no Rust code in its scope reads."""
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import unittest

SCRIPT = Path(__file__).with_name('ci-native-changes.py')
ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / 'app/SiloUI'
spec = importlib.util.spec_from_file_location('ci_native_changes', SCRIPT)
changes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(changes)

ENGINE_CRATES = ('src-tauri/engine', 'api', 'host')
LITERAL = re.compile(r'"((?:\.\./|\./)?[\w.@-]+(?:/[\w.@-]+)+/?)"')


def tracked(*patterns):
    return subprocess.check_output(['git', 'ls-files', *patterns], cwd=ROOT, text=True).splitlines()


def crate_dir(path):
    for parent in path.parents:
        if (parent / 'Cargo.toml').exists():
            return parent
    return path.parent


def referenced_paths(name):
    """Existing repository paths that a string literal in a file names, read from the file or its crate."""
    path = ROOT / name
    found = set()
    for literal in LITERAL.findall(path.read_text(errors='replace')):
        for base in {path.parent, crate_dir(path)}:
            target = Path(os.path.normpath(base / literal))
            if target.exists() and target.is_relative_to(ROOT):
                relative = target.relative_to(ROOT).as_posix()
                found.add(relative + ('/' if target.is_dir() else ''))
    return found


class ClassificationTests(unittest.TestCase):
    def test_documentation_website_and_demo_only_changes_skip_native(self):
        for paths in (['README.md'], ['website/package.json', 'website/package-lock.json'],
                      ['demo/src/film.ts', 'docs/SiloUI-APP.md', 'assets/silo-logo.svg', 'SECURITY.md'],
                      ['artifacts/silo-logo-system.html'], ['app/SiloUI/src/App.tsx', 'README.md'],
                      ['app/SiloUI/src/features/x/view.tsx']):
            with self.subTest(paths=paths):
                self.assertFalse(changes.needs_native(paths))
                self.assertFalse(changes.needs_engine(paths))

    def test_any_other_change_runs_native_jobs(self):
        for paths in (['app/SiloUI/src-tauri/src/main.rs'], ['README.md', 'app/SiloUI/package.json'],
                      ['app/SiloUI/README.md'], ['.github/workflows/ci.yml'], ['LICENSE'],
                      ['app/SiloUI/src/test/contracts/backup-state.json'],
                      ['app/SiloUI/src/features/application/model/reserved-secret-names.json'],
                      ['app/SiloUI/srcx/a.ts'], ['docsy/file.md'], ['websites/x']):
            with self.subTest(paths=paths):
                self.assertTrue(changes.needs_native(paths))

    def test_engine_runs_unless_every_path_is_outside_its_crates(self):
        for paths in (['app/SiloUI/src-tauri/src/main.rs'], ['app/SiloUI/src-tauri/silod/src/lib.rs'],
                      ['app/SiloUI/client/src/lib.rs', 'app/SiloUI/src/App.tsx']):
            with self.subTest(paths=paths):
                self.assertTrue(changes.needs_native(paths))
                self.assertFalse(changes.needs_engine(paths))
        for paths in (['app/SiloUI/src-tauri/engine/src/lib.rs'], ['app/SiloUI/api/src/lib.rs'],
                      ['app/SiloUI/host/src/lib.rs'], ['app/SiloUI/src-tauri/guest/verify-tools.sh'],
                      ['app/SiloUI/Cargo.lock'], ['app/SiloUI/src-tauri/tauri.conf.json'],
                      ['app/SiloUI/src-tauri/src/main.rs', 'app/SiloUI/runtime-inputs.json'],
                      ['app/SiloUI/.config/nextest.toml'], ['app/SiloUI/src/test/contracts/x.json'],
                      ['app/SiloUI/src-tauri/src/commands/remote.rs']):
            with self.subTest(paths=paths):
                self.assertTrue(changes.needs_engine(paths))

    def test_empty_list_runs_everything(self):
        for paths in ([], ['', '  \n']):
            self.assertTrue(changes.needs_native(paths))
            self.assertTrue(changes.needs_engine(paths))

    def test_command_line_output(self):
        run = lambda text: subprocess.run([sys.executable, str(SCRIPT)], input=text, capture_output=True,
                                          text=True, check=True).stdout
        self.assertEqual(run('docs/README.md\nwebsite/x.ts\n'), 'native=false\nengine=false\n')
        self.assertEqual(run('app/SiloUI/src-tauri/src/main.rs\n'), 'native=true\nengine=false\n')
        self.assertEqual(run(''), 'native=true\nengine=true\n')


class ReadBoundaryTests(unittest.TestCase):
    def native_inputs(self):
        names = tracked('app/SiloUI/*.rs', 'app/SiloUI/*Cargo.toml', 'app/SiloUI/*tauri*.json')
        return [name for name in names if '/vendor/' not in name]

    def test_no_native_input_names_a_skippable_path(self):
        trees = '|'.join(p.rstrip('/') for p in changes.SKIPPABLE_PREFIXES)
        for name in self.native_inputs():
            with self.subTest(file=name):
                for path in referenced_paths(name):
                    self.assertFalse(changes.native_unaffected(path), path)
                # Path-building chains such as .join("..").join("docs") are not resolvable by regex.
                self.assertNotRegex((ROOT / name).read_text(errors='replace'), r'join\("(?:' + trees + r')"\)')

    def test_engine_crates_name_no_path_outside_their_scope(self):
        scope = tuple(f'app/SiloUI/{crate}/' for crate in ENGINE_CRATES)
        for name in self.native_inputs():
            if not name.startswith(scope):
                continue
            with self.subTest(file=name):
                for path in referenced_paths(name):
                    self.assertFalse(changes.engine_unaffected(path), path)

    def test_engine_depends_only_on_crates_in_its_scope(self):
        pending, seen = ['src-tauri/engine'], set()
        while pending:
            crate = pending.pop()
            if crate in seen:
                continue
            seen.add(crate)
            manifest = tomllib.loads((APP / crate / 'Cargo.toml').read_text())
            for table in ('dependencies', 'dev-dependencies', 'build-dependencies'):
                for spec in manifest.get(table, {}).values():
                    if isinstance(spec, dict) and 'path' in spec:
                        pending.append(os.path.relpath(os.path.normpath(APP / crate / spec['path']), APP))
        self.assertEqual(seen, set(ENGINE_CRATES))

    def test_no_tracked_symlink_points_into_skippable_trees(self):
        root = ROOT.resolve()
        for entry in tracked('-s'):
            mode, _, _, name = entry.split(None, 3)
            if mode != '120000' or changes.engine_unaffected(name):
                continue
            with self.subTest(link=name):
                target = (ROOT / name).resolve()
                self.assertTrue(target.is_relative_to(root), 'symlink leaves the repository')
                relative = target.relative_to(root).as_posix()
                self.assertFalse(changes.engine_unaffected(relative) or changes.engine_unaffected(relative + '/'))


class WorkflowWiringTests(unittest.TestCase):
    def test_pushes_skip_detection_and_jobs_fail_open(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        job = lambda name: re.search(r'(?ms)^  ' + name + r':\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow)[1]
        detect = job('changes')
        self.assertIn("if: github.event_name == 'pull_request'", detect)
        # Without --no-renames, a file moved into a skippable tree lists only its new path.
        self.assertIn('git -c core.quotePath=false diff --name-only --no-renames HEAD^1 HEAD', detect)
        self.assertIn("engine: ${{ steps.native.outputs.engine }}", detect)
        rust, linux = job('rust'), job('engine-linux')
        for body in (rust, linux):
            self.assertIn('needs: changes', body)
        self.assertIn("if: ${{ !cancelled() && needs.changes.outputs.native != 'false' }}", rust)
        self.assertIn("if: ${{ !cancelled() && needs.changes.outputs.engine != 'false' }}", linux)
        self.assertIn("ENGINE: ${{ needs.changes.outputs.engine }}", rust)
        self.assertIn('if [ "$ENGINE" = false ]; then', rust)


if __name__ == '__main__':
    unittest.main()
