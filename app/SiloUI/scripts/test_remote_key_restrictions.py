"""Opt-in loopback sshd fixture; no app, VM, or installed SSH keys are used."""
import contextlib
import os
from pathlib import Path
import pwd
import re
import shutil
import socket
import subprocess
import tempfile
import time
import unittest


REMOTE_SOURCE = Path(__file__).parents[1] / 'src-tauri/engine/src/remote.rs'


@unittest.skipUnless(os.environ.get('SILO_TEST_LOCAL_SSHD') == '1', 'Opt-in local sshd test')
class RemoteKeyRestrictionsTest(unittest.TestCase):
    def test_owner_key_only_runs_forced_command_and_cannot_forward(self):
        sshd = shutil.which('sshd') or '/usr/sbin/sshd'
        if not Path(sshd).is_file():
            self.skipTest('OpenSSH server is unavailable')
        source = REMOTE_SOURCE.read_text()
        options = re.search(r'fn authorized_key_options\(\).*?r#"(.*?)"#', source, re.S).group(1)
        # Replace only the channel's forced app command with a fixture byte stream.
        with tempfile.TemporaryDirectory(prefix='silo-sshd-test-', dir='/tmp') as directory:
            root = Path(directory)
            env = {**os.environ, 'HOME': directory}
            options = options.format(f'cd {root} && exec /usr/bin/env HOME={root} /bin/cat')
            for key in ['host', 'client']:
                subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(root / key)],
                               check=True, env=env, cwd=root, capture_output=True)
            public = (root / 'client.pub').read_text().strip()
            authorized = root / 'authorized_keys'
            authorized.write_text(f'{options} {public}\n')
            authorized.chmod(0o600)
            port = self.free_port()
            known = root / 'known_hosts'
            known.write_text(f'[127.0.0.1]:{port} {(root / "host.pub").read_text()}')
            username = pwd.getpwuid(os.getuid()).pw_name
            config = root / 'sshd.conf'
            config.write_text(f'''ListenAddress 127.0.0.1
Port {port}
HostKey {root}/host
PidFile {root}/pid
AuthorizedKeysFile {authorized}
StrictModes no
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
PermitUserRC no
AllowUsers {username}
AllowTcpForwarding yes
AllowStreamLocalForwarding yes
GatewayPorts yes
LogLevel ERROR
''')
            with (root / 'sshd.log').open('w+') as log:
                server = subprocess.Popen([sshd, '-D', '-e', '-f', str(config)], env=env, cwd=root,
                                          stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                try:
                    self.await_listener(server, port)
                    client = ['ssh', '-F', 'none', '-p', str(port), '-i', str(root / 'client'),
                              '-o', 'IdentityAgent=none', '-o', 'IdentitiesOnly=yes',
                              '-o', 'StrictHostKeyChecking=yes', '-o', f'UserKnownHostsFile={known}',
                              '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=3']
                    target = f'{username}@127.0.0.1'
                    reply = subprocess.run([*client, '--', target, 'ignored'], input=b'bridge fixture\n',
                                           capture_output=True, env=env, cwd=root, timeout=5)
                    self.assertEqual(reply.returncode, 0, reply.stderr.decode())
                    self.assertEqual(reply.stdout, b'bridge fixture\n')
                    with contextlib.ExitStack() as stack:
                        tcp = stack.enter_context(socket.socket())
                        tcp.bind(('127.0.0.1', 0))
                        tcp.listen()
                        tcp.settimeout(.1)
                        unix = stack.enter_context(socket.socket(socket.AF_UNIX))
                        unix.bind(str(root / 'sentinel.sock'))
                        unix.listen()
                        unix.settimeout(.1)
                        for forwarding in [f'127.0.0.1:{self.free_port()}:127.0.0.1:{tcp.getsockname()[1]}',
                                           f'127.0.0.1:{self.free_port()}', f'{root}/remote.sock:127.0.0.1:{tcp.getsockname()[1]}']:
                            result = subprocess.run([*client, '-N', '-o', 'ExitOnForwardFailure=yes',
                                                     '-R', forwarding, '--', target], capture_output=True,
                                                    env=env, cwd=root, timeout=5)
                            self.assertNotEqual(result.returncode, 0, forwarding)
                            self.assertIn(b'forwarding failed', result.stderr, result.stderr.decode())
                        self.assertFalse((root / 'remote.sock').exists())
                        for destination, sentinel in [(f'127.0.0.1:{tcp.getsockname()[1]}', tcp),
                                                       (str(root / 'sentinel.sock'), unix)]:
                            local = self.free_port()
                            control = root / 'control'
                            tunnel = subprocess.Popen([*client, '-N', '-M', '-S', str(control), '-L',
                                                       f'127.0.0.1:{local}:{destination}', '--', target],
                                                      env=env, cwd=root, stdin=subprocess.DEVNULL,
                                                      stdout=subprocess.DEVNULL, stderr=log)
                            try:
                                self.await_listener(tunnel, local)
                                with socket.create_connection(('127.0.0.1', local), timeout=3) as connection:
                                    connection.sendall(b'forbidden')
                                    try:
                                        self.assertEqual(connection.recv(20), b'')
                                    except ConnectionResetError:
                                        pass
                                with self.assertRaises(socket.timeout):
                                    sentinel.accept()
                            finally:
                                self.stop(tunnel)
                            control.unlink(missing_ok=True)
                finally:
                    self.stop(server)

    @staticmethod
    def free_port():
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            return reservation.getsockname()[1]

    @staticmethod
    def await_listener(process, port):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise AssertionError(f'Fixture process exited: {process.returncode}')
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1):
                    return
            except OSError:
                time.sleep(.01)
        raise AssertionError('Fixture listener did not start')

    @staticmethod
    def stop(process):
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=5)


if __name__ == '__main__':
    unittest.main()
