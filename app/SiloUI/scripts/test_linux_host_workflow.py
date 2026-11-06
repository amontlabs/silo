"""The Linux host workflow runs a real computer on a KVM runner with a Silo Dev engine installed
from its archive, and its end-to-end script drives and always cleans up e2e-hosts-* computers."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[2]
WORKFLOW = (ROOT / '.github/workflows/linux-host.yml').read_text()
E2E = SCRIPTS / 'linux-host-e2e.sh'

# A `silo` that keeps its computers in a JSON file and records every call.
FAKE_SILO = textwrap.dedent('''\
    #!{python}
    import json, os, sys
    from pathlib import Path
    state = Path(os.environ['FAKE_STATE'])
    computers = json.loads(state.read_text()) if state.exists() else {{}}
    args = sys.argv[1:]
    with open(os.environ['FAKE_CALLS'], 'a') as calls:
        calls.write(' '.join(args) + '\\n')
    as_json = args[:1] == ['--json']
    args = args[1:] if as_json else args
    command, rest = args[0], args[1:]
    def save():
        state.write_text(json.dumps(computers))
    if command == 'list':
        if os.environ.get('FAKE_DOWN') or (as_json and os.environ.get('FAKE_JSON_LIST_FAILS')):
            sys.exit(1)
        print(json.dumps({{'computers': [{{'name': n, 'state': s}} for n, s in computers.items()]}}))
    elif command == 'create':
        computers[rest[0]] = 'stopped'; save()
    elif command == 'start':
        computers[rest[0]] = 'running'; save()
    elif command == 'stop':
        computers[rest[0]] = 'stopped'; save()
    elif command == 'fork':
        computers[rest[rest.index('--name') + 1]] = 'stopped'; save()
    elif command == 'status':
        print(json.dumps({{'name': rest[0], 'state': computers[rest[0]]}}))
    elif command == 'exec':
        print(os.environ.get('FAKE_UNAME', 'Linux e2e 6.12.0 x86_64 GNU/Linux'))
    elif command == 'checkpoint':
        pass
    elif command == 'drop':
        if computers[rest[0]] == 'running':
            sys.exit(f'{{rest[0]}} is still running')
        computers.pop(rest[0]); save()
    else:
        sys.exit(f'unexpected command {{args}}')
''')


class LinuxHostWorkflowTests(unittest.TestCase):
    def test_it_runs_on_pull_requests_and_by_hand_with_read_only_access_and_no_secrets(self):
        trigger = WORKFLOW.split('\njobs:', 1)[0]
        self.assertRegex(trigger, r'(?m)^on:\n  workflow_dispatch:\n  pull_request:\n    paths:\n')
        self.assertNotIn('push:', trigger)
        self.assertNotIn('schedule:', trigger)
        self.assertIn("      - 'app/SiloUI/**'", trigger)
        self.assertIn("      - '.github/workflows/linux-host.yml'", trigger)
        self.assertRegex(WORKFLOW, r'(?m)^permissions:\n  contents: read\n')
        self.assertEqual(WORKFLOW.count('permissions:'), 1)
        self.assertNotIn('secrets.', WORKFLOW)
        for name, value in (('SILO_GITHUB_APP_SLUG', 'silo-ci-test'), ('SILO_GITHUB_CLIENT_ID', 'test-client'),
                            ('SILO_GITHUB_CLIENT_SECRET', 'test-secret')):
            self.assertIn(f'      {name}: {value}\n', WORKFLOW)

    def test_the_frontend_paths_it_skips_are_the_ones_no_rust_code_reads(self):
        native = (SCRIPTS / 'ci-native-changes.py').read_text()
        for path in re.findall(r"'(app/SiloUI/src/[^']+)'", native):
            pattern = path + '**' if path.endswith('/') else path
            self.assertIn(f"      - '{pattern}'", WORKFLOW)

    def test_the_job_has_kvm_and_fails_without_it(self):
        self.assertIn('runs-on: ubuntu-24.04\n', WORKFLOW)
        self.assertIn('KERNEL=="kvm", GROUP="kvm", MODE="0666", OPTIONS+="static_node=kvm"', WORKFLOW)
        self.assertIn('udevadm trigger --name-match=kvm', WORKFLOW)
        self.assertIn("fcntl.ioctl(device, 0xAE00, 0) == 12", WORKFLOW)
        self.assertIn('fcntl.ioctl(device, 0xAE01, 0)', WORKFLOW)
        self.assertNotIn('continue-on-error', WORKFLOW)

    def test_it_installs_the_dev_engine_archive_and_runs_it_as_a_user_service(self):
        steps = WORKFLOW.split('    steps:\n', 1)[1]
        order = ['Probe KVM', 'Prepare the runtime', 'Build and package the Dev engine',
                 'Install the engine from its archive', 'Start the engine as a systemd user service',
                 'Drive a Linux computer through the silo command', 'Stop the engine and remove the service']
        positions = [steps.index(f'- name: {name}\n') for name in order]
        self.assertEqual(positions, sorted(positions))
        self.assertIn('package-engine.py --platform linux-x64 --channel development', steps)
        self.assertIn('sha256sum -c', steps)
        install = steps.split('- name: Install the engine from its archive\n', 1)[1].split('      - ', 1)[0]
        # The engine-directory trust check refuses group- or world-writable paths.
        self.assertLess(install.index('tar -xzf'), install.index('chmod -R go-w "$RUNNER_TEMP/prefix"'))
        self.assertIn('--print-channel-identifier)" = org.silo.dev', steps)
        self.assertIn('sudo loginctl enable-linger "$(id -un)"', steps)
        self.assertNotIn('$USER', WORKFLOW)
        self.assertIn('echo "DBUS_SESSION_BUS_ADDRESS=unix:path=$runtime_dir/bus" >> "$GITHUB_ENV"', steps)
        self.assertIn('"$ENGINE/silod" --service register', steps)
        self.assertIn("'linger: yes'", steps)
        self.assertIn('bash scripts/linux-host-e2e.sh "$ENGINE"', steps)
        self.assertIn('UNIT: org.silo.dev.engine.service', WORKFLOW)
        # A graceful stop: SIGTERM, exit 0, socket removed; then the unit is gone.
        stop = steps.split('- name: Stop the engine and remove the service\n', 1)[1].split('      - ', 1)[0]
        # It stops the engine after a failure too, and asserts only after a passing run.
        self.assertTrue(stop.startswith("        if: always() && env.ENGINE != ''\n"), stop)
        self.assertIn('JOB_STATUS: ${{ job.status }}', stop)
        self.assertIn('if [[ "$JOB_STATUS" != success ]]; then', stop)
        for needle in ('systemctl --user stop "$UNIT"', 'test "$result" = success', 'test "$status" = 0',
                       'engine.sock', '--service unregister', '= notRegistered'):
            self.assertIn(needle, stop)

    def test_it_saves_no_caches_and_installs_no_openssl(self):
        # Pull requests never write caches; prepare-release-runtime only restores here.
        self.assertNotIn('rust-cache', WORKFLOW)
        self.assertNotIn('save:', WORKFLOW)
        # Nothing in either lockfile links OpenSSL (reqwest uses rustls).
        self.assertNotIn('libssl-dev', WORKFLOW)
        self.assertNotIn('name = "openssl-sys"', (ROOT / 'app/SiloUI/Cargo.lock').read_text())

    def test_logs_are_uploaded_only_when_it_fails(self):
        collect = WORKFLOW.split('- name: Collect logs\n', 1)[1]
        self.assertTrue(collect.startswith('        if: failure()\n'))
        upload = WORKFLOW.split('uses: actions/upload-artifact@', 1)[1]
        self.assertIn('        if: failure()\n', upload)
        self.assertIn('name: linux-host-logs', upload)
        self.assertIn('journalctl --user --unit "$UNIT"', collect)


class LinuxHostEndToEndScriptTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.engine = self.root / 'silo-engine'
        self.engine.mkdir()
        silo = self.engine / 'silo'
        silo.write_text(FAKE_SILO.format(python=sys.executable))
        silo.chmod(0o755)
        self.state, self.calls = self.root / 'state.json', self.root / 'calls.txt'

    def run_script(self, **environment):
        env = {**os.environ, 'FAKE_STATE': str(self.state), 'FAKE_CALLS': str(self.calls),
               'SILO_E2E_READY_TIMEOUT': '1', **environment}
        result = subprocess.run(['bash', str(E2E), str(self.engine)], env=env, capture_output=True, text=True,
                                timeout=60)
        calls = [line for line in self.calls.read_text().splitlines() if not line.startswith(('list', '--json list'))] \
            if self.calls.exists() else []
        return result, calls

    def computers(self):
        return json.loads(self.state.read_text()) if self.state.exists() else {}

    def test_it_creates_starts_runs_checkpoints_forks_and_drops_both(self):
        result, calls = self.run_script()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(calls, [
            'create e2e-hosts-ci --linux --cpus 1 --memory 2 --disk 2',
            'start e2e-hosts-ci',
            'exec e2e-hosts-ci -- uname -a',
            'checkpoint e2e-hosts-ci --name e2e-hosts-ci',
            'fork e2e-hosts-ci --name e2e-hosts-ci-fork',
            '--json status e2e-hosts-ci-fork',
            'start e2e-hosts-ci-fork',
            'exec e2e-hosts-ci-fork -- uname -a',
            'stop e2e-hosts-ci-fork',
            'stop e2e-hosts-ci',
            'drop e2e-hosts-ci-fork',
            'drop e2e-hosts-ci',
        ])
        self.assertEqual(self.computers(), {})
        self.assertIn('== Passed', result.stdout)

    def test_a_failed_step_still_drops_what_it_made(self):
        result, calls = self.run_script(FAKE_UNAME='Darwin')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('did not print a Linux kernel', result.stderr)
        self.assertEqual(calls[-2:], ['stop e2e-hosts-ci', 'drop e2e-hosts-ci'])
        self.assertEqual(self.computers(), {})

    def test_it_never_touches_a_computer_it_did_not_make(self):
        self.state.write_text(json.dumps({'e2e-hosts-ci': 'running'}))
        result, calls = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('already exists', result.stderr)
        self.assertEqual(calls, [])
        self.assertEqual(self.computers(), {'e2e-hosts-ci': 'running'})
        result, calls = self.run_script(SILO_E2E_COMPUTER='main')
        self.assertEqual(result.returncode, 2)
        self.assertIn('named e2e-hosts-*', result.stderr)

    def test_a_list_that_fails_stops_the_run_before_it_makes_anything(self):
        result, calls = self.run_script(FAKE_JSON_LIST_FAILS='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Could not list', result.stderr)
        self.assertEqual(calls, [])

    def test_an_engine_that_does_not_answer_fails(self):
        result, calls = self.run_script(FAKE_DOWN='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('did not answer', result.stderr)
        self.assertEqual(calls, [])


if __name__ == '__main__':
    unittest.main()
