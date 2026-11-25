"""The parts of facts.py that need no recorded session. Run: python3 -m unittest test_facts -v"""
import unittest
import facts as F
from facts_traffic_only import segments, test_command


def sel(argv):
    r, rest = F.runner_of(argv)
    return r, [s.get("value", s["selector"]) for s in F.selectors(r, rest)] if r else None


class Selectors(unittest.TestCase):
    def test_whole_suites(self):
        for a in (["python3", "-m", "pytest", "-q"], ["pytest", "-x"], ["go", "test", "./..."], ["cargo", "test"],
                  ["node", "--test"], ["/repo/node_modules/.bin/jest", "--ci"], ["vitest", "run"]):
            r, s = sel(a)
            self.assertIsNotNone(r, a)
            self.assertEqual([x for x in s if x not in ("-x", "run")], [], a)

    def test_narrowed(self):
        cases = {("pytest", "-k", "div"): ["div"], ("pytest", "tests/test_a.py::test_b"): ["tests/test_a.py::test_b"],
                 ("go", "test", "-run", "TestX", "./..."): ["TestX"], ("cargo", "test", "parser"): ["parser"],
                 ("node", "--test", "--test-name-pattern=login"): ["login"], ("jest", "-t", "login"): ["login"],
                 ("jest", "--onlyChanged"): ["--onlyChanged"], ("vitest", "related", "src/a.ts"): ["related", "src/a.ts"],
                 ("mocha", "--grep", "auth"): ["auth"], ("pytest", "--lf"): ["--lf"]}
        for a, want in cases.items():
            self.assertEqual(sel(list(a))[1], want, a)

    def test_cargo_args_after_double_dash_are_the_harness_s(self):
        self.assertEqual(sel(["cargo", "test", "--", "--nocapture"])[1], [])

    def test_not_a_runner(self):
        self.assertEqual(F.runner_of(["python3", "manage.py", "runserver"])[0], None)


class Outcome(unittest.TestCase):
    def test_cut_by_head(self):
        run = {"name": "pytest", "exit": 1, "end": 10}
        member, reader = {"end": 10}, {"end": 5}
        self.assertEqual(F.outcome(run, member, [member, reader]), "output cut, outcome unknown")

    def test_failed_then_tail(self):
        run = {"name": "pytest", "exit": 1, "end": 10}
        member, reader = {"end": 10}, {"end": 11}
        self.assertEqual(F.outcome(run, member, [member, reader]), "failed")

    def test_no_tests(self):
        run = {"name": "pytest", "exit": 5, "end": 1}
        self.assertEqual(F.outcome(run, {"end": 1}, [{"end": 1}]), "no tests collected")


class ShownCounts(unittest.TestCase):
    def test_pytest(self):
        self.assertEqual(F.shown_counts("x\n1 failed, 4 passed in 0.02s"), {"failed": 1, "passed": 4})

    def test_node_tap(self):
        self.assertEqual(F.shown_counts("# tests 3\n# pass 2\n# fail 1\n"), {"passed": 2, "failed": 1, "skipped": 0})

    def test_jest(self):
        self.assertEqual(F.shown_counts("Tests:       1 failed, 4 passed, 5 total"), {"failed": 1, "passed": 4})

    def test_cut(self):
        self.assertIsNone(F.shown_counts("===== test session starts =====\nplatform linux"))


class CommandText(unittest.TestCase):
    def test_pipeline_split(self):
        self.assertEqual(segments("python3 -m pytest -q 2>&1 | tail -5"), [[["python3", "-m", "pytest", "-q"], ["tail", "-5"]]])

    def test_script_runner_not_seen(self):
        self.assertEqual(test_command(["npm", "test"])[0], "npm script (runner not seen)")
        self.assertEqual(test_command(["make", "test"])[0], "make target (runner not seen)")


class Weakening(unittest.TestCase):
    def cp(self, files):
        return {"files": {p: {"sha": str(hash(t)), "text": t} for p, t in files.items()}}

    def test_markers_and_deleted_tests(self):
        a = self.cp({"test_a.py": "def test_x():\n    assert f()\n", "tests/test_b.py": "x", "src/a.js": "f()\n"})
        b = self.cp({"test_a.py": "@pytest.mark.skip\ndef test_x():\n    pass\n", "src/a.js": "f() // eslint-disable-line\n",
                     ".github/workflows/ci.yml": "on: push"})
        whats = sorted(w["what"] for w in F.weakening(a, b))
        self.assertEqual(whats, ["fewer assertions", "file that decides which checks run changed", "lint or type suppression added",
                                 "skip marker added", "test file deleted"])


if __name__ == "__main__":
    unittest.main()
