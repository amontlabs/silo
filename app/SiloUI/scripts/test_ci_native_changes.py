"""The native CI jobs may skip only pull requests that no Rust code can read."""
import importlib.util
from pathlib import Path
import re
import subprocess
import sys
import unittest

SCRIPT = Path(__file__).with_name('ci-native-changes.py')
ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('ci_native_changes', SCRIPT)
changes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(changes)


class NativeChangeTests(unittest.TestCase):
    def test_documentation_website_and_demo_only_changes_skip(self):
        for paths in (['README.md'], ['website/package.json', 'website/package-lock.json'],
                      ['demo/src/film.ts', 'docs/SiloUI-APP.md', 'assets/silo-logo.svg', 'SECURITY.md'],
                      ['artifacts/silo-logo-system.html']):
            with self.subTest(paths=paths):
                self.assertFalse(changes.needs_native(paths))

    def test_any_other_change_runs_native_jobs(self):
        for paths in (['app/SiloUI/src-tauri/src/main.rs'], ['README.md', 'app/SiloUI/package.json'],
                      ['app/SiloUI/README.md'], ['.github/workflows/ci.yml'], ['LICENSE'],
                      ['app/SiloUI/src/App.tsx'], ['docsy/file.md'], ['websites/x']):
            with self.subTest(paths=paths):
                self.assertTrue(changes.needs_native(paths))

    def test_empty_list_runs_native_jobs(self):
        self.assertTrue(changes.needs_native([]))
        self.assertTrue(changes.needs_native(['', '  \n']))

    def test_command_line_output(self):
        result = subprocess.run([sys.executable, str(SCRIPT)], input='docs/README.md\nwebsite/x.ts\n',
                                capture_output=True, text=True, check=True)
        self.assertEqual(result.stdout, 'native=false\n')
        result = subprocess.run([sys.executable, str(SCRIPT)], input='', capture_output=True, text=True, check=True)
        self.assertEqual(result.stdout, 'native=true\n')

    def test_skippable_trees_are_not_read_by_rust_code(self):
        tracked = subprocess.check_output(['git', 'ls-files', 'app/SiloUI/*.rs', 'app/SiloUI/**/build.rs'],
                                          cwd=ROOT, text=True).splitlines()
        reference = re.compile(r'(?:\.\./){2,}(?:' + '|'.join(p.rstrip('/') for p in changes.SKIPPABLE_PREFIXES) + r')/')
        for name in tracked:
            text = (ROOT / name).read_text(errors='replace')
            with self.subTest(file=name):
                self.assertIsNone(reference.search(text))

    def test_no_tracked_symlink_points_into_skippable_trees(self):
        entries = subprocess.check_output(['git', 'ls-files', '-s'], cwd=ROOT, text=True).splitlines()
        for entry in entries:
            mode, _, _, name = entry.split(None, 3)
            if mode == '120000':
                with self.subTest(link=name):
                    target = (ROOT / name).resolve().relative_to(ROOT.resolve()).as_posix()
                    self.assertFalse(changes.skippable(target) and not changes.skippable(name))

    def test_main_pushes_and_workflow_wiring_always_run_native_jobs(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        # Without --no-renames, a file moved into a skippable tree lists only its new path.
        self.assertIn('git diff --name-only --no-renames HEAD^1 HEAD', workflow)
        self.assertIn('if [ "$EVENT" != pull_request ]; then\n            echo native=true', workflow)
        for job in ('rust', 'engine-linux'):
            body = re.search(r'(?ms)^  ' + job + r':\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow)[1]
            with self.subTest(job=job):
                self.assertIn('needs: changes', body)
                self.assertIn("if: ${{ !cancelled() && needs.changes.outputs.native != 'false' }}", body)


if __name__ == '__main__':
    unittest.main()
