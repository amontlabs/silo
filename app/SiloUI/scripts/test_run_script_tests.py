"""The parallel script-test runner must cover every module discover would find and fail on any failure."""
import importlib.util
from pathlib import Path
import unittest

SCRIPTS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('run_script_tests', SCRIPTS / 'run-script-tests.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_no_test_modules_in_subdirectories(self):
        # The runner globs the top level only; discover would also recurse into packages.
        nested = [path for path in SCRIPTS.rglob('test_*.py') if path.parent != SCRIPTS and 'node_modules' not in path.parts]
        self.assertEqual(nested, [])

    def test_module_with_no_tests_fails(self):
        name, code, _ = runner.run(SCRIPTS / 'run-script-tests.py')
        self.assertNotEqual(code, 0, name)


if __name__ == '__main__':
    unittest.main()
