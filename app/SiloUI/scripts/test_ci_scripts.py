"""CI's test jobs run only the scripts in scripts/ci.

ci.yml installs tools, restores caches, runs the offline runtime preflight and
calls scripts/ci/*.sh; a test command written directly in ci.yml would run in
CI only. No job or step may hide a failure (continue-on-error, a skipping
`if:` or its own shell). Every `cargo nextest` call in those scripts takes its
flags from the one shared variable in scripts/ci/common.sh.

The workflow is parsed with PyYAML; scripts/ci/scripts.sh runs this module
through uv with PyYAML pinned.
"""
import os
import re
import shlex
import subprocess
import tempfile
from pathlib import Path
import unittest

try:
    import yaml
except ImportError as error:
    raise SystemExit('PyYAML is missing: run scripts/ci/scripts.sh, or from app/SiloUI '
                     'uv run --no-project --with pyyaml==6.0.3 python -m unittest scripts/test_ci_scripts.py') from error

ROOT = Path(__file__).resolve().parents[3]
CI = ROOT / 'app/SiloUI/scripts/ci'
WORKFLOW = ROOT / '.github/workflows/ci.yml'

SHARED = 'SILO_NEXTEST_FLAGS'
SHARED_USE = f'"${{{SHARED}[@]}}"'
REQUIRED_FLAGS = ['--locked', '--profile', 'ci', '--no-fail-fast', '--retries', '0', '--flaky-result', 'fail',
                  '--ignore-default-filter']
FLAG_VALUES = (('--profile', 'ci'), ('--retries', '0'), ('--flaky-result', 'fail'))
# Options and settings that stop a nextest run early, rerun tests, filter them
# or replace the shared profile.
WEAKENING = re.compile(r'--fail-fast\b|--ff\b|--max-fail\b|--retries\b|--flaky-result\b|--default-filter\b'
                       r'|--profile\b|\bNEXTEST_\w+')
NEXTEST_CALL = re.compile(r'\bcargo\s+(?:\+\S+\s+)?nextest\b|\bcargo-nextest\b')
PROPER_NEXTEST_CALL = re.compile(rf'cargo nextest run {re.escape(SHARED_USE)}(?:\s|$)')

# Job-level conditions: the change detection runs for pull requests, and the
# native jobs fail open on its output.
CHANGES_CONDITION = "github.event_name == 'pull_request'"
NATIVE_CONDITION = "${{ !cancelled() && needs.changes.outputs.native != 'false' }}"
# The one step-level condition: a scripts/ci call also runs after an earlier step failed.
STEP_CONDITION = '${{ !cancelled() }}'
# nextest's install steps name their download with these; nextest reads no other NEXTEST_ setting from ci.yml.
NEXTEST_INSTALL_ENV = {'NEXTEST_VERSION', 'NEXTEST_ASSET', 'NEXTEST_SHA256'}

# Commands that check or test something, named in the error for a direct call.
TEST_COMMAND = re.compile(
    r'\bcargo\s+(?:\+\S+\s+)?(?:nextest|test|fmt|tree|clippy|check)\b(?!\s+--version)|\bcargo-nextest\b'
    r'|\bnpm\s+(?:(?:--prefix\s+\S+\s+)?(?:test|t)\b|(?:--prefix\s+\S+\s+)?run(?:-script)?\s+(?:typecheck|lint|test\S*))'
    r'|\bnpx\b|\bvitest\b|\btsc\b|\boxlint\b|\bnode\s+--test\b'
    r'|-m\s+(?:unittest|pytest)\b|(?:^|[\s;&|])pytest\b'
    r'|run-script-tests|validate-release-version|check-command-manifest'
    r'|\bgo\s+(?:test|vet)\b|\bgofmt\b|\bshellcheck\b'
    r'|\blychee\s+-|"?\$\{?LYCHEE\}?"?\s+-'
    r'|stageGitRuntime')
# Every other line of a `run:` outside the change detection installs a tool or
# runs the offline runtime preflight.
TEMP_FILE = r'"\$RUNNER_TEMP/[\w.-]+"'
INSTALL_LINES = [re.compile(pattern) for pattern in (
    r'npm ci',
    r'node scripts/preflight\.mjs',
    r'sudo apt-get update && sudo apt-get install -y --no-install-recommends [\w.+-]+(?: [\w.+-]+)*',
    r'brew install [\w-]+',
    r'python3 -m pip install --quiet -r [\w./-]*requirements[\w.-]*\.txt',
    r'runtime_toolchain=\$\(node -p "require\(process\.env\.GITHUB_WORKSPACE \+ '
    r"'/app/SiloUI/runtime-inputs\.json'\)\.toolchain\"\)",
    r'rustup toolchain install "\$runtime_toolchain" --profile minimal(?: --component rustfmt)?',
    r'rustup default "\$runtime_toolchain"',
    rf'curl -fsSL --retry 3 -o {TEMP_FILE} "https://github\.com/[\w./$-]+"',
    r'echo "\$\w+_SHA256  \$RUNNER_TEMP/[\w.-]+" \| (?:sha256sum|shasum -a 256) --check --strict -',
    rf'mkdir -p {TEMP_FILE}',
    rf'tar -xzf {TEMP_FILE} -C (?:{TEMP_FILE}|"\$\(dirname "\$\(command -v cargo\)"\)")',
    r'lychee=\$\(find "\$RUNNER_TEMP/lychee" -type f -name lychee \| head -n 1\)',
    r'test -x "\$lychee"',
    r'echo "LYCHEE=\$lychee" >> "\$GITHUB_ENV"',
    r'cargo nextest --version',
)]
# A scripts/ci call is the script and plain arguments: no shell operator can
# mask its exit status or run something after it.
SCRIPT_CALL = re.compile(r'(?:\.\./|app/SiloUI/)?scripts/ci/([\w-]+\.sh)(?: +[\w./=:,+@%-]+)*')
EXPRESSION = re.compile(r'\$\{\{[^}]*\}\}')


def command_lines(command):
    """The lines of a `run:` value, with backslash continuations joined."""
    return [line.strip() for line in command.replace('\\\n', ' ').splitlines() if line.strip()]


def workflow_problems(workflow, scripts):
    """Problems with a workflow's test commands; scripts names the files in scripts/ci."""
    problems = []
    called = set()
    calling = set()
    document = yaml.safe_load(workflow) or {}
    jobs = document.get('jobs') or {}

    def nextest_env(where, env, allowed=()):
        for key in sorted(str(key) for key in (env or {})):
            if key.startswith('NEXTEST_') and key not in allowed:
                problems.append(f'{where}: sets {key}')

    def default_shell(where, defaults):
        if isinstance(defaults, dict) and 'shell' in (defaults.get('run') or {}):
            problems.append(f'{where}: sets a default shell')

    default_shell('workflow', document.get('defaults'))
    nextest_env('workflow', document.get('env'))
    for job, body in jobs.items():
        body = body or {}
        if 'continue-on-error' in body:
            problems.append(f'{job}: continue-on-error hides its failures')
        if 'if' in body and str(body['if']).strip() != (CHANGES_CONDITION if job == 'changes' else NATIVE_CONDITION):
            problems.append(f'{job}: runs only if {body["if"]}')
        default_shell(job, body.get('defaults'))
        nextest_env(job, body.get('env'))
        if job == 'changes':
            continue
        for index, step in enumerate(body.get('steps') or []):
            name = f'{job} step {step.get("name") or index + 1}'
            command = step.get('run')
            calls_script = isinstance(command, str) and 'scripts/ci/' in command
            if 'continue-on-error' in step:
                problems.append(f'{name}: continue-on-error hides its failures')
            if 'shell' in step:
                problems.append(f'{name}: sets its own shell')
            if 'if' in step and not (calls_script and str(step['if']).strip() == STEP_CONDITION):
                problems.append(f'{name}: runs only if {step["if"]}')
            nextest_env(name, step.get('env'), NEXTEST_INSTALL_ENV if command is not None and not calls_script else ())
            if command is None:
                continue
            lines = command_lines(str(command))
            if calls_script:
                call = SCRIPT_CALL.fullmatch(EXPRESSION.sub('X', lines[0])) if len(lines) == 1 else None
                if not call:
                    problems.append(f'{job}: a step that calls scripts/ci runs something else too: {" / ".join(lines)}')
                elif call[1] not in scripts:
                    problems.append(f'{job}: calls missing scripts/ci/{call[1]}')
                else:
                    called.add(call[1])
                    calling.add(job)
                continue
            for line in lines:
                if any(pattern.fullmatch(line) for pattern in INSTALL_LINES):
                    continue
                if TEST_COMMAND.search(line):
                    problems.append(f'{job}: runs a test command outside scripts/ci: {line}')
                else:
                    problems.append(f'{job}: runs a command that is neither an install nor a scripts/ci call: {line}')
    for job in sorted(set(jobs) - {'changes'}):
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
        for flag, value in FLAG_VALUES:
            if flag in flags and flags[flags.index(flag) + 1:flags.index(flag) + 2] != [value]:
                problems.append(f'{SHARED} must set {flag} {value}')
        extra = [flag for flag in flags if flag not in REQUIRED_FLAGS]
        if extra:
            problems.append(f'{SHARED} has other flags: {" ".join(extra)}')
    for name, text in sorted(scripts.items()):
        code = '\n'.join(line.split('#', 1)[0] for line in text.splitlines())
        if name == 'common.sh':
            if len(re.findall(rf'\b{SHARED}\b', code)) != 1:
                problems.append(f'common.sh changes {SHARED} beside its definition')
            continue
        if not re.search(r'(?m)^source "\$\(dirname "\$\{BASH_SOURCE\[0\]\}"\)/common\.sh"$', text):
            problems.append(f'{name} does not source common.sh')
        if WEAKENING.search(code) or SHARED in code.replace(SHARED_USE, ''):
            problems.append(f'{name} changes the shared nextest flags')
        for line in code.splitlines():
            for call in NEXTEST_CALL.finditer(line):
                if not PROPER_NEXTEST_CALL.match(line, call.start()):
                    problems.append(f'{name}: cargo nextest without {SHARED_USE}: {line.strip()}')
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
    scripts = {'common.sh': '', 'frontend.sh': '', 'links.sh': '', 'linux.sh': '', 'macos.sh': '', 'updater.sh': ''}
    calls = ('  frontend:\n    steps:\n      - run: scripts/ci/frontend.sh\n'
             '      - run: scripts/ci/linux.sh\n      - run: scripts/ci/macos.sh\n      - run: scripts/ci/updater.sh\n'
             '  docs:\n    steps:\n      - run: app/SiloUI/scripts/ci/links.sh\n')

    def problems(self, steps):
        return workflow_problems('jobs:\n' + steps, self.scripts)

    def with_extra_job(self, job):
        return self.problems(self.calls + '  extra:\n' + job)

    def test_script_calls_pass(self):
        self.assertEqual(self.problems(
            '  frontend:\n    steps:\n      - run: npm ci\n'
            '      - run: scripts/ci/frontend.sh tests --maxWorkers=2 --shard=${{ matrix.shard }}/3\n'
            '      - if: ${{ !cancelled() }}\n        run: scripts/ci/linux.sh engine\n'
            '      - run: scripts/ci/macos.sh\n      - run: scripts/ci/updater.sh\n'
            '  docs:\n'
            "    if: ${{ !cancelled() && needs.changes.outputs.native != 'false' }}\n"
            '    steps:\n      - name: Links\n        run: app/SiloUI/scripts/ci/links.sh\n'
            '      - name: Install cargo-nextest\n        env:\n          NEXTEST_VERSION: 0.9.151\n'
            '        run: cargo nextest --version\n'), [])

    def test_a_direct_test_command_fails(self):
        for command in ['cargo nextest run -p silo-engine --locked', 'npm test -- --shard=1/3', 'npm run typecheck',
                        'python3 -m unittest discover -s scripts', 'go test ./...', '"$LYCHEE" --offline x.md',
                        'cargo test --locked -p tauri-plugin-updater', '"$LYCHEE" \\\n          --offline x.md']:
            with self.subTest(command=command):
                problems = self.with_extra_job('    steps:\n      - run: scripts/ci/links.sh\n'
                                               f'      - name: Extra\n        run: |\n          npm ci\n          {command}\n')
                self.assertTrue(any('outside scripts/ci' in problem for problem in problems), problems)

    def test_any_command_but_an_install_fails(self):
        for command in ['python3 scripts/test_x.py', 'node scripts/foo.test.mjs']:
            with self.subTest(command=command):
                problems = self.with_extra_job(f'    steps:\n      - run: scripts/ci/links.sh\n      - run: {command}\n')
                self.assertIn(f'extra: runs a command that is neither an install nor a scripts/ci call: {command}', problems)

    def test_a_script_call_with_extra_commands_fails(self):
        for command in ['|\n          scripts/ci/frontend.sh\n          npm test', 'scripts/ci/linux.sh || true',
                        'scripts/ci/macos.sh | tee log.txt', 'scripts/ci/updater.sh ; cargo test -p x',
                        'scripts/ci/linux.sh && cargo nextest run', 'scripts/ci/linux.sh > log.txt',
                        'scripts/ci/linux.sh & wait', 'scripts/ci/linux.sh < /dev/null', 'scripts/ci/linux.sh $(true)']:
            with self.subTest(command=command):
                problems = self.with_extra_job(f'    steps:\n      - run: {command}\n')
                self.assertTrue(any('runs something else too' in problem for problem in problems), problems)

    def test_hiding_a_failure_fails(self):
        cases = {
            '    continue-on-error: true\n    steps:\n      - run: scripts/ci/linux.sh\n':
                'extra: continue-on-error hides its failures',
            '    steps:\n      - run: scripts/ci/linux.sh\n        continue-on-error: true\n':
                'extra step 1: continue-on-error hides its failures',
            '    steps:\n      - run: scripts/ci/linux.sh\n        if: false\n': 'extra step 1: runs only if False',
            '    steps:\n      - run: npm ci\n        if: ${{ !cancelled() }}\n      - run: scripts/ci/linux.sh\n':
                'extra step 1: runs only if ${{ !cancelled() }}',
            "    if: github.ref == 'refs/heads/x'\n    steps:\n      - run: scripts/ci/linux.sh\n":
                "extra: runs only if github.ref == 'refs/heads/x'",
            '    steps:\n      - run: scripts/ci/linux.sh\n        shell: bash {0}\n': 'extra step 1: sets its own shell',
            '    defaults:\n      run:\n        shell: sh\n    steps:\n      - run: scripts/ci/linux.sh\n':
                'extra: sets a default shell',
            '    env:\n      NEXTEST_PROFILE: default\n    steps:\n      - run: scripts/ci/linux.sh\n':
                'extra: sets NEXTEST_PROFILE',
            '    steps:\n      - run: scripts/ci/linux.sh\n        env:\n          NEXTEST_VERSION: 1\n':
                'extra step 1: sets NEXTEST_VERSION',
        }
        for job, problem in cases.items():
            with self.subTest(job=job):
                self.assertIn(problem, self.with_extra_job(job))

    def test_a_flow_style_step_is_checked(self):
        problems = self.with_extra_job('    steps:\n      - run: scripts/ci/linux.sh\n'
                                       '      - {name: x, run: cargo nextest run -p silo-engine}\n')
        self.assertIn('extra: runs a test command outside scripts/ci: cargo nextest run -p silo-engine', problems)

    def test_a_missing_or_uncalled_script_fails(self):
        problems = self.problems(self.calls.replace('app/SiloUI/scripts/ci/links.sh', 'app/SiloUI/scripts/ci/missing.sh'))
        self.assertIn('docs: calls missing scripts/ci/missing.sh', problems)
        self.assertIn('scripts/ci/links.sh is not called by ci.yml', problems)

    def test_a_job_without_a_script_fails(self):
        problems = self.with_extra_job('    steps:\n      - run: npm ci\n')
        self.assertEqual(problems, ['extra: calls no scripts/ci script'])


class ScriptGuardTests(unittest.TestCase):
    common = f'{SHARED}=(--locked --profile ci --no-fail-fast --retries 0 --flaky-result fail --ignore-default-filter)\n'
    source = 'source "$(dirname "${BASH_SOURCE[0]}")/common.sh"\n'

    def problems(self, body, common=None):
        return script_problems({'common.sh': self.common if common is None else common, 'linux.sh': self.source + body})

    def test_shared_flags_pass(self):
        self.assertEqual(self.problems(f'step engine cargo nextest run {SHARED_USE} -p silo-engine\n'
                                       f'  step host cargo nextest run {SHARED_USE}\n'), [])

    def test_nextest_without_the_shared_variable_fails(self):
        for call in ['cargo nextest run --locked -p silo-host', 'cargo nextest run -p silo-host "${SILO_NEXTEST_FLAGS[@]}x"',
                     'cargo nextest run $SILO_NEXTEST_FLAGS -p silo-host', 'cargo nextest r "${SILO_NEXTEST_FLAGS[@]}"',
                     'cargo-nextest nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silo-host',
                     'cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p a; cargo nextest run -p b']:
            with self.subTest(call=call):
                problems = self.problems(f'step host {call}\n')
                self.assertTrue(any('without' in problem for problem in problems), problems)

    def test_overriding_the_flags_fails(self):
        for body in ['--retries 2 -p silo-host', '-p silo-host --fail-fast', '-p silo-host --ff', '-p silo-host --max-fail 1',
                     '-p silo-host --max-fail=1', '--profile default -p silo-host']:
            with self.subTest(body=body):
                problems = self.problems(f'step host cargo nextest run {SHARED_USE} {body}\n')
                self.assertIn('linux.sh changes the shared nextest flags', problems)
        for line in [f'{SHARED}+=(--fail-fast)', f'{SHARED}=(--locked)', f'{SHARED}[0]=--ff', f'unset {SHARED}',
                     'export NEXTEST_PROFILE=default', 'NEXTEST_FAIL_FAST=1 cargo nextest run "${SILO_NEXTEST_FLAGS[@]}"']:
            with self.subTest(line=line):
                problems = self.problems(f'{line}\nstep host cargo nextest run {SHARED_USE} -p silo-host\n')
                self.assertIn('linux.sh changes the shared nextest flags', problems)

    def test_changing_the_shared_variable_in_common_fails(self):
        problems = self.problems(f'step host cargo nextest run {SHARED_USE}\n', self.common + f'{SHARED}+=(--fail-fast)\n')
        self.assertIn(f'common.sh changes {SHARED} beside its definition', problems)

    def test_a_weaker_shared_variable_fails(self):
        problems = script_problems({'common.sh': f'{SHARED}=(--locked --no-fail-fast --retries 2 --flaky-result fail)\n'})
        self.assertIn(f'{SHARED} lacks --ignore-default-filter', problems)
        self.assertIn(f'{SHARED} lacks --profile', problems)
        self.assertIn(f'{SHARED} must set --retries 0', problems)
        problems = script_problems({'common.sh': self.common.replace('--profile ci', '--profile default')})
        self.assertIn(f'{SHARED} must set --profile ci', problems)
        problems = script_problems({'common.sh': self.common.replace(')', ' --fail-fast)')})
        self.assertIn(f'{SHARED} has other flags: --fail-fast', problems)

    def test_a_script_that_does_not_source_common_fails(self):
        problems = script_problems({'common.sh': self.common, 'runner.sh': 'go test ./...\n'})
        self.assertIn('runner.sh does not source common.sh', problems)


class ScriptPartTests(unittest.TestCase):
    """Runs the scripts with a cargo and python3 that record their arguments."""

    def run_script(self, *arguments, cargo_tree_fails=False):
        with tempfile.TemporaryDirectory() as folder:
            log = Path(folder) / 'calls.log'
            for tool in ('cargo', 'python3'):
                fake = Path(folder) / tool
                fake.write_text('#!/bin/sh\n'
                                f'echo "{tool} $*" >> "{log}"\n'
                                + ('[ "$1" = tree ] && exit 101\n' if cargo_tree_fails and tool == 'cargo' else '')
                                + 'exit 0\n')
                fake.chmod(0o755)
            env = {**os.environ, 'PATH': f'{folder}{os.pathsep}{os.environ["PATH"]}'}
            result = subprocess.run(['bash', *arguments], cwd=ROOT, env=env, capture_output=True, text=True)
            calls = log.read_text().splitlines() if log.exists() else []
            return result, calls

    def nextest_packages(self, calls):
        return [call.split(' --ignore-default-filter ', 1)[1] for call in calls if call.startswith('cargo nextest run ')]

    def test_linux_parts(self):
        expected = {
            'engine': ['-p silo-engine'],
            'daemon': ['-p silod -p silo-cli -p silo-client -p silo-api -p silo-container-shim'],
            'host': ['-p silo-host', '-p silo-host --no-default-features'],
        }
        for part, packages in expected.items():
            with self.subTest(part=part):
                result, calls = self.run_script(str(CI / 'linux.sh'), part)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.nextest_packages(calls), packages)
        result, calls = self.run_script(str(CI / 'linux.sh'))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.nextest_packages(calls), [package for packages in expected.values() for package in packages])
        result, _ = self.run_script(str(CI / 'linux.sh'), 'everything')
        self.assertEqual(result.returncode, 2)

    def test_a_failing_cargo_tree_is_not_reported_as_gtk(self):
        result, calls = self.run_script(str(CI / 'linux.sh'), 'daemon', cargo_tree_fails=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn('cargo tree failed for silod', result.stderr)
        self.assertNotIn('depends on the GTK stack', result.stderr)
        self.assertEqual(len(self.nextest_packages(calls)), 1, 'the daemon tests still run')

    def test_macos_parts(self):
        result, calls = self.run_script(str(CI / 'macos.sh'), 'checks')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([call.split()[:3] for call in calls],
                         [['python3', '-m', 'unittest'], ['cargo', 'fmt', '--check']])
        result, calls = self.run_script(str(CI / 'macos.sh'), 'tests')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.nextest_packages(calls), ['--manifest-path Cargo.toml --workspace'])
        result, calls = self.run_script(str(CI / 'macos.sh'))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 3)


if __name__ == '__main__':
    unittest.main()
