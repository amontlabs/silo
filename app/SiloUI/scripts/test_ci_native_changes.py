"""The native CI jobs may skip only pull requests that no Rust code can read."""
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import sys
import unittest

SCRIPT = Path(__file__).with_name('ci-native-changes.py')
ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / 'app/SiloUI'
spec = importlib.util.spec_from_file_location('ci_native_changes', SCRIPT)
changes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(changes)

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

    def test_any_other_change_runs_native_jobs(self):
        for paths in (['app/SiloUI/src-tauri/src/main.rs'], ['README.md', 'app/SiloUI/package.json'],
                      ['app/SiloUI/README.md'], ['.github/workflows/ci.yml'], ['LICENSE'],
                      ['app/SiloUI/src/test/contracts/backup-state.json'],
                      ['app/SiloUI/src/features/application/model/reserved-secret-names.json'],
                      ['app/SiloUI/srcx/a.ts'], ['docsy/file.md'], ['websites/x']):
            with self.subTest(paths=paths):
                self.assertTrue(changes.needs_native(paths))

    def test_empty_list_runs_everything(self):
        for paths in ([], ['', '  \n']):
            self.assertTrue(changes.needs_native(paths))

    def test_command_line_output(self):
        run = lambda text: subprocess.run([sys.executable, str(SCRIPT)], input=text, capture_output=True,
                                          text=True, check=True).stdout
        self.assertEqual(run('docs/README.md\napp/SiloUI/src/App.tsx\n'), 'native=false\n')
        self.assertEqual(run('app/SiloUI/src-tauri/src/main.rs\n'), 'native=true\n')
        self.assertEqual(run(''), 'native=true\n')


class ReadBoundaryTests(unittest.TestCase):
    def native_inputs(self):
        names = tracked('app/SiloUI/*.rs', 'app/SiloUI/*Cargo.toml', 'app/SiloUI/*tauri*.json')
        return [name for name in names if '/vendor/' not in name]

    def test_no_native_input_names_a_skippable_path(self):
        trees = '|'.join(p.rstrip('/') for p in changes.SKIPPABLE_PREFIXES)
        for name in self.native_inputs():
            with self.subTest(file=name):
                for path in referenced_paths(name):
                    self.assertFalse(changes.skippable(path), path)
                text = (ROOT / name).read_text(errors='replace')
                # Path-building chains such as .join("..").join("docs") are not resolvable by regex.
                self.assertNotRegex(text, r'join\("(?:' + trees + r')"\)')
                # Path joins resolve from the crate, or from src-tauri for test_support::manifest_root().
                crate = crate_dir(ROOT / name)
                bases = {crate, crate.parent} if 'manifest_root()' in text else {crate}
                for joined in re.findall(r'join\("([^"]+)"\)', text):
                    for base in bases:
                        target = Path(os.path.normpath(base / joined))
                        if target.exists() and target.is_relative_to(ROOT):
                            relative = target.relative_to(ROOT).as_posix() + ('/' if target.is_dir() else '')
                            with self.subTest(join=joined):
                                self.assertFalse(changes.skippable(relative), relative)

    def test_no_tracked_symlink_points_into_skippable_trees(self):
        root = ROOT.resolve()
        for entry in tracked('-s'):
            mode, _, _, name = entry.split(None, 3)
            if mode != '120000' or changes.skippable(name):
                continue
            with self.subTest(link=name):
                target = (ROOT / name).resolve()
                self.assertTrue(target.is_relative_to(root), 'symlink leaves the repository')
                relative = target.relative_to(root).as_posix()
                self.assertFalse(changes.skippable(relative) or changes.skippable(relative + '/'))


class WorkflowWiringTests(unittest.TestCase):
    def test_pushes_skip_detection_and_jobs_fail_open(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        job = lambda name: re.search(r'(?ms)^  ' + name + r':\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow)[1]
        detect = job('changes')
        self.assertIn("if: github.event_name == 'pull_request'", detect)
        # Without --no-renames, a file moved into a skippable tree lists only its new path.
        self.assertIn('git -c core.quotePath=false diff --name-only --no-renames HEAD^1 HEAD', detect)
        for name in ('rust', 'engine-linux'):
            body = job(name)
            with self.subTest(job=name):
                self.assertIn('needs: changes', body)
                self.assertIn("if: ${{ !cancelled() && needs.changes.outputs.native != 'false' }}", body)


if __name__ == '__main__':
    unittest.main()
