"""CI's test jobs run only the scripts in scripts/ci, which local checks run too.

ci.yml installs tools, caches and calls scripts/ci/*.sh; a test command written
directly in ci.yml would run in CI only. Every `cargo nextest` call in those
scripts takes its flags from the one shared variable in scripts/ci/common.sh.
"""
import re
import shlex
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[3]
CI = ROOT / 'app/SiloUI/scripts/ci'
WORKFLOW = ROOT / '.github/workflows/ci.yml'

SHARED = 'SILO_NEXTEST_FLAGS'
REQUIRED_FLAGS = ['--locked', '--no-fail-fast', '--retries', '0', '--flaky-result', 'fail', '--ignore-default-filter']

# Commands that check or test something. Installing tools (and printing their
# version), caching and the offline runtime preflight stay in ci.yml.
TEST_COMMAND = re.compile(
    r'\bcargo\s+(?:\+\S+\s+)?(?:nextest|test|fmt|tree|clippy|check)\b(?!\s+--version)'
    r'|\bnpm\s+(?:(?:--prefix\s+\S+\s+)?(?:test|t)\b|(?:--prefix\s+\S+\s+)?run(?:-script)?\s+(?:typecheck|lint|test\S*))'
    r'|\bnpx\b|\bvitest\b|\btsc\b|\boxlint\b|\bnode\s+--test\b'
    r'|-m\s+(?:unittest|pytest)\b|(?:^|[\s;&|])pytest\b'
    r'|run-script-tests|validate-release-version|check-command-manifest'
    r'|\bgo\s+(?:test|vet)\b|\bgofmt\b|\bshellcheck\b'
    r'|\blychee\s+-|"?\$\{?LYCHEE\}?"?\s+-'
    r'|stageGitRuntime')
SCRIPT_CALL = re.compile(r'(?:\.\./|app/SiloUI/)?scripts/ci/([\w-]+\.sh)(?:\s.*)?')


def run_steps(workflow):
    """Yields (job, command) for every `run:` in the workflow's jobs."""
    lines = workflow.splitlines()
    job = None
    index = 0
    while index < len(lines):
        line = lines[index]
        index += 1
        header = re.match(r'^  ([\w-]+):\s*$', line)
        if header:
            job = header[1]
            continue
        found = re.match(r'^( *)(?:- )?run: ?(.*)$', line)
        if not found or job is None:
            continue
        indent = len(found[1])
        value = found[2].strip()
        if value[:1] in ('|', '>'):
            body = []
            while index < len(lines) and (not lines[index].strip() or len(lines[index]) - len(lines[index].lstrip()) > indent):
                body.append(lines[index].strip())
                index += 1
            value = '\n'.join(line for line in body if line)
        yield job, value


def workflow_problems(workflow, scripts):
    """Problems with a workflow's test commands; scripts names the files in scripts/ci."""
    problems = []
    called = set()
    jobs = set()
    calling = set()
    for job, command in run_steps(workflow):
        jobs.add(job)
        if 'scripts/ci/' in command:
            for line in command.splitlines():
                call = SCRIPT_CALL.fullmatch(line.strip())
                if not call:
                    problems.append(f'{job}: a step that calls scripts/ci runs something else too: {line.strip()}')
                elif call[1] not in scripts:
                    problems.append(f'{job}: calls missing scripts/ci/{call[1]}')
                else:
                    called.add(call[1])
                    calling.add(job)
            continue
        # A folded or continued command can split a test command over lines.
        found = [line for line in command.splitlines() if TEST_COMMAND.search(line)]
        joined = ' '.join(command.replace('\\\n', ' ').split())
        if not found and TEST_COMMAND.search(joined):
            found = [joined]
        for line in found:
            problems.append(f'{job}: runs a test command outside scripts/ci: {line}')
    for job in sorted(jobs - {'changes'}):
        if job not in calling:
            problems.append(f'{job}: calls no scripts/ci script')
    for name in sorted(set(scripts) - called - {'common.sh'}):
        problems.append(f'scripts/ci/{name} is not called by ci.yml')
    return problems


def script_problems(scripts):
    """Problems with the nextest calls of scripts/ci; scripts maps file names to their text."""
    problems = []
    common = scripts.get('common.sh', '')
    definition = re.findall(rf'(?m)^{SHARED}=\((.*)\)$', common)
    if len(definition) != 1:
        problems.append(f'common.sh must define {SHARED} once')
    else:
        flags = shlex.split(definition[0])
        for flag in REQUIRED_FLAGS:
            if flag.startswith('--') and flag not in flags:
                problems.append(f'{SHARED} lacks {flag}')
        for flag, value in (('--retries', '0'), ('--flaky-result', 'fail')):
            if flag in flags and flags[flags.index(flag) + 1:flags.index(flag) + 2] != [value]:
                problems.append(f'{SHARED} must set {flag} {value}')
    for name, text in sorted(scripts.items()):
        if name == 'common.sh':
            continue
        if not re.search(r'(?m)^source "\$\(dirname "\$\{BASH_SOURCE\[0\]\}"\)/common\.sh"$', text):
            problems.append(f'{name} does not source common.sh')
        if re.search(rf'(?m)^\s*{SHARED}=|--retries|--flaky-result|NEXTEST_RETRIES|NEXTEST_FLAKY_RESULT|--default-filter', text):
            problems.append(f'{name} changes the shared nextest flags')
        for line in text.splitlines():
            code = line.split('#', 1)[0]
            if re.search(r'\bcargo\s+(?:\+\S+\s+)?nextest\b', code) and \
                    not re.search(rf'\bcargo nextest run "\$\{{{SHARED}\[@\]\}}"', code):
                problems.append(f'{name}: cargo nextest without "${{{SHARED}[@]}}": {line.strip()}')
    return problems


def current_scripts():
    return {path.name: path.read_text() for path in CI.glob('*.sh')}


class CurrentFilesTests(unittest.TestCase):
    def test_ci_runs_its_tests_only_through_the_scripts(self):
        self.assertEqual(workflow_problems(WORKFLOW.read_text(), current_scripts()), [])

    def test_every_nextest_call_takes_the_shared_flags(self):
        self.assertEqual(script_problems(current_scripts()), [])

    def test_the_scripts_are_executable(self):
        for name in current_scripts():
            if name != 'common.sh':
                with self.subTest(script=name):
                    self.assertTrue((CI / name).stat().st_mode & 0o111)


class WorkflowGuardTests(unittest.TestCase):
    scripts = {'common.sh': '', 'frontend.sh': '', 'links.sh': ''}

    def problems(self, steps):
        return workflow_problems('jobs:\n' + steps, self.scripts)

    def test_script_calls_pass(self):
        self.assertEqual(self.problems(
            '  frontend:\n    steps:\n      - run: npm ci\n      - run: scripts/ci/frontend.sh tests --shard=1/3\n'
            '  docs:\n    steps:\n      - name: Links\n        run: app/SiloUI/scripts/ci/links.sh\n'), [])

    def test_a_direct_test_command_fails(self):
        for command in ['cargo nextest run -p silo-engine --locked', 'npm test -- --shard=1/3', 'npm run typecheck',
                        'python3 -m unittest discover -s scripts', 'go test ./...', '"$LYCHEE" --offline x.md',
                        'cargo test --locked -p tauri-plugin-updater', '"$LYCHEE" \\\n          --offline x.md']:
            with self.subTest(command=command):
                problems = self.problems('  frontend:\n    steps:\n      - run: scripts/ci/frontend.sh\n'
                                         '  docs:\n    steps:\n      - run: app/SiloUI/scripts/ci/links.sh\n'
                                         f'      - name: Extra\n        run: |\n          echo start\n          {command}\n')
                self.assertTrue(any('outside scripts/ci' in problem for problem in problems), problems)

    def test_a_script_call_with_extra_commands_fails(self):
        problems = self.problems('  frontend:\n    steps:\n      - run: |\n          scripts/ci/frontend.sh\n          npm test\n'
                                 '  docs:\n    steps:\n      - run: app/SiloUI/scripts/ci/links.sh\n')
        self.assertTrue(any('runs something else too' in problem for problem in problems), problems)

    def test_a_missing_or_uncalled_script_fails(self):
        problems = self.problems('  frontend:\n    steps:\n      - run: scripts/ci/frontend.sh\n'
                                 '  docs:\n    steps:\n      - run: app/SiloUI/scripts/ci/missing.sh\n')
        self.assertIn('docs: calls missing scripts/ci/missing.sh', problems)
        self.assertIn('scripts/ci/links.sh is not called by ci.yml', problems)

    def test_a_job_without_a_script_fails(self):
        problems = self.problems('  frontend:\n    steps:\n      - run: scripts/ci/frontend.sh\n'
                                 '      - run: app/SiloUI/scripts/ci/links.sh\n  docs:\n    steps:\n      - run: echo hi\n')
        self.assertEqual(problems, ['docs: calls no scripts/ci script'])


class ScriptGuardTests(unittest.TestCase):
    common = f'{SHARED}=(--locked --profile ci --no-fail-fast --retries 0 --flaky-result fail --ignore-default-filter)\n'
    source = 'source "$(dirname "${BASH_SOURCE[0]}")/common.sh"\n'

    def test_shared_flags_pass(self):
        self.assertEqual(script_problems({'common.sh': self.common, 'linux.sh': self.source +
                                          f'step engine cargo nextest run "${{{SHARED}[@]}}" -p silo-engine\n'}), [])

    def test_nextest_without_the_shared_variable_fails(self):
        for call in ['cargo nextest run --locked -p silo-host', 'cargo nextest run -p silo-host "${SILO_NEXTEST_FLAGS[@]}x"',
                     'cargo nextest run $SILO_NEXTEST_FLAGS -p silo-host']:
            with self.subTest(call=call):
                problems = script_problems({'common.sh': self.common, 'linux.sh': self.source + f'step host {call}\n'})
                self.assertTrue(any('without' in problem for problem in problems), problems)

    def test_overriding_the_flags_fails(self):
        problems = script_problems({'common.sh': self.common, 'linux.sh': self.source +
                                    f'step host cargo nextest run "${{{SHARED}[@]}}" --retries 2 -p silo-host\n'})
        self.assertIn('linux.sh changes the shared nextest flags', problems)

    def test_a_weaker_shared_variable_fails(self):
        problems = script_problems({'common.sh': f'{SHARED}=(--locked --no-fail-fast --retries 2 --flaky-result fail)\n'})
        self.assertIn(f'{SHARED} lacks --ignore-default-filter', problems)
        self.assertIn(f'{SHARED} must set --retries 0', problems)

    def test_a_script_that_does_not_source_common_fails(self):
        problems = script_problems({'common.sh': self.common, 'runner.sh': 'go test ./...\n'})
        self.assertIn('runner.sh does not source common.sh', problems)


if __name__ == '__main__':
    unittest.main()
