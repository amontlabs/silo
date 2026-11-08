"""The Linux host workflow runs a real computer on a KVM runner with a Silo Dev engine installed
from its archive, and its end-to-end script drives and always cleans up e2e-hosts-* computers."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[2]
WORKFLOW = (ROOT / '.github/workflows/linux-host.yml').read_text()
E2E = SCRIPTS / 'linux-host-e2e.sh'
HOSTS_E2E = SCRIPTS / 'hosts-e2e.sh'
RUNNER_E2E = SCRIPTS / 'runner-host-e2e.sh'

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
        failures = Path(os.environ['FAKE_STATE'] + '.image')
        left = int(os.environ.get('FAKE_IMAGE_FAILS', '0')) - (int(failures.read_text()) if failures.exists() else 0)
        if left > 0:
            failures.write_text(str((int(failures.read_text()) if failures.exists() else 0) + 1))
            sys.exit('silo: Silo could not download the VM image. Check your network connection, then retry.')
        if os.environ.get('FAKE_CREATE_FAILS'):
            sys.exit('silo: no space left')
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

# Runs a command "as" an account: records the account for the fake silo and silod.
FAKE_RUN_AS = textwrap.dedent('''\
    #!/bin/sh
    user=$1
    shift
    FAKE_USER=$user exec "$@"
''')

# The host's and the controller's `silo`, sharing one JSON state with the host's computers,
# the host's authorized keys and the controller's saved devices.
FAKE_TWO_SILOS = textwrap.dedent('''\
    #!{python}
    import json, os, sys
    from pathlib import Path
    state_path = Path(os.environ['FAKE_STATE'])
    state = json.loads(state_path.read_text()) if state_path.exists() else {{'computers': {{}}, 'keys': [], 'devices': []}}
    user, args = os.environ['FAKE_USER'], sys.argv[1:]
    with open(os.environ['FAKE_CALLS'], 'a') as calls:
        calls.write(user + ': ' + ' '.join(args) + '\\n')
    def save():
        state_path.write_text(json.dumps(state))
    as_json = '--json' in args
    args = [a for a in args if a != '--json']
    device = None
    if args[:1] == ['--device']:
        device, args = args[1], args[2:]
        assert user == 'ctl' and device in state['devices'], (user, device)
    remote = user == 'host' or device is not None
    command, rest = args[0], args[1:]
    key = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKVyZDGnacgUkbjXHi/TsIZ+GxIgprlkfONi9t33BGz3 ctl'
    print_ = 'SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw'
    computers = state['computers']
    if command == 'list':
        listed = computers if remote or '--all-devices' in rest else {{}}
        print(json.dumps({{'computers': [{{'name': n, 'state': s}} for n, s in listed.items()]}}))
    elif command == 'devices' and rest[0] == 'key':
        print(json.dumps({{'publicKey': key, 'fingerprint': print_}}))
    elif command == 'host' and rest[0] == 'init':
        added = key not in state['keys']
        given = open(rest[rest.index('--authorize') + 1]).read().strip()
        assert given == key, given
        if added:
            state['keys'].append(key); save()
        print(json.dumps({{'vm': {{'ok': True}}, 'service': {{'status': 'enabled', 'linger': 'yes'}},
                          'connections': {{'enabled': True}},
                          'authorized': [{{'fingerprint': print_, 'added': added}}],
                          'controllers': [{{'fingerprint': print_}} for _ in state['keys']]}}))
    elif command == 'host' and rest[0] == 'status':
        print(json.dumps({{'controllers': [{{'fingerprint': print_}} for _ in state['keys']]}}))
    elif command == 'devices' and rest[0] == 'add':
        assert rest[1] == 'host@127.0.0.1:22', rest
        pin = rest[rest.index('--host-key') + 1]
        state['devices'].append('dev-host'); save()
        print(json.dumps({{'device': {{'id': 'dev-host', 'name': 'box'}}, 'repaired': [], 'hostKey': {{'fingerprint': pin}}}}))
    elif command == 'devices' and rest[0] == 'remove':
        state['devices'].remove(rest[1]); state['keys'] = []; save()
    elif command == 'create':
        computers[rest[0]] = 'stopped'; save()
    elif command == 'start':
        computers[rest[0]] = 'running'; save()
    elif command == 'stop':
        computers[rest[0]] = 'stopped'; save()
    elif command == 'exec':
        print(os.environ.get('FAKE_UNAME', 'Linux e2e 6.12.0 x86_64 GNU/Linux'))
    elif command == 'fork':
        computers[rest[rest.index('--name') + 1]] = 'running'; save()
        print('Linux e2e 6.12.0 x86_64 GNU/Linux')
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
                 'Drive a Linux computer and a container computer through the silo command', 'Stop the engine and remove the service']
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

    def test_container_computers_are_driven_after_the_host_is_prepared_for_them(self):
        steps = WORKFLOW.split('    steps:\n', 1)[1]
        prepare = steps.split('- name: Prepare the host for container computers\n', 1)[1].split('      - ', 1)[0]
        for needle in ('apt-get install', 'crun skopeo umoci uidmap', 'kernel.apparmor_restrict_unprivileged_userns=0', '/etc/subuid'):
            self.assertIn(needle, prepare)
        self.assertLess(steps.index('- name: Prepare the host for container computers'),
                        steps.index('- name: Drive a Linux computer and a container computer'))
        drive = steps.split('- name: Drive a Linux computer and a container computer', 1)[1].split('      - ', 1)[0]
        self.assertIn("SILO_E2E_CONTAINER: '1'", drive)
        script = E2E.read_text()
        for needle in ('create "$ctr" --container', 'CapEff:0000000000000000', 'NoNewPrivs:1', 'io_uring', '169.254.169.254',
                       'container_checks', '"$silo" drop "$ctr"'):
            self.assertIn(needle, script)

    def test_silo_runner_runs_as_a_service_next_to_the_engine_without_contacting_github(self):
        steps = WORKFLOW.split('    steps:\n', 1)[1]
        step = steps.split('- name: Run silo-runner as a service next to the engine\n', 1)[1].split('      - ', 1)[0]
        self.assertIn('bash scripts/runner-host-e2e.sh "$ENGINE" "$UNIT"', step)
        self.assertLess(steps.index('--service register'), steps.index('- name: Run silo-runner as a service'))
        self.assertLess(steps.index('- name: Run silo-runner as a service'), steps.index('- name: Drive a Linux computer'))
        script = RUNNER_E2E.read_text()
        for needle in ('check -offline', 'service register', '-engine-unit', 'service unregister', 'journalctl --user -u',
                       'Machine:'):
            self.assertIn(needle, script)
        # No token is given to the helper: CI holds no GitHub admin credential.
        self.assertNotIn('SILO_RUNNER_TOKEN=', script)
        self.assertNotIn('api.github.com', script)
        checked = subprocess.run(['bash', '-n', str(RUNNER_E2E)], capture_output=True, text=True)
        self.assertEqual(checked.returncode, 0, checked.stderr)
        usage = subprocess.run(['bash', str(RUNNER_E2E)], capture_output=True, text=True)
        self.assertEqual(usage.returncode, 2)
        self.assertIn('usage:', usage.stderr)

    def test_two_accounts_manage_a_computer_on_each_other_after_the_first_run(self):
        steps = WORKFLOW.split('    steps:\n', 1)[1]
        order = ['Stop the engine and remove the service', 'Prepare a host and a controller account',
                 'Manage a computer on another device', "Stop both accounts' engines", 'Collect logs']
        positions = [steps.index(f'- name: {name}\n') for name in order]
        self.assertEqual(positions, sorted(positions))
        prepare = steps.split('- name: Prepare a host and a controller account\n', 1)[1].split('      - ', 1)[0]
        # Both accounts run one engine directory that neither can change.
        for needle in ('sudo install -d -o root -g root -m 0755 /var/lib/silo-ci',
                       'sudo chown -R root:root /var/lib/silo-ci/silo-engine', 'sudo chmod -R go-w /var/lib/silo-ci/silo-engine',
                       'sudo systemctl start ssh', 'sudo loginctl enable-linger "$account"',
                       "sudo usermod --password '*' \"$account\""):
            self.assertIn(needle, prepare)
        self.assertIn('bash scripts/hosts-e2e.sh /var/lib/silo-ci/silo-engine silo-host silo-controller', steps)
        stop = steps.split("- name: Stop both accounts' engines\n", 1)[1].split('      - ', 1)[0]
        self.assertTrue(stop.startswith("        if: always() && env.HOSTS_E2E == '1'\n"), stop)
        self.assertIn('--service unregister', stop)

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
               'SILO_E2E_READY_TIMEOUT': '1', 'SILO_E2E_RETRY_WAIT': '0', **environment}
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

    def test_a_failed_image_download_is_retried(self):
        result, calls = self.run_script(FAKE_IMAGE_FAILS='2')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(calls[:3], ['create e2e-hosts-ci --linux --cpus 1 --memory 2 --disk 2'] * 3)
        self.assertEqual(self.computers(), {})

    def test_a_persistent_download_failure_fails_after_three_attempts(self):
        result, calls = self.run_script(FAKE_IMAGE_FAILS='9')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([c for c in calls if c.startswith('create')], ['create e2e-hosts-ci --linux --cpus 1 --memory 2 --disk 2'] * 3)
        self.assertIn('could not download the VM image', result.stderr)

    def test_another_create_failure_is_not_retried(self):
        result, calls = self.run_script(FAKE_CREATE_FAILS='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([c for c in calls if c.startswith('create')], ['create e2e-hosts-ci --linux --cpus 1 --memory 2 --disk 2'])

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


class HostsEndToEndScriptTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.engine = self.root / 'silo-engine'
        self.engine.mkdir()
        for name, text in (('silo', FAKE_TWO_SILOS.format(python=sys.executable)),
                           ('silod', '#!/bin/sh\necho "$FAKE_USER: silod $*" >> "$FAKE_CALLS"\necho enabled\n')):
            (self.engine / name).write_text(text)
            (self.engine / name).chmod(0o755)
        self.run_as = self.root / 'run-as'
        self.run_as.write_text(FAKE_RUN_AS)
        self.run_as.chmod(0o755)
        self.host_key = self.root / 'ssh_host_ed25519_key.pub'
        self.host_key.write_text('ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKVyZDGnacgUkbjXHi/TsIZ+GxIgprlkfONi9t33BGz3 root@box\n')
        self.state, self.calls = self.root / 'state.json', self.root / 'calls.txt'

    def run_script(self, **environment):
        env = {**os.environ, 'FAKE_STATE': str(self.state), 'FAKE_CALLS': str(self.calls),
               'SILO_E2E_RUN_AS': str(self.run_as), 'SILO_E2E_HOST_KEY_FILE': str(self.host_key),
               'SILO_E2E_READY_TIMEOUT': '1', **environment}
        result = subprocess.run(['bash', str(HOSTS_E2E), str(self.engine), 'host', 'ctl'], env=env,
                                capture_output=True, text=True, timeout=60)
        calls = [line for line in self.calls.read_text().splitlines() if ' list' not in line] \
            if self.calls.exists() else []
        return result, calls

    def state_now(self):
        return json.loads(self.state.read_text())

    @unittest.skipUnless(shutil.which('ssh-keygen'), 'needs ssh-keygen')
    def test_it_prepares_the_host_adds_it_and_drives_a_computer_there(self):
        result, calls = self.run_script(SILO_E2E_REMOTE_EXEC='1')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        fingerprint = 'SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw'
        self.assertEqual(calls[:5], [
            'ctl: silod --service register',
            'ctl: --json devices key',
            f'host: --json host init --authorize {calls[2].split()[-1]}',
            f'host: --json host init --authorize {calls[2].split()[-1]}',
            'host: host status',
        ])
        self.assertIn(f'ctl: --json devices add host@127.0.0.1:22 --host-key {fingerprint}', calls)
        tail = calls[calls.index(f'ctl: --json devices add host@127.0.0.1:22 --host-key {fingerprint}') + 1:]
        self.assertEqual(tail, [
            'ctl: --device dev-host create e2e-hosts-remote --linux --cpus 1 --memory 2 --disk 2',
            'ctl: --device dev-host start e2e-hosts-remote',
            'ctl: --device dev-host exec e2e-hosts-remote -- uname -a',
            'ctl: --device dev-host fork e2e-hosts-remote --name e2e-hosts-remote-fork -- uname -a',
            'ctl: --device dev-host stop e2e-hosts-remote-fork',
            'ctl: --device dev-host drop e2e-hosts-remote-fork',
            'ctl: --device dev-host stop e2e-hosts-remote',
            'ctl: --device dev-host drop e2e-hosts-remote',
            'ctl: devices remove dev-host',
            'host: --json host status',
        ])
        self.assertEqual(self.state_now(), {'computers': {}, 'keys': [], 'devices': []})
        self.assertIn('== Passed', result.stdout)

    @unittest.skipUnless(shutil.which('ssh-keygen'), 'needs ssh-keygen')
    def test_without_remote_exec_it_skips_exec_and_fork(self):
        result, calls = self.run_script()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse([call for call in calls if ' exec ' in call or ' fork ' in call])

    @unittest.skipUnless(shutil.which('ssh-keygen'), 'needs ssh-keygen')
    def test_a_failed_step_drops_what_it_made_and_removes_the_host(self):
        result, calls = self.run_script(SILO_E2E_REMOTE_EXEC='1', FAKE_UNAME='Darwin')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('did not print a Linux kernel', result.stderr)
        self.assertEqual(calls[-3:], ['host: stop e2e-hosts-remote', 'host: drop e2e-hosts-remote',
                                      'ctl: devices remove dev-host'])
        self.assertEqual(self.state_now(), {'computers': {}, 'keys': [], 'devices': []})

    def test_it_never_touches_a_computer_it_did_not_make(self):
        self.state.write_text(json.dumps({'computers': {'e2e-hosts-remote': 'running'}, 'keys': [], 'devices': []}))
        result, calls = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('already exists', result.stderr)
        self.assertFalse([call for call in calls if 'devices add' in call or ' drop ' in call])
        self.assertEqual(self.state_now()['computers'], {'e2e-hosts-remote': 'running'})
        result, _ = self.run_script(SILO_E2E_COMPUTER='main')
        self.assertEqual(result.returncode, 2)
        self.assertIn('named e2e-hosts-*', result.stderr)


if __name__ == '__main__':
    unittest.main()
