"""Root-only package lifecycle test. Run only in a disposable Linux container/runner."""
import ctypes
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

SCRIPTS = Path(__file__).parent
SOURCE = Path('/etc/apt/sources.list.d/silo.sources')
MARKER = Path('/var/lib/silo/package-update-in-progress')
TOOLS_DIR = 'usr/libexec/silo/tools'
PTRACE_TRACEME, PTRACE_DETACH = 0, 17


# A shell standing in for the running Silo app. For each request line (a directory and
# a version) read from the FIFO, it starts the update helper as its own child with root
# privileges and the caller's PKEXEC_UID, which is how pkexec runs the real helper.
APP_LOOP = '''exec 3<>"$1"
while read -r directory version <&3; do
  "$2" -p -c 'exec setpriv --reuid=0 --regid=0 --clear-groups env PKEXEC_UID="$1" /usr/lib/silo/silo-system-update "$2" "$3"' \\
    privileged "$(id -u)" "$$" "$version" < "$directory/input" > "$directory/stdout" 2> "$directory/stderr"
  echo $? > "$directory/status.tmp" && mv "$directory/status.tmp" "$directory/status"
done'''


def fake_package_tree(tree, app='/bin/sleep'):
    """Silo's packaged layout: the app in /usr/bin, its tools and runtime in libexec."""
    (tree / 'usr/bin').mkdir(parents=True)
    shutil.copy(app, tree / 'usr/bin/silo-ui')
    tools = tree / TOOLS_DIR
    tools.mkdir(parents=True)
    for name in ('msb', 'git', 'git-lfs', 'git-remote-http', 'git-remote-https', 'libkrunfw.so.5.6.1'):
        shutil.copy('/bin/true', tools / name)


def held(argv):
    """Start argv held right after exec: a real /proc identity without running it."""
    libc = ctypes.CDLL(None, use_errno=True)
    pid = os.fork()
    if pid == 0:
        try:
            libc.ptrace(PTRACE_TRACEME, 0, None, None)
            os.execv(argv[0], argv)
        finally:
            os._exit(127)
    _, status = os.waitpid(pid, os.WUNTRACED)
    if not os.WIFSTOPPED(status):
        raise RuntimeError(f'{argv[0]} did not stop after exec')
    return pid


def release(pid):
    """Let a held process continue; the fake executables then exit on their own."""
    ctypes.CDLL(None, use_errno=True).ptrace(PTRACE_DETACH, pid, None, None)
    os.waitpid(pid, 0)


@unittest.skipUnless(os.environ.get('SILO_APT_LIFECYCLE_TEST') == '1' and os.geteuid() == 0, 'Explicit disposable root environment required')
class InstallerTests(unittest.TestCase):
    def test_in_app_upgrade_refreshes_stale_apt_and_preserves_unrelated_processes(self):
        from test_apt_repository import RepositoryTests, repo
        self.assertFalse(Path('/usr/bin/silo-ui').exists(), 'Never run on an installed Silo host')
        fixture = RepositoryTests('test_signed_repository_selects_newest_version')
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        env = {**os.environ, 'DEBIAN_FRONTEND': 'noninteractive'}
        def run(*args, check=True, **kwargs):
            result = subprocess.run(args, env=env, capture_output=True, **kwargs)
            if check and result.returncode:
                self.fail(f'{args}: {result.stdout.decode()} {result.stderr.decode()}')
            return result
        arch = run('dpkg', '--print-architecture').stdout.decode().strip()
        for version, target, package in fixture.packages:
            tree = fixture.root / f'{version}-{target}'
            fake_package_tree(tree, '/bin/sh')
            run('dpkg-deb', '--build', str(tree), str(package))
            run('python3', str(SCRIPTS / 'package-debian-release.py'), str(package))
        old = [item for item in fixture.packages if item[0] == '0.1.0']
        repo.build(old, fixture.site, fixture.fingerprint, fixture.key)
        published = fixture.root / 'published'
        published.symlink_to(fixture.site, target_is_directory=True)
        from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
        from functools import partial
        from threading import Thread
        class Handler(SimpleHTTPRequestHandler):
            def do_GET(self):
                # Both publications can occur within the filesystem's timestamp
                # resolution; the fixture must serve its newly signed bytes.
                if 'If-Modified-Since' in self.headers:
                    del self.headers['If-Modified-Since']
                super().do_GET()
            def log_message(self, *_args):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Handler, directory=str(fixture.root)))
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        source = f'Types: deb\nURIs: http://127.0.0.1:{server.server_port}/published/apt\nSuites: stable\nComponents: main\nSigned-By: {fixture.key}\n'
        process = None
        try:
            package = next(path for version, target, path in old if target == arch)
            run('dpkg', '-i', str(package))
            SOURCE.write_text(source)
            run('apt-get', '-o', f'Dir::Etc::sourcelist={SOURCE}', '-o', 'Dir::Etc::sourceparts=-', '-o', 'APT::Get::List-Cleanup=0', 'update')
            self.assertIn(b'Candidate: 0.1.0', run('apt-cache', 'policy', 'silo').stdout)
            new_site = fixture.root / 'new-site'
            repo.build([item for item in fixture.packages if item[0] in ('0.1.0', '0.2.0')], new_site, fixture.fingerprint, fixture.key)
            published.unlink(); published.symlink_to(new_site, target_is_directory=True)
            # No apt refresh here: reproduce the exact reported stale candidate.
            self.assertIn(b'Candidate: 0.1.0', run('apt-cache', 'policy', 'silo').stdout)
            # Only the process that starts the helper may request an update, so the
            # running app launches it through a setuid-root shell, as it would through pkexec.
            requests = Path(tempfile.mkdtemp(prefix='silo-update-requests-'))
            self.addCleanup(shutil.rmtree, requests, ignore_errors=True)
            requests.chmod(0o755)
            privileged = requests / 'privileged'
            shutil.copy('/bin/sh', privileged)
            privileged.chmod(0o4755)
            fifo = requests / 'requests'
            os.mkfifo(fifo, 0o666)
            os.chmod(fifo, 0o666)
            process = subprocess.Popen(['/usr/bin/silo-ui', '-c', APP_LOOP, 'silo-ui', str(fifo), str(privileged)], user=65534)
            counter = iter(range(1000))
            def request(version, answer):
                directory = requests / str(next(counter))
                directory.mkdir()
                directory.chmod(0o777)
                (directory / 'input').write_bytes(answer)
                (directory / 'input').chmod(0o644)
                with fifo.open('w') as stream:
                    stream.write(f'{directory} {version}\n')
                deadline = time.monotonic() + 600
                while not (directory / 'status').exists():
                    self.assertLess(time.monotonic(), deadline, 'The update helper did not finish')
                    time.sleep(.1)
                return subprocess.CompletedProcess([], int((directory / 'status').read_text()), (directory / 'stdout').read_bytes(), (directory / 'stderr').read_bytes())
            # A second, uncoordinated Silo instance must still block installation.
            other = subprocess.Popen(['/usr/bin/silo-ui', '-c', 'read line'], stdin=subprocess.PIPE, user=65534)
            try:
                result = request('0.2.0', b'install\n')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b'Quit Silo', result.stderr)
                self.assertIsNone(other.poll())
                self.assertIsNone(process.poll())
            finally:
                other.terminate(); other.wait(); other.stdin.close()
            # Without the go-ahead after the download, nothing is installed.
            result = request('0.2.0', b'cancel\n')
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout.decode().splitlines(), ['refreshing', 'downloading', 'ready'])
            self.assertEqual(run('dpkg-query', '-W', '-f=${Version}', 'silo').stdout, b'0.1.0')
            result = request('0.2.0', b'install\n')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.decode().splitlines(), ['refreshing', 'downloading', 'ready', 'installing'])
            self.assertEqual(run('dpkg-query', '-W', '-f=${Version}', 'silo').stdout, b'0.2.0')
            self.assertIsNone(process.poll(), 'The updater never kills the UI')
            self.assertFalse(Path('/run/silo/system-update.json').exists())
            self.assertFalse(MARKER.exists())
            # An already updated package only needs the older running app to restart.
            self.assertEqual(request('0.2.0', b'install\n').stdout, b'ready\n')
            self.assertNotEqual(request('0.1.0', b'install\n').returncode, 0)
            # A caller that did not start the helper is refused even if it names the running app.
            env['PKEXEC_UID'] = '65534'
            result = run('/usr/lib/silo/silo-system-update', str(process.pid), '0.2.0', check=False, input=b'install\n')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'must be requested by the installed Silo application', result.stderr)
        finally:
            if process is not None:
                process.terminate(); process.wait()
            run('dpkg', '--purge', 'silo', check=False)
            SOURCE.unlink(missing_ok=True)
            MARKER.unlink(missing_ok=True)

    def test_install_opt_out_upgrade_and_running_process(self):
        self.assertFalse(Path('/usr/bin/silo-ui').exists(), 'Never run on an installed Silo host')
        env = {**os.environ, 'DEBIAN_FRONTEND': 'noninteractive'}
        def run(*args, check=True, **kw):
            result = subprocess.run(args, env=env, capture_output=True, **kw)
            if check and result.returncode:
                self.fail(f'{args}: {result.stderr.decode()}')
            return result
        with tempfile.TemporaryDirectory() as tmp:
            packages = []
            for version in ('0.1.0', '0.2.0'):
                tree = Path(tmp, version)
                (tree / 'DEBIAN').mkdir(parents=True)
                (tree / 'DEBIAN/control').write_text(f'Package: silo\nVersion: {version}\nArchitecture: {subprocess.check_output(["dpkg", "--print-architecture"], text=True).strip()}\nMaintainer: Test\nDescription: Disposable Silo lifecycle test\n')
                fake_package_tree(tree)
                package = Path(tmp, f'{version}.deb')
                run('dpkg-deb', '--build', str(tree), str(package))
                run('python3', str(SCRIPTS / 'package-debian-release.py'), str(package))
                packages.append(str(package))
            try:
                run('debconf-set-selections', input=b'silo silo/system-updates boolean false\n')
                run('dpkg', '-i', packages[0])
                self.assertFalse(SOURCE.exists())
                self.assertFalse(MARKER.exists())
                run('dpkg', '--purge', 'silo')
                run('debconf-set-selections', input=b'silo silo/system-updates boolean true\n')
                run('dpkg', '-i', packages[0])
                self.assertEqual(SOURCE.read_text(), (SCRIPTS / 'debian/silo.sources').read_text())
                process = subprocess.Popen(['/usr/bin/silo-ui', '60'])
                try:
                    result = run('dpkg', '-i', packages[1], check=False)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(b'Quit Silo', result.stderr)
                    self.assertIn(f'Silo (process {process.pid})'.encode(), result.stderr)
                    self.assertIsNone(process.poll())
                    self.assertFalse(MARKER.exists())
                finally:
                    process.terminate(); process.wait()
                runtime = held(['/' + TOOLS_DIR + '/msb', 'server'])
                try:
                    result = run('dpkg', '-i', packages[1], check=False)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f'msb, process {runtime}'.encode(), result.stderr)
                    self.assertFalse(MARKER.exists())
                finally:
                    release(runtime)
                # Remote-management relays hold no VM or app state and never block.
                relays = [held(['/usr/bin/silo-ui', '--remote-bridge']), held(['/usr/bin/silo-ui', '--remote-guest', 'office', 'vm'])]
                try:
                    SOURCE.write_text(SOURCE.read_text() + 'Enabled: no\n')
                    run('apt-get', '-y', 'install', packages[1])
                finally:
                    for relay in relays:
                        release(relay)
                self.assertEqual(run('dpkg-query', '-W', '-f=${Version}', 'silo').stdout, b'0.2.0')
                self.assertIn('Enabled: no', SOURCE.read_text())
                self.assertFalse(MARKER.exists())
                run('dpkg', '--purge', 'silo')
                self.assertTrue(SOURCE.exists(), 'Preserve administrator changes')
                SOURCE.unlink()
                run('debconf-set-selections', input=b'silo silo/system-updates boolean true\n')
                run('dpkg', '-i', packages[0])
                SOURCE.unlink()
                run('dpkg', '-i', packages[1])
                self.assertFalse(SOURCE.exists(), 'Preserve a source disabled by deletion')
            finally:
                run('dpkg', '--purge', 'silo', check=False)
                SOURCE.unlink(missing_ok=True)
                MARKER.unlink(missing_ok=True)


if __name__ == '__main__':
    unittest.main()
