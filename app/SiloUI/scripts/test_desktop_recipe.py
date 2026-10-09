"""Execute the guest desktop recipe in a temporary filesystem with fake OS tools.

These tests never install software, start a desktop, or modify host guest paths.
The shell's real branching and generated session script remain under test.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / 'src-tauri/guest/setup-desktop.sh'

# Package/account operations are boundaries of the shell recipe. Record each one
# and simulate only the filesystem effects later recipe steps depend on.
STUB = r'''
import json, os, pathlib, shutil, subprocess, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
root = pathlib.Path(os.environ['RECIPE_ROOT'])
with (root / 'calls.jsonl').open('a') as output:
    output.write(json.dumps([name, args]) + '\n')
if name == 'cat':
    if args and os.environ.get('V4_PACKAGE_READ_FAIL_ONCE') and not (root / 'package-read-failed').exists():
        (root / 'package-read-failed').touch()
        sys.exit(23)
    for argument in args:
        sys.stdout.write(pathlib.Path(argument).read_text())
    if not args:
        sys.stdout.write(sys.stdin.read())
elif name == 'dconf':
    if args == ['update']:
        database = root / 'etc/dconf/db/local'
        database.parent.mkdir(parents=True, exist_ok=True)
        database.write_text('compiled')
elif name == 'id':
    print('silo' if '-gn' in args else '0')
elif name == 'dpkg':
    if '--print-architecture' in args:
        print(os.environ.get('DPKG_ARCH', 'arm64'))
elif name == 'dpkg-query':
    if os.environ.get('PREINSTALLED_IMAGE') and ('-f=${Status}' in args or '-f=${Version}' in args):
        package = args[-1]
        if os.environ.get('V4_MISSING_PACKAGE') == package:
            sys.exit(1)
        if '-f=${Version}' in args:
            sys.stdout.write(os.environ.get('V4_SELKIES_VERSION', '2.0.0-1~ubuntu24.04'))
        else:
            sys.stdout.write('install ok installed')
    elif '-W' in args and len(args) == 1:
        print('xfce4-session\t4.18')
    elif 'greybird-gtk-theme' in args:
        if not (root / 'theme-installed').exists():
            sys.exit(1)
        print('install ok installed')
elif name == 'apt-get':
    if 'install' in args and 'greybird-gtk-theme' in args:
        (root / 'theme-installed').touch()
    if 'install' in args and any('selkies.deb' in arg for arg in args):
        if os.environ.get('SELKIES_INSTALL_FAIL'):
            sys.exit(31)
        binary = pathlib.Path(os.environ['SELKIES_BINARY'])
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_text('#!/bin/sh\nexit 0\n')
        binary.chmod(0o755)
elif name == 'python3':
    if args and args[0].endswith(('/patch-selkies-web-client.py', '/patch-selkies-display-scaling.py')):
        if os.environ.get('SELKIES_PATCH_FAIL'):
            sys.exit(32)
        if os.environ.get('SELKIES_PATCH_FAIL_ONCE') and not (root / 'patch-failed-once').exists():
            (root / 'patch-failed-once').touch()
            sys.exit(32)
    if args and args[0] in ('-', '-c'):
        source = sys.stdin.read() if args[0] == '-' else args[1]
        if args[0] == '-' and not any(marker in source for marker in
                                      ('SILO_STREAMER_LOCK_V1', 'SILO_STREAMER_RECEIPT_V1',
                                       'SILO_DESKTOP_CONNECTION_V1', 'SILO_GUEST_IMAGE_MARKER_V1')):
            sys.exit('unexpected Python recipe helper')
        result = subprocess.run([sys.executable, *args], input=source if args[0] == '-' else None,
                                 text=True, capture_output=True, env=os.environ)
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        sys.exit(result.returncode)
    elif args and args[0].endswith('/desktop-service.py') and args[1:] == ['prepare-install']:
        print('silo ' + str(root / 'home/silo'))
    elif args and args[0].endswith('/desktop-service.py') and args[1:] == ['status']:
        print(json.dumps({
            'state': os.environ.get('LEGACY_STATE', 'stopped'),
            'sessionState': os.environ.get('SESSION_STATE', 'stopped'),
            'streamState': os.environ.get('STREAM_STATE', 'stopped'),
        }))
elif name == 'install':
    if '-d' in args:
        skip = False
        for argument in args:
            if skip:
                skip = False
            elif argument in ('-m', '-o', '-g'):
                skip = True
            elif not argument.startswith('-'):
                pathlib.Path(argument).mkdir(parents=True, exist_ok=True)
    else:
        destination = pathlib.Path(args[-1])
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(args[-2], destination)
        destination.chmod(int(args[args.index('-m') + 1], 8))
elif name == 'curl':
    pathlib.Path(args[args.index('-o') + 1]).touch()
elif name == 'sha256sum':
    supplied = sys.stdin.read().split()
    if supplied and supplied[0] != os.environ.get('EXPECTED_STREAMER_SHA'):
        sys.exit('unexpected streamer package digest')
elif name == 'df':
    print('Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/test 9999999 0 9999999 0% /')
'''


RUNTIME_COMMANDS = ('xauth', 'Xvfb', 'pulseaudio', 'xfce4-session', 'xfwm4',
                    'dbus-run-session', 'runuser')
SHARED_PACKAGES = (SOURCE.parent / 'desktop-packages.txt').read_text().split()


class DesktopRecipe(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='silo-desktop-recipe-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        binaries = self.root / 'bin'
        binaries.mkdir()
        stub = '#!' + sys.executable + '\n' + STUB
        for name in ('id', 'dpkg', 'dpkg-query', 'apt-get', 'python3', 'install',
                     'curl', 'sha256sum', 'usermod', 'chown', 'flock', 'df', 'dconf', 'cat'):
            path = binaries / name
            path.write_text(stub)
            path.chmod(0o755)
        self.state = self.root / 'var/lib/silo-desktop'
        self.state.mkdir(parents=True)
        self.home = self.root / 'home/silo'
        (self.home / '.vnc').mkdir(parents=True)
        self.fixture = self.root / 'sources'
        self.fixture.mkdir()
        (self.fixture / 'desktop-service.py').write_text(stub)
        (self.fixture / 'patch-selkies-web-client.py').write_text('# fixture patcher\n')
        (self.fixture / 'patch-selkies-display-scaling.py').write_text('# fixture patcher\n')
        for shared in ('desktop-packages.txt', 'silo-accessibility.py'):
            (self.fixture / shared).write_text((SOURCE.parent / shared).read_text())
        streamer_lock = (SOURCE.parent / 'desktop-streamer-lock.json').read_text()
        (self.fixture / 'desktop-streamer-lock.json').write_text(streamer_lock)
        os_release = self.root / 'os-release'
        os_release.write_text('ID=ubuntu\nVERSION_ID=24.04\n')
        source = SOURCE.read_text().replace('/etc/os-release', str(os_release))
        # Rewrite only absolute guest roots, leaving shell logic unchanged. All
        # remaining mutating OS tools are explicit stubs above.
        for path in ('/var/lib/silo-desktop', '/usr/local', '/home/silo', '/run/silo-desktop'):
            source = source.replace(path, str(self.root) + path)
        source = source.replace('/usr/bin/selkies', str(self.root / 'usr/bin/selkies'))
        self.recipe = self.root / 'recipe.sh'
        self.recipe.write_text(source)
        self.env = dict(os.environ, PATH=str(binaries) + ':/usr/bin:/bin',
                        RECIPE_ROOT=str(self.root),
                        SILO_DESKTOP_SERVICE_SOURCE=str(self.fixture / 'desktop-service.py'),
                        SILO_SELKIES_WEB_CLIENT_PATCH_SOURCE=str(self.fixture / 'patch-selkies-web-client.py'),
                        SILO_SELKIES_DISPLAY_PATCH_SOURCE=str(self.fixture / 'patch-selkies-display-scaling.py'),
                        SELKIES_BINARY=str(self.root / 'usr/bin/selkies'),
                        EXPECTED_STREAMER_SHA='3900f3ba805898c495829629092553cc1cf4d5a864ffc4056f57d21646ad45e4')

    def run_recipe(self, action='install', env=None):
        result = subprocess.run(['/bin/sh', str(self.recipe), action], env=dict(self.env, **(env or {})),
                                text=True, capture_output=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return [json.loads(line) for line in (self.root / 'calls.jsonl').read_text().splitlines()]

    def assert_session_identity(self):
        session = self.home / '.vnc/xstartup'
        self.assertTrue(os.access(session, os.X_OK))
        # Run the generated script, replacing only its final desktop launch with
        # observation of the environment delivered to that launch.
        # /etc/xdg/autostart entries (the accessibility poller) only run inside
        # a session started by xfce4-session under its own D-Bus session.
        self.assertIn('exec dbus-run-session -- xfce4-session', session.read_text())
        script = session.read_text().replace('exec dbus-run-session -- xfce4-session',
                                             'printf "%s" "$XDG_CURRENT_DESKTOP"')
        result = subprocess.run(['/bin/sh', '-c', script], env={'PATH': '/usr/bin:/bin'},
                                text=True, capture_output=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'XFCE')

    def test_fresh_desktop_installs_theme_and_launches_identified_session(self):
        calls = self.run_recipe()
        self.assertTrue((self.root / 'theme-installed').exists())
        self.assert_session_identity()
        self.assertIn(['silo-desktop', ['boot']], calls)
        receipt_path = self.state / 'streamer.json'
        self.assertEqual(receipt_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(json.loads(receipt_path.read_text()), {
            'schemaVersion': 1, 'state': 'ready', 'backend': 'selkies',
            'version': '2.0.0', 'recipeVersion': 4, 'architecture': 'arm64',
            'packageSha256': self.env['EXPECTED_STREAMER_SHA'],
            'resolution': {'width': 1440, 'height': 900},
        })
        curl = next(args for name, args in calls if name == 'curl')
        self.assertIn('https://github.com/selkies-project/selkies/releases/download/2.0.0/selkies-2.0.0-ubuntu24.04-arm64.deb', curl)
        apt = [args for name, args in calls if name == 'apt-get']
        self.assertTrue(any('xvfb' in args and 'pulseaudio' in args for args in apt))
        self.assertFalse(any('kasmvncserver_noble' in arg or 'kasmvnc.deb' in arg
                             for args in apt + [curl] for arg in args))
        patch_call = ['python3', [str(self.fixture / 'patch-selkies-web-client.py'), 'arm64']]
        self.assertIn(patch_call, calls)
        display_call = ['python3', [str(self.fixture / 'patch-selkies-display-scaling.py'), 'arm64']]
        self.assertIn(display_call, calls)
        package_install = next(i for i, (name, args) in enumerate(calls)
                               if name == 'apt-get' and any('selkies.deb' in arg for arg in args))
        self.assertLess(package_install, calls.index(patch_call))
        connection_path = self.state / 'connection.json'
        self.assertEqual(connection_path.stat().st_mode & 0o777, 0o600)
        connection = json.loads(connection_path.read_text())
        self.assertEqual(connection['username'], 'silo')
        self.assertEqual(connection['port'], 6901)
        self.assertRegex(connection['password'], r'^[0-9a-f]{64}$')

    def test_fresh_desktop_installs_no_luda_tools(self):
        calls = self.run_recipe()
        self.assertFalse(any('luda' in ' '.join([name, *args]).lower() for name, args in calls))
        self.assertLess(0, calls.index(['silo-desktop', ['boot']]))

    def test_fresh_desktop_selects_amd64_streamer_asset_and_receipt(self):
        digest = 'bbaa4d71012b9374a753b7dfddc1da07e31f34b04277fe4fb3d045f18fd88391'
        calls = self.run_recipe(env={'DPKG_ARCH': 'amd64', 'EXPECTED_STREAMER_SHA': digest})
        curl = next(args for name, args in calls if name == 'curl')
        self.assertIn('https://github.com/selkies-project/selkies/releases/download/2.0.0/selkies-2.0.0-ubuntu24.04-amd64.deb', curl)
        receipt = json.loads((self.state / 'streamer.json').read_text())
        self.assertEqual(receipt['architecture'], 'amd64')
        self.assertEqual(receipt['recipeVersion'], 4)
        self.assertEqual(receipt['packageSha256'], digest)
        self.assertIn(['python3', [str(self.fixture / 'patch-selkies-web-client.py'), 'amd64']], calls)
        self.assertIn(['python3', [str(self.fixture / 'patch-selkies-display-scaling.py'), 'amd64']], calls)

    def test_existing_desktop_install_upgrades_session_and_theme(self):
        (self.state / 'installed.json').write_text('{"version":"1"}')
        for action in ('install',):
            with self.subTest(action=action):
                (self.home / '.vnc/xstartup').write_text('#!/bin/sh\nexec xfce4-session\n')
                (self.root / 'theme-installed').unlink(missing_ok=True)
                (self.root / 'calls.jsonl').unlink(missing_ok=True)
                calls = self.run_recipe(action)
                self.assertTrue((self.root / 'theme-installed').exists())
                self.assert_session_identity()
                self.assertFalse(any(name == 'curl' for name, _ in calls), 'Do not reinstall VNC')
                self.assertFalse((self.state / 'streamer.json').exists(),
                                 'Ordinary setup preserves the legacy Kasm backend')
                self.assertFalse(any(name == 'silo-desktop' and args in (['boot'], ['stop'])
                                     for name, args in calls), 'Preserve the running desktop')
                # Repeating setup on an upgraded desktop needs no package network.
                (self.root / 'calls.jsonl').unlink()
                calls = self.run_recipe(action)
                self.assertFalse(any(name == 'apt-get' for name, _ in calls))

    def test_update_streamer_requires_stopped_desktop_even_when_stream_failed(self):
        (self.state / 'installed.json').write_text('{"version":"1"}')
        result = subprocess.run(['/bin/sh', str(self.recipe), 'update-streamer'],
                                env=dict(self.env, SESSION_STATE='running',
                                         LEGACY_STATE='failed', STREAM_STATE='failed'),
                                text=True, capture_output=True, timeout=120)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Stop the desktop before updating its streamer', result.stderr)
        self.assertFalse((self.state / 'streamer.json').exists())
        self.assertFalse((self.root / 'usr/local/bin/silo-desktop').exists())
        calls = [json.loads(line) for line in (self.root / 'calls.jsonl').read_text().splitlines()]
        self.assertFalse(any(name in ('curl', 'apt-get') for name, _ in calls))

    def test_update_streamer_writes_receipt_after_pinned_install_without_restarting_desktop(self):
        (self.state / 'installed.json').write_text('{"version":"1"}')
        connection = {'username': 'silo', 'password': 'a' * 64, 'port': 6901}
        connection_path = self.state / 'connection.json'
        connection_path.write_text(json.dumps(connection))
        connection_path.chmod(0o600)
        calls = self.run_recipe('update-streamer')
        receipt = json.loads((self.state / 'streamer.json').read_text())
        self.assertEqual(receipt['backend'], 'selkies')
        self.assertEqual(receipt['recipeVersion'], 4)
        self.assertEqual(receipt['packageSha256'], self.env['EXPECTED_STREAMER_SHA'])
        self.assertEqual((self.state / 'streamer.json').stat().st_mode & 0o777, 0o600)
        self.assertTrue(any(name == 'apt-get' and any('selkies.deb' in arg for arg in args)
                            for name, args in calls))
        self.assertIn(['python3', [str(self.fixture / 'patch-selkies-web-client.py'), 'arm64']], calls)
        self.assertIn(['python3', [str(self.fixture / 'patch-selkies-display-scaling.py'), 'arm64']], calls)
        installed_helper = self.root / 'usr/local/bin/silo-desktop'
        self.assertEqual(installed_helper.read_text(), (self.fixture / 'desktop-service.py').read_text())
        self.assertEqual(installed_helper.stat().st_mode & 0o777, 0o755)
        self.assertFalse(any(name == 'silo-desktop' and args in (['boot'], ['start'], ['stop'], ['restart'])
                             for name, args in calls))
        self.assertEqual(json.loads(connection_path.read_text()), connection)

    def test_failed_selkies_package_install_does_not_write_receipt(self):
        (self.state / 'installed.json').write_text('{"version":"1"}')
        connection = {'username': 'silo', 'password': 'a' * 64, 'port': 6901}
        connection_path = self.state / 'connection.json'
        connection_path.write_text(json.dumps(connection))
        connection_path.chmod(0o600)
        old_receipt = {'schemaVersion': 1, 'state': 'ready', 'backend': 'selkies',
                       'version': '2.0.0', 'recipeVersion': 2, 'architecture': 'arm64',
                       'packageSha256': self.env['EXPECTED_STREAMER_SHA'],
                       'resolution': {'width': 1440, 'height': 900}}
        receipt_path = self.state / 'streamer.json'
        receipt_path.write_text(json.dumps(old_receipt))
        result = subprocess.run(['/bin/sh', str(self.recipe), 'update-streamer'],
                                env=dict(self.env, SELKIES_PATCH_FAIL='1'),
                                text=True, capture_output=True, timeout=120)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(receipt_path.read_text()), old_receipt)
        self.assertFalse((self.root / 'usr/local/bin/silo-desktop').exists())


class PreinstalledImageDesktop(DesktopRecipe):
    """v4 guest images already hold the desktop; only per-VM state is provisioned."""

    def setUp(self):
        super().setUp()
        self.env['PREINSTALLED_IMAGE'] = '1'
        marker = self.root / 'usr/local/share/silo/guest-image.json'
        marker.parent.mkdir(parents=True)
        self.write_marker(capabilities=['desktop', 'accessibility', 'lcu-system-packages'])
        binary = self.root / 'usr/bin/selkies'
        binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\n')
        binary.chmod(0o755)
        autostart = self.root / 'etc/xdg/autostart/silo-accessibility.desktop'
        autostart.parent.mkdir(parents=True)
        autostart.write_text('[Desktop Entry]\n')
        for command in RUNTIME_COMMANDS:
            path = self.root / 'bin' / command
            path.write_text('#!/bin/sh\nexit 0\n')
            path.chmod(0o755)
        helper = self.root / 'usr/local/libexec/silo-accessibility'
        helper.parent.mkdir(parents=True)
        helper.write_text('#!/bin/sh\n')
        helper.chmod(0o755)
        for name, content in (('profile/user', 'user-db:user\n'), ('db/local', 'compiled'),
                              ('db/local.d/00-silo-accessibility',
                               '[org/gnome/desktop/interface]\ntoolkit-accessibility=true\n')):
            path = self.root / 'etc/dconf' / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        recipe = self.recipe.read_text()
        for guest_path in ('/etc/xdg', '/etc/dconf'):
            recipe = recipe.replace(guest_path, str(self.root) + guest_path)
        self.recipe.write_text(recipe)

    def write_marker(self, **overrides):
        marker = {'schemaVersion': 1, 'version': 'ubuntu-24.04-v4', 'streamerVersion': '2.0.0',
                  'capabilities': ['desktop']}
        marker.update(overrides)
        (self.root / 'usr/local/share/silo/guest-image.json').write_text(json.dumps(marker))

    # The inherited v3 tests do not apply to the preinstalled path.
    test_fresh_desktop_installs_theme_and_launches_identified_session = None
    test_fresh_desktop_installs_no_luda_tools = None
    test_fresh_desktop_selects_amd64_streamer_asset_and_receipt = None
    test_existing_desktop_install_upgrades_session_and_theme = None
    test_update_streamer_requires_stopped_desktop_even_when_stream_failed = None
    test_update_streamer_writes_receipt_after_pinned_install_without_restarting_desktop = None
    test_failed_selkies_package_install_does_not_write_receipt = None

    def test_v4_image_provisions_state_without_apt_or_network(self):
        calls = self.run_recipe()
        names = [name for name, _ in calls]
        self.assertNotIn('apt-get', names)
        self.assertNotIn('curl', names)
        self.assertNotIn('df', names)
        self.assertIn(['python3', [str(self.fixture / 'patch-selkies-web-client.py'), 'arm64']], calls)
        self.assertIn(['python3', [str(self.fixture / 'patch-selkies-display-scaling.py'), 'arm64']], calls)
        self.assertIn(['silo-desktop', ['boot']], calls)
        self.assertEqual(json.loads((self.state / 'streamer.json').read_text())['version'], '2.0.0')
        self.assertEqual((self.state / 'streamer.json').stat().st_mode & 0o777, 0o600)
        connection = json.loads((self.state / 'connection.json').read_text())
        self.assertEqual((connection['username'], connection['port']), ('silo', 6901))
        self.assertRegex(connection['password'], r'^[0-9a-f]{64}$')
        self.assertEqual(json.loads((self.state / 'installed.json').read_text())['image'], 'preinstalled')
        self.assertTrue((self.state / 'packages.txt').exists())
        self.assertEqual((self.state / 'install-stage').read_text().strip(), 'installed')
        self.assertTrue((self.root / 'usr/local/bin/silo-desktop').exists())
        self.assertTrue(os.access(self.root / 'usr/local/libexec/silo-desktop-boot', os.X_OK))
        self.assert_session_identity()

    def test_v4_amd64_receipt_uses_the_build_architecture(self):
        digest = 'bbaa4d71012b9374a753b7dfddc1da07e31f34b04277fe4fb3d045f18fd88391'
        self.run_recipe(env={'DPKG_ARCH': 'amd64', 'EXPECTED_STREAMER_SHA': digest})
        receipt = json.loads((self.state / 'streamer.json').read_text())
        self.assertEqual((receipt['architecture'], receipt['packageSha256']), ('amd64', digest))

    def test_v4_rerun_is_an_offline_session_refresh(self):
        self.run_recipe()
        (self.root / 'calls.jsonl').unlink()
        (self.home / '.vnc/xstartup').write_text('#!/bin/sh\nexec xfce4-session\n')
        calls = self.run_recipe()
        self.assertFalse(any(name in ('apt-get', 'curl') for name, _ in calls))
        self.assert_session_identity()

    def run_fallback(self, env=None, action='install', v4=True):
        result = subprocess.run(['/bin/sh', str(self.recipe), action], env=dict(self.env, **(env or {})),
                                text=True, capture_output=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = [json.loads(line) for line in (self.root / 'calls.jsonl').read_text().splitlines()]
        installs = [args for name, args in calls
                    if name == 'apt-get' and 'install' in args and not any('selkies.deb' in a for a in args)]
        self.assertEqual(len(installs), 1)
        self.assertTrue(any(name == 'curl' for name, _ in calls))
        self.assertEqual(json.loads((self.state / 'installed.json').read_text()).get('image'), None)
        defaults = self.root / 'etc/xdg/mimeapps.list'
        if v4:
            # The complete v4 package set and accessibility defaults are restored.
            for package in SHARED_PACKAGES:
                self.assertIn(package, installs[0])
            self.assertNotIn('mousepad', installs[0])
            for package in ('gnome-text-editor', 'python3-pyatspi', 'dconf-cli'):
                self.assertIn(package, installs[0])
            self.assertEqual((self.root / 'usr/local/libexec/silo-accessibility').read_text(),
                             (SOURCE.parent / 'silo-accessibility.py').read_text())
            self.assertIn('Exec=' + str(self.root) + '/usr/local/libexec/silo-accessibility',
                          (self.root / 'etc/xdg/autostart/silo-accessibility.desktop').read_text())
            self.assertIn('toolkit-accessibility=true',
                          (self.root / 'etc/dconf/db/local.d/00-silo-accessibility').read_text())
            self.assertEqual((self.root / 'etc/dconf/profile/user').read_text(), 'user-db:user\nsystem-db:local\n')
            self.assertIn(['dconf', ['update']], calls)
            self.assertIn('text/plain=org.gnome.TextEditor.desktop', defaults.read_text())
        else:
            self.assertIn('mousepad', installs[0])
            self.assertNotIn('gnome-text-editor', installs[0])
            self.assertFalse(defaults.exists())
        return result.stderr

    def test_v4_missing_package_falls_back_to_the_full_install_with_a_message(self):
        stderr = self.run_fallback({'V4_MISSING_PACKAGE': 'gnome-text-editor'})
        self.assertIn('guest image package gnome-text-editor is missing', stderr)

    def test_v4_unreadable_package_list_cannot_skip_package_verification(self):
        stderr = self.run_fallback({'V4_PACKAGE_READ_FAIL_ONCE': '1'})
        self.assertIn('desktop package list is unreadable', stderr)

    def test_v4_wrong_streamer_version_falls_back(self):
        stderr = self.run_fallback({'V4_SELKIES_VERSION': '1.6.2-1'})
        self.assertIn('does not contain Selkies 2.0.0', stderr)

    def test_v4_marker_for_another_streamer_or_unreadable_falls_back(self):
        self.write_marker(streamerVersion='1.0.0')
        self.assertIn('different desktop streamer', self.run_fallback())
        for name in ('calls.jsonl', 'theme-installed'):
            (self.root / name).unlink(missing_ok=True)
        for path in self.state.iterdir():
            path.unlink()
        (self.root / 'usr/local/share/silo/guest-image.json').write_text('not json')
        self.assertIn('marker is unreadable', self.run_fallback())

    def test_image_without_desktop_capability_installs_silently_like_v3(self):
        self.write_marker(capabilities=['something-else'])
        stderr = self.run_fallback(v4=False)
        self.assertNotIn('guest image', stderr)

    def test_v4_missing_runtime_command_falls_back(self):
        tools = self.root / 'fallback-tools'
        tools.mkdir()
        for name in ('awk', 'cat', 'chmod', 'dirname', 'mkdir', 'mv', 'rm'):
            executable = shutil.which(name, path='/usr/bin:/bin')
            self.assertIsNotNone(executable, name)
            (tools / name).symlink_to(executable)
        # Keep the recipe's real command lookup, without host desktop binaries.
        self.env['PATH'] = str(self.root / 'bin') + ':' + str(tools)
        (self.root / 'bin/xauth').unlink()
        stderr = self.run_fallback()
        self.assertIn('missing xauth', stderr)

    def test_v4_missing_accessibility_helper_falls_back_and_restores_it(self):
        (self.root / 'usr/local/libexec/silo-accessibility').unlink()
        self.assertIn('missing the accessibility helper', self.run_fallback())

    def test_v4_missing_dconf_database_falls_back(self):
        (self.root / 'etc/dconf/db/local').unlink()
        self.assertIn('missing the accessibility settings', self.run_fallback())

    def test_v4_damaged_selkies_client_falls_back_and_reinstalls_the_streamer(self):
        stderr = self.run_fallback({'SELKIES_PATCH_FAIL_ONCE': '1'})
        self.assertIn('web client is missing or damaged', stderr)
        calls = [json.loads(line) for line in (self.root / 'calls.jsonl').read_text().splitlines()]
        self.assertTrue(any(name == 'apt-get' and '--reinstall' in args and any('selkies.deb' in a for a in args)
                            for name, args in calls))

    def test_explicit_rerun_revalidates_and_repairs_a_preinstalled_guest(self):
        self.run_recipe()
        connection = (self.state / 'connection.json').read_text()
        (self.root / 'calls.jsonl').unlink()
        # A healthy rerun stays an offline refresh; a removed package is repaired.
        self.assertFalse(any(name in ('apt-get', 'curl') for name, _ in self.run_recipe()))
        (self.root / 'calls.jsonl').unlink()
        stderr = self.run_fallback({'V4_MISSING_PACKAGE': 'xvfb'})
        self.assertIn('guest image package xvfb is missing', stderr)
        self.assertEqual((self.state / 'connection.json').read_text(), connection)

    def test_rerun_on_an_older_recipe_receipt_patches_selkies_before_the_new_helper(self):
        (self.state / 'installed.json').write_text('{"version":"1"}')
        (self.state / 'streamer.json').write_text(json.dumps({'recipeVersion': 3}))
        calls = self.run_recipe()
        patchers = [['python3', [str(self.fixture / name), 'arm64']] for name in (
            'patch-selkies-web-client.py', 'patch-selkies-display-scaling.py')]
        for patcher in patchers:
            self.assertIn(patcher, calls)
        self.assertEqual((self.root / 'usr/local/bin/silo-desktop').read_text(),
                         (self.fixture / 'desktop-service.py').read_text())
        self.assertFalse(any(name in ('apt-get', 'curl') for name, _ in calls))

    def test_rerun_with_a_failing_display_patch_keeps_the_previous_helper(self):
        (self.state / 'installed.json').write_text('{"version":"1"}')
        (self.state / 'streamer.json').write_text(json.dumps({'recipeVersion': 3}))
        helper = self.root / 'usr/local/bin/silo-desktop'
        helper.parent.mkdir(parents=True)
        helper.write_text('previous helper\n')
        result = subprocess.run(['/bin/sh', str(self.recipe), 'install'], env=dict(self.env, SELKIES_PATCH_FAIL='1'),
                                text=True, capture_output=True, timeout=120)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(helper.read_text(), 'previous helper\n')

    def test_legacy_install_without_streamer_receipt_is_only_refreshed(self):
        (self.state / 'installed.json').write_text('{"version":"1"}')
        calls = self.run_recipe(env={'V4_MISSING_PACKAGE': 'xvfb'})
        self.assertFalse(any(name in ('apt-get', 'curl') for name, _ in calls))


if __name__ == '__main__':
    unittest.main()
