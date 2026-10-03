#!/usr/bin/python3
"""Guest-only desktop lifecycle. No harness or host credentials are involved."""
import fcntl
import base64
import json
import os
import pwd
import re
from pathlib import Path
import secrets
import signal
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

STATE = Path('/var/lib/silo-desktop')
RUN = Path('/run/silo-desktop')
USER = 'silo'
HOME = Path('/home/silo')
SELF = '/usr/local/bin/silo-desktop'
LOG = Path('/var/log/silo-desktop.log')
WORKING_ACCOUNT = Path('/var/lib/silo/working-account.json')
SELKIES_EXECUTABLE = Path('/usr/bin/selkies')
LCU_RECEIPT = STATE / 'lcu.json'
LCU_APP = Path('/usr/lib/chatgpt')
LCU_PREFIX = Path('/opt/lcu')
TMP = Path('/tmp')
DISPLAY_LOCK = TMP / '.X1-lock'
DISPLAY_SOCKET_DIR = TMP / '.X11-unix'
# The desktop starts at DESKTOP_START_SIZE. Xvfb is created with XVFB_SCREEN as its
# largest size so RandR can grow the screen to follow a viewer window.
DESKTOP_START_SIZE = (1440, 900)
XVFB_SCREEN = '4096x4096x24'
# CVT reduced-blanking timings for 1440x900 at 60 Hz, used when Xvfb lists no such mode.
START_MODELINE = ['88.75', '1440', '1488', '1520', '1600', '900', '903', '909', '926',
                  '+hsync', '-vsync']
XRANDR_READY_SECONDS = 10
XRANDR_TIMEOUT_SECONDS = 10
# Receipt revisions the service still runs; only the newest is current.
STREAMER_RECIPE_VERSIONS = (1, 2, 3)
STREAMER_CURRENT_RECIPE = 3
SELKIES_MAX_ATTEMPTS = 3
# A stream that stays up this long starts a fresh retry budget, so unrelated
# crashes hours apart never add up to a failed display.
SELKIES_STABLE_SECONDS = 60
# Retry n (n >= 1) waits SELKIES_RETRY_BASE_SECONDS * 2 ** (n - 1) seconds.
SELKIES_RETRY_BASE_SECONDS = 2
SELKIES_RETRY_MAX_SECONDS = 30
# The session processes (Xvfb, PulseAudio, Xfce) are started together. A launch that
# fails tears the partial session down and retries; retry n waits
# SESSION_RETRY_BASE_SECONDS * 2 ** (n - 1) seconds.
SESSION_MAX_ATTEMPTS = 3
SESSION_RETRY_BASE_SECONDS = 1
BOOT_ID = Path('/proc/sys/kernel/random/boot_id')


def validate_policy_file(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
        raise RuntimeError('Working account policy must be a root-owned regular file without group or other write access')


def desktop_account():
    try:
        WORKING_ACCOUNT.lstat()
    except FileNotFoundError:
        raise RuntimeError('Desktop requires the Silo working account; migrate this computer or create a new computer') from None
    validate_policy_file(WORKING_ACCOUNT)
    policy = json.loads(WORKING_ACCOUNT.read_text())
    if policy != dict(schemaVersion=1, user='silo', home='/home/silo'):
        raise RuntimeError('Unsupported Silo working account policy')
    try:
        account = pwd.getpwnam('silo')
    except KeyError:
        raise RuntimeError('Silo working account is missing') from None
    if account.pw_uid != 1001 or account.pw_gid != 1001 or account.pw_dir != '/home/silo':
        raise RuntimeError('Silo working account has an unexpected UID or home')
    return 'silo', Path(account.pw_dir)


def prepare_configuration(home):
    # Only files claimed before an installation attempt may be replaced on retry.
    # The account's unrelated files and directory permissions remain untouched.
    managed = read('configuration-managed.json')
    if managed is not None and managed != {'home': str(home)}:
        raise RuntimeError('Desktop configuration belongs to a different home')
    directory = home / '.vnc'
    paths = [directory / 'kasmvnc.yaml', directory / 'xstartup', home / '.kasmpasswd']
    if home.is_symlink() or directory.is_symlink() or any(path.is_symlink() for path in paths):
        raise RuntimeError('Desktop configuration paths must not be symbolic links')
    if directory.exists() and not directory.is_dir():
        raise RuntimeError('Desktop configuration directory is not a directory')
    if managed is None:
        if any(path.exists() for path in paths):
            raise RuntimeError('Existing desktop configuration conflicts with the Silo desktop; preserve or move it before installing')
        write(STATE / 'configuration-managed.json', {'home': str(home)})


def read(name, default=None):
    try:
        return json.loads((STATE / name).read_text())
    except FileNotFoundError:
        return default


def write(path, value):
    fd, temporary = tempfile.mkstemp(prefix=f'.{path.name}-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(json.dumps(value) + '\n')
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def identity(pid):
    try:
        # Field 22, accounting for spaces in the parenthesized process name.
        return (BOOT_ID.read_text().strip() + ':' +
                Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[19])
    except (FileNotFoundError, ProcessLookupError):
        return None


def current_boot_id():
    try:
        return BOOT_ID.read_text().strip()
    except OSError:
        return None


def managed_process_identity(pid):
    try:
        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        return dict(startTicks=int(fields[19]), pgid=int(fields[2]),
                    uid=Path(f'/proc/{pid}').stat().st_uid,
                    exe=os.readlink(f'/proc/{pid}/exe'))
    except (FileNotFoundError, ProcessLookupError, PermissionError, OSError, ValueError, IndexError):
        return None


def managed_process_matches(record):
    if not isinstance(record, dict):
        return False
    current = managed_process_identity(record.get('pid'))
    return bool(current and all(current.get(key) == record.get(key)
                                for key in ('startTicks', 'pgid', 'uid', 'exe')) and
                record.get('pgid') == record.get('pid'))


def stop_managed_process(record):
    if not managed_process_matches(record):
        return
    os.killpg(record['pgid'], signal.SIGTERM)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if not managed_process_identity(record['pid']):
            return
        time.sleep(0.1)
    if managed_process_identity(record['pid']):
        raise RuntimeError(f"{record['name']} did not stop cleanly; no forced kill attempted")


def supervisor():
    try:
        saved = json.loads((RUN / 'supervisor.json').read_text())
        return saved['pid'] if identity(saved['pid']) == saved['start'] else None
    except FileNotFoundError:
        return None


def listening():
    return port_listening(6901)


def legacy_display_present():
    """Fail closed when a Kasm :1 display may outlive its supervisor."""
    if port_listening(5901):
        return True
    for path in (DISPLAY_LOCK, DISPLAY_SOCKET_DIR / 'X1'):
        try:
            path.lstat()
            return True
        except FileNotFoundError:
            continue
        except OSError:
            return True
    return False


def streamer_backend():
    """Return kasm, selkies, or None for an unusable installed-recipe receipt."""
    receipt_path = STATE / 'streamer.json'
    try:
        info = receipt_path.lstat()
    except FileNotFoundError:
        return 'kasm'
    except OSError:
        return None
    try:
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or
                info.st_mode & 0o022):
            return None
        receipt = json.loads(receipt_path.read_text())
        machine = {'x86_64': 'amd64', 'aarch64': 'arm64'}.get(os.uname().machine)
        resolution = receipt.get('resolution') if isinstance(receipt, dict) else None
        if (not isinstance(receipt, dict) or type(receipt.get('schemaVersion')) is not int or
                receipt.get('schemaVersion') != 1 or
                receipt.get('state') != 'ready' or receipt.get('backend') != 'selkies' or
                receipt.get('version') != '2.0.0' or type(receipt.get('recipeVersion')) is not int or
                receipt.get('recipeVersion') not in STREAMER_RECIPE_VERSIONS or
                receipt.get('architecture') != machine or
                not isinstance(receipt.get('packageSha256'), str) or
                not re.fullmatch(r'[0-9a-f]{64}', receipt['packageSha256']) or
                resolution != {'width': DESKTOP_START_SIZE[0], 'height': DESKTOP_START_SIZE[1]} or
                not SELKIES_EXECUTABLE.is_file() or not os.access(SELKIES_EXECUTABLE, os.X_OK)):
            return None
        return 'selkies'
    except (OSError, ValueError, TypeError, AttributeError):
        return None


def streamer_recipe_version():
    """Return the validated Selkies recipe revision, or zero when unavailable."""
    try:
        value = json.loads((STATE / 'streamer.json').read_text())
        revision = value.get('recipeVersion')
        return revision if type(revision) is int and revision in STREAMER_RECIPE_VERSIONS else 0
    except (OSError, ValueError, TypeError, AttributeError):
        return 0


def selkies_state():
    try:
        return json.loads((RUN / 'selkies.json').read_text())
    except FileNotFoundError:
        return None
    except (OSError, ValueError):
        return None


def write_selkies_state(value):
    write(RUN / 'selkies.json', value)


def port_listening(port):
    try:
        with socket.create_connection(('127.0.0.1', port), timeout=0.3):
            return True
    except OSError:
        return False


def process_exists(pid):
    try:
        os.stat(f'/proc/{pid}')
        return True
    except FileNotFoundError:
        return False


def verified_stale_display_pair(account):
    """Return identities for an inactive Kasm :1 lock/socket pair, or None."""
    lock = DISPLAY_LOCK
    display_socket = DISPLAY_SOCKET_DIR / 'X1'
    try:
        lock_info = lock.lstat()
        socket_info = display_socket.lstat()
        if (not stat.S_ISREG(lock_info.st_mode) or lock_info.st_uid != account.pw_uid or
                not stat.S_ISSOCK(socket_info.st_mode) or socket_info.st_uid != account.pw_uid):
            return None
        flags = os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0)
        fd = os.open(lock, flags)
        try:
            opened = os.fstat(fd)
            if (not stat.S_ISREG(opened.st_mode) or opened.st_uid != account.pw_uid or
                    (opened.st_dev, opened.st_ino) != (lock_info.st_dev, lock_info.st_ino)):
                return None
            contents = os.read(fd, 64).decode('ascii').strip()
        finally:
            os.close(fd)
        if not re.fullmatch(r'[0-9]+', contents):
            return None
        pid = int(contents)
        if pid <= 1 or process_exists(pid) or port_listening(5901) or listening():
            return None
        return lock_info, socket_info, pid
    except (FileNotFoundError, PermissionError, OSError, ValueError, UnicodeError):
        return None


def prepare_display_socket_directory():
    """Establish root-owned sticky X11 socket storage before starting Kasm."""
    root_uid = os.geteuid()
    if root_uid != 0:
        return False
    try:
        account = pwd.getpwnam(USER)
        tmp_info = TMP.lstat()
        if (not stat.S_ISDIR(tmp_info.st_mode) or stat.S_ISLNK(tmp_info.st_mode) or
                tmp_info.st_uid != root_uid or not tmp_info.st_mode & stat.S_ISVTX or
                port_listening(5901) or listening()):
            return False
        try:
            DISPLAY_SOCKET_DIR.mkdir(mode=0o1777)
        except FileExistsError:
            pass

        flags = os.O_RDONLY | getattr(os, 'O_DIRECTORY', 0) | getattr(os, 'O_NOFOLLOW', 0)
        fd = os.open(DISPLAY_SOCKET_DIR, flags)
        try:
            opened = os.fstat(fd)
            current = DISPLAY_SOCKET_DIR.lstat()
            if (not stat.S_ISDIR(opened.st_mode) or stat.S_ISLNK(current.st_mode) or
                    (opened.st_dev, opened.st_ino) != (current.st_dev, current.st_ino)):
                return False
            entries = set(os.listdir(fd))
            owner = opened.st_uid
            mode = stat.S_IMODE(opened.st_mode)
            if owner == root_uid and mode == 0o1777:
                return True
            if owner not in (root_uid, account.pw_uid):
                return False

            if entries:
                stale_pair = verified_stale_display_pair(account)
                if entries != {'X1'} or stale_pair is None:
                    return False
            else:
                try:
                    DISPLAY_LOCK.lstat()
                    # A lock without its socket is ambiguous; never adopt it.
                    return False
                except FileNotFoundError:
                    pass

            # Do not change ownership or permissions while a display can be live.
            if port_listening(5901) or listening():
                return False
            os.fchown(fd, root_uid, 0)
            os.fchmod(fd, 0o1777)
            normalized = os.fstat(fd)
            current = DISPLAY_SOCKET_DIR.lstat()
            return (stat.S_ISDIR(normalized.st_mode) and normalized.st_uid == root_uid and
                    stat.S_IMODE(normalized.st_mode) == 0o1777 and
                    (normalized.st_dev, normalized.st_ino) == (current.st_dev, current.st_ino))
        finally:
            os.close(fd)
    except (FileNotFoundError, PermissionError, OSError, ValueError):
        return False


def remove_stale_display_artifacts():
    """Remove only Kasm's dead :1 lock/socket pair before one startup retry."""
    lock = DISPLAY_LOCK
    socket_dir = DISPLAY_SOCKET_DIR
    display_socket = socket_dir / 'X1'
    try:
        account = pwd.getpwnam(USER)
        root_uid = os.geteuid()
        tmp_info = TMP.lstat()
        dir_info = socket_dir.lstat()
        if (not stat.S_ISDIR(tmp_info.st_mode) or tmp_info.st_uid != root_uid or
                not tmp_info.st_mode & stat.S_ISVTX or
                not stat.S_ISDIR(dir_info.st_mode) or dir_info.st_uid != root_uid or
                stat.S_ISLNK(dir_info.st_mode)):
            return False
        stale_pair = verified_stale_display_pair(account)
        if stale_pair is None:
            return False
        lock_info, socket_info, pid = stale_pair

        # Recheck identities immediately before unlinking; never follow paths
        # supplied by the computer or remove anything beyond this display's pair.
        current_lock = lock.lstat()
        current_socket = display_socket.lstat()
        if ((current_lock.st_dev, current_lock.st_ino) != (lock_info.st_dev, lock_info.st_ino) or
                (current_socket.st_dev, current_socket.st_ino) != (socket_info.st_dev, socket_info.st_ino) or
                not stat.S_ISREG(current_lock.st_mode) or current_lock.st_uid != account.pw_uid or
                not stat.S_ISSOCK(current_socket.st_mode) or current_socket.st_uid != account.pw_uid or
                process_exists(pid) or port_listening(5901) or listening()):
            return False
        lock.unlink()
        display_socket.unlink()
        return True
    except (FileNotFoundError, PermissionError, OSError, ValueError, UnicodeError):
        return False


def lcu_status():
    """Project only the installed LCU receipt and required runtime paths."""
    result = dict(lcuState=None, lcuReason=None, lcuVersion=None,
                  lcuAppVersion=None, lcuRuntimeVersion=None,
                  lcuAgents=None, lcuReadiness=None)

    def project(receipt, state, reason=None, readiness=None):
        result.update(lcuState=state, lcuReason=reason,
                      lcuVersion=safe_lcu_version(receipt.get('version')),
                      lcuAppVersion=safe_lcu_version(receipt.get('appVersion')),
                      lcuRuntimeVersion=safe_lcu_runtime_version(receipt.get('runtimeVersion')),
                      lcuAgents=sorted({agent for agent in receipt.get('agents', [])
                                        if agent in ('pi', 'codex', 'claude-code')})
                      if isinstance(receipt.get('agents'), list) else None,
                      lcuReadiness=readiness)

    try:
        app_info = LCU_APP.lstat()
        official_app = stat.S_ISDIR(app_info.st_mode) and not stat.S_ISLNK(app_info.st_mode)
    except OSError:
        official_app = False
    if not official_app:
        project({}, 'needs-runtime', 'chatgpt-app-required', 'unverified')
        return result

    try:
        info = LCU_RECEIPT.lstat()
    except FileNotFoundError:
        project({}, 'not-installed', readiness='unverified')
        return result
    except OSError:
        project({}, 'repair-required', 'invalid-receipt', 'failed')
        return result
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022):
        project({}, 'repair-required', 'invalid-receipt', 'failed')
        return result
    try:
        receipt = json.loads(LCU_RECEIPT.read_text())
    except (OSError, ValueError):
        project({}, 'repair-required', 'invalid-receipt', 'failed')
        return result
    if not isinstance(receipt, dict) or receipt.get('schemaVersion') != 1:
        project({}, 'repair-required', 'unsupported-receipt', 'failed')
        return result
    if receipt.get('status') == 'failed':
        reason = receipt.get('reason')
        if reason not in ('chatgpt-app-required', 'invalid-receipt', 'unsupported-receipt',
                          'managed-runtime-missing', 'setup-failed'):
            reason = 'setup-failed'
        project(receipt, 'failed', reason, 'failed')
        return result
    if receipt.get('status') == 'installing':
        project(receipt, 'installing', readiness='unverified')
        return result
    current = LCU_PREFIX / 'current'
    runtime = current / 'bin/lcu'
    managed_app = current / 'app'
    if (receipt.get('status') != 'ready' or not current.is_symlink() or
            not runtime.is_file() or not os.access(runtime, os.X_OK) or
            not managed_app.is_dir()):
        project(receipt, 'repair-required', 'managed-runtime-missing', 'failed')
        return result
    readiness = receipt.get('readiness')
    if readiness not in ('ready', 'unverified', 'failed'):
        readiness = 'unverified'
    project(receipt, 'ready', readiness=readiness)
    return result


def safe_lcu_version(value):
    return value if isinstance(value, str) and len(value) <= 64 and re.fullmatch(r'[0-9A-Za-z.+_-]+', value) else None


def safe_lcu_runtime_version(value):
    return value if isinstance(value, str) and len(value) <= 64 and re.fullmatch(
        r'[0-9]+\.[0-9]+\.[0-9]+/[0-9]{14}-[0-9a-f]{12}', value) else None


def selkies_http_ready():
    try:
        connection = read('connection.json')
        if (not isinstance(connection, dict) or connection.get('username', USER) != USER or
                connection.get('port') != 6901 or not isinstance(connection.get('password'), str)):
            return False
        token = base64.b64encode(f"{USER}:{connection['password']}".encode()).decode()
        request = urllib.request.Request('http://127.0.0.1:6901/',
                                         headers={'Authorization': f'Basic {token}'})
        with urllib.request.urlopen(request, timeout=1) as response:
            return (response.status == 200 and
                    'text/html' in response.headers.get('Content-Type', '').lower() and
                    bool(response.read(4096)))
    except (OSError, ValueError, KeyError, urllib.error.URLError):
        return False


def sleep_until_service_event(seconds):
    time.sleep(seconds)


def account_demoter(account):
    def demote():
        os.initgroups(USER, account.pw_gid)
        os.setgid(account.pw_gid)
        os.setuid(account.pw_uid)
    return demote


def launch_managed_process(name, argv, environment, account):
    demote = account_demoter(account)
    with LOG.open('ab', buffering=0) as output:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=output,
                                 stderr=output, env=environment, start_new_session=True,
                                 preexec_fn=demote, close_fds=True)
    time.sleep(0.2)
    current = managed_process_identity(child.pid)
    if (child.poll() is not None or not current or current['uid'] != account.pw_uid or
            current['pgid'] != child.pid):
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
            child.wait(timeout=5)
        raise RuntimeError(f'{name} exited or failed identity validation; inspect {LOG}')
    return child, dict(name=name, pid=child.pid, **current)


def run_xrandr(arguments, environment, account):
    try:
        result = subprocess.run(['xrandr', *arguments], env=environment, capture_output=True,
                                text=True, timeout=XRANDR_TIMEOUT_SECONDS,
                                preexec_fn=account_demoter(account), close_fds=True)
    except (OSError, subprocess.SubprocessError) as error:
        raise RuntimeError(f'xrandr could not run: {error}') from None
    return result


def xrandr_screen(environment, account):
    """Return (output, current size, listed mode names) from `xrandr --query`, or None."""
    result = run_xrandr(['--query'], environment, account)
    if result.returncode != 0:
        return None
    current = re.search(r'current (\d+) x (\d+)', result.stdout)
    output = re.search(r'^(\S+) connected', result.stdout, re.MULTILINE)
    if not current or not output:
        return None
    modes = set(re.findall(r'^\s+(\d+x\d+)\s', result.stdout, re.MULTILINE))
    return output.group(1), (int(current.group(1)), int(current.group(2))), modes


def set_desktop_start_size(environment, account):
    """Shrink the freshly started Xvfb screen to DESKTOP_START_SIZE through RandR."""
    width, height = DESKTOP_START_SIZE
    name = f'{width}x{height}'
    screen = None
    for _ in range(XRANDR_READY_SECONDS * 5):
        screen = xrandr_screen(environment, account)
        if screen:
            break
        time.sleep(0.2)
    if not screen:
        raise RuntimeError('Xvfb did not report a RandR output; check that xrandr and the RANDR extension are available')
    output, current, modes = screen
    if current != DESKTOP_START_SIZE:
        if name not in modes:
            created = run_xrandr(['--newmode', name, *START_MODELINE], environment, account)
            added = run_xrandr(['--addmode', output, name], environment, account)
            if added.returncode != 0:
                raise RuntimeError(f'xrandr could not add the {name} mode: '
                                   f'{(added.stderr or created.stderr).strip()}')
        applied = run_xrandr(['--output', output, '--mode', name, '--fb', name], environment, account)
        if applied.returncode != 0:
            raise RuntimeError(f'xrandr could not set the {name} screen size: {applied.stderr.strip()}')
        screen = xrandr_screen(environment, account)
        if not screen or screen[1] != DESKTOP_START_SIZE:
            raise RuntimeError(f'The desktop screen did not change to {name}')


def stop_managed_child(child):
    if child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            raise RuntimeError('Desktop service child did not stop cleanly; no forced kill attempted') from None


def launch_selkies_streamer(account, environment):
    connection = read('connection.json')
    if (not isinstance(connection, dict) or connection.get('username') != USER or
            connection.get('port') != 6901 or not isinstance(connection.get('password'), str) or
            not re.fullmatch(r'[0-9a-f]{64}', connection['password'])):
        raise RuntimeError('Desktop connection credentials are invalid')
    argv = [str(SELKIES_EXECUTABLE), '--addr=127.0.0.1', '--port=6901',
            '--enable-https=false', f'--basic-auth-user={USER}',
            f"--basic-auth-password={connection['password']}", '--encoder=h264enc',
            '--use-cpu=true', '--mode=websockets', '--enable-dual-mode=false|locked',
            '--enable-resize=true', '--use-css-scaling=true|locked',
            '--enable-clipboard=true', '--enable-binary-clipboard=true',
            '--clipboard-seamless=false', '--file-transfers=none',
            '--audio-enabled=true', '--audio-bitrate=64000',
            '--microphone-enabled=false|locked', '--ui-sidebar-show-audio-settings=false']
    return launch_managed_process('selkies', argv, environment, account)


def wait_before_stream_retry(failures, stopping, restart_requested):
    """Back off before retry number `failures`; True when a restart request arrived."""
    delay = min(SELKIES_RETRY_BASE_SECONDS * 2 ** (failures - 1), SELKIES_RETRY_MAX_SECONDS)
    for _ in range(int(delay / 0.5)):
        if stopping():
            return False
        if restart_requested():
            return True
        sleep_until_service_event(0.5)
    return False


def supervise_selkies_stream(state, account, environment, stopping, restart_requested):
    """Supervise only Selkies. Exhaustion never changes session processes or state."""
    attempts = 0
    while not stopping():
        if restart_requested():
            attempts = 0
        if 0 < attempts < SELKIES_MAX_ATTEMPTS:
            if wait_before_stream_retry(attempts, stopping, restart_requested):
                attempts = 0
            if stopping():
                continue
        if attempts >= SELKIES_MAX_ATTEMPTS:
            state['streamState'] = 'failed'
            state['streamProcess'] = None
            write_selkies_state(state)
            while not stopping():
                if restart_requested():
                    attempts = 0
                    state['streamState'] = 'starting'
                    write_selkies_state(state)
                    break
                sleep_until_service_event(0.5)
            continue

        attempts += 1
        state['streamAttempts'] = attempts
        state['streamState'] = 'starting'
        state['streamProcess'] = None
        write_selkies_state(state)
        try:
            child, record = launch_selkies_streamer(account, environment)
        except (OSError, RuntimeError, subprocess.CalledProcessError):
            continue
        state['streamProcess'] = record
        write_selkies_state(state)
        deadline = time.monotonic() + 15
        restarted = False
        while child.poll() is None and not stopping():
            if restart_requested():
                restarted = True
                stop_managed_child(child)
                break
            if selkies_http_ready():
                state['streamState'] = 'running'
                write_selkies_state(state)
                break
            try:
                child.wait(timeout=0.2)
            except subprocess.TimeoutExpired:
                pass
            if time.monotonic() >= deadline and child.poll() is None:
                stop_managed_child(child)
                break

        if stopping():
            stop_managed_child(child)
            state['streamProcess'] = None
            state['streamState'] = 'stopped'
            write_selkies_state(state)
            return
        if restarted:
            state['streamProcess'] = None
            state['streamState'] = 'starting'
            attempts = 0
            write_selkies_state(state)
            continue

        running_since = time.monotonic()
        while child.poll() is None and not stopping():
            if restart_requested():
                restarted = True
                stop_managed_child(child)
                break
            try:
                child.wait(timeout=0.5)
            except subprocess.TimeoutExpired:
                pass
            if attempts and time.monotonic() - running_since >= SELKIES_STABLE_SECONDS:
                attempts = 0
        if stopping():
            stop_managed_child(child)
            state['streamProcess'] = None
            state['streamState'] = 'stopped'
            write_selkies_state(state)
            return
        state['streamProcess'] = None
        if restarted:
            state['streamState'] = 'starting'
            attempts = 0
        else:
            state['streamState'] = 'starting'
        write_selkies_state(state)


def stop_selkies_processes(state, children_by_pid=None):
    children_by_pid = children_by_pid or {}
    if state.get('bootId') != current_boot_id():
        state.update(sessionState='stopped', sessionProcesses=[],
                     streamState='stopped', streamProcess=None, streamAttempts=0)
        write_selkies_state(state)
        return
    stream = state.get('streamProcess')
    if stream:
        child = children_by_pid.get(stream.get('pid'))
        stop_managed_child(child) if child else stop_managed_process(stream)
    for process in reversed(state.get('sessionProcesses', [])):
        child = children_by_pid.get(process.get('pid'))
        stop_managed_child(child) if child else stop_managed_process(process)
    state.update(sessionState='stopped', sessionProcesses=[],
                 streamState='stopped', streamProcess=None, streamAttempts=0)
    write_selkies_state(state)


def selkies_session_state(state, service_running):
    if not isinstance(state, dict):
        return 'starting' if service_running else 'stopped'
    if state.get('bootId') != current_boot_id():
        return 'stopped'
    saved = state.get('sessionState')
    records = state.get('sessionProcesses')
    if saved == 'failed':
        return 'failed'
    if saved == 'stopped':
        return 'stopped'
    if saved == 'running' and isinstance(records, list) and len(records) == 3 and all(managed_process_matches(item) for item in records):
        return 'running'
    if saved == 'starting' and service_running:
        return 'starting'
    return 'failed' if saved == 'running' else 'stopped'


def selkies_stream_state(state, service_running):
    if not isinstance(state, dict):
        return 'starting' if service_running else 'stopped'
    if state.get('bootId') != current_boot_id():
        return 'stopped'
    saved = state.get('streamState')
    record = state.get('streamProcess')
    if saved == 'failed':
        return 'failed'
    if saved == 'stopped':
        return 'stopped'
    if saved == 'running' and managed_process_matches(record) and selkies_http_ready():
        return 'running'
    if saved == 'starting' and service_running:
        return 'starting'
    return 'failed' if saved == 'running' else 'stopped'


class SessionCommandMissing(RuntimeError):
    """A required session program is not installed; retrying cannot help."""


def session_pulse_is_live():
    """True while PulseAudio of this boot's recorded session still runs."""
    state = selkies_state()
    if not isinstance(state, dict) or state.get('bootId') != current_boot_id():
        return False
    records = state.get('sessionProcesses')
    return isinstance(records, list) and any(
        isinstance(item, dict) and item.get('name') == 'pulse' and managed_process_matches(item)
        for item in records)


def clear_stale_pulse_runtime(pulse):
    """Remove the pid file and socket of a PulseAudio that is gone.

    /run lives on the computer's disk, so a restart or an imported disk still carries the
    previous session's `pid`. Boots are nearly deterministic: the new PulseAudio often
    gets the very pid the file names, and PulseAudio then refuses to start ("Daemon
    already running") because that pid is itself."""
    if session_pulse_is_live():
        return
    for name in ('pid', 'native'):
        try:
            (pulse / name).unlink()
        except FileNotFoundError:
            pass


def reset_session_runtime():
    """Empty the previous boot's session runtime directory (sockets, bus and ICE files)."""
    runtime = RUN / 'user'
    try:
        if runtime.is_symlink() or not runtime.is_dir():
            return
        for entry in runtime.iterdir():
            if entry.is_symlink() or not entry.is_dir():
                entry.unlink()
            else:
                shutil.rmtree(entry)
    except OSError:
        pass


def prepare_selkies_runtime(account):
    if not prepare_display_socket_directory():
        raise RuntimeError('The X11 socket directory is active or unsafe; inspect the desktop service log')
    if DISPLAY_LOCK.exists() or (DISPLAY_SOCKET_DIR / 'X1').exists():
        if not remove_stale_display_artifacts():
            raise RuntimeError('Display :1 is active or unsafe; inspect it before starting the desktop')

    runtime = RUN / 'user'
    if runtime.is_symlink():
        raise RuntimeError('Desktop runtime directory must not be a symbolic link')
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chown(runtime, account.pw_uid, account.pw_gid)
    os.chmod(runtime, 0o700)
    pulse = runtime / 'pulse'
    if pulse.is_symlink():
        raise RuntimeError('PulseAudio runtime directory must not be a symbolic link')
    pulse.mkdir(mode=0o700, exist_ok=True)
    os.chown(pulse, account.pw_uid, account.pw_gid)
    os.chmod(pulse, 0o700)
    clear_stale_pulse_runtime(pulse)
    authority = runtime / 'Xauthority'
    if authority.exists() or authority.is_symlink():
        info = authority.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != account.pw_uid:
            raise RuntimeError('Desktop X authority file has an unexpected owner or type')
        authority.unlink()
    cookie = secrets.token_hex(16)
    try:
        subprocess.run(['runuser', '-u', USER, '--', 'xauth', '-f', str(authority),
                        'add', ':1', 'MIT-MAGIC-COOKIE-1', cookie], check=True,
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except subprocess.CalledProcessError:
        raise RuntimeError('Could not initialize desktop X authority') from None


def start_selkies():
    if supervisor():
        if selkies_session_state(selkies_state(), True) != 'failed':
            return
        stop()
    account = pwd.getpwnam(USER)
    if not SELKIES_EXECUTABLE.is_file() or not os.access(SELKIES_EXECUTABLE, os.X_OK):
        raise RuntimeError('Installed Selkies recipe is incomplete; run the explicit streamer update')
    prepare_selkies_runtime(account)
    (RUN / 'failed').unlink(missing_ok=True)
    subprocess.Popen([SELF, 'supervise-selkies'], stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    for _ in range(150):
        current = status()
        if current['sessionState'] == 'running' and current['streamState'] == 'running':
            return
        if current['sessionState'] == 'failed':
            raise RuntimeError('Desktop session failed to start; inspect /var/log/silo-desktop.log')
        if current['streamState'] == 'failed':
            raise RuntimeError('Selkies failed to start; desktop session remains running; inspect /var/log/silo-desktop.log')
        time.sleep(0.1)
    raise RuntimeError('Desktop is still starting; check status before retrying')


def log_line(text):
    try:
        with LOG.open('a') as output:
            output.write(text + '\n')
    except OSError:
        pass


def start_session_processes(commands, environment, account, state, children, stopping,
                            after_launch=None):
    """Launch the session processes in order, retrying a failed launch with backoff.

    A failed attempt stops what it started, so the next one begins from nothing."""
    for attempt in range(1, SESSION_MAX_ATTEMPTS + 1):
        try:
            if attempt > 1:
                prepare_selkies_runtime(account)
            for command, argv in commands:
                if stopping():
                    return
                if not shutil.which(argv[0]):
                    raise SessionCommandMissing(f'Required desktop command is missing: {argv[0]}')
                child, record = launch_managed_process(command, argv, environment, account)
                children.append(child)
                state['sessionProcesses'].append(record)
                write_selkies_state(state)
                if after_launch:
                    after_launch(command)
            return
        except SessionCommandMissing:
            raise
        except (OSError, RuntimeError) as error:
            log_line(f'Desktop session start attempt {attempt} of {SESSION_MAX_ATTEMPTS} failed: {error}')
            for child in reversed(children):
                stop_managed_child(child)
            children.clear()
            state['sessionProcesses'] = []
            write_selkies_state(state)
            if attempt == SESSION_MAX_ATTEMPTS:
                raise
            delay = SESSION_RETRY_BASE_SECONDS * 2 ** (attempt - 1)
            for _ in range(int(delay / 0.5)):
                if stopping():
                    return
                sleep_until_service_event(0.5)


def supervise_selkies():
    with (RUN / 'supervisor.lock').open('w') as guard:
        try:
            fcntl.flock(guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        write(RUN / 'supervisor.json', {'pid': os.getpid(), 'start': identity(os.getpid())})
        stopping = False
        restart_requested_flag = False
        session_children = []
        state = dict(backend='selkies', bootId=current_boot_id(),
                     sessionState='starting', sessionProcesses=[], streamState='starting',
                     streamProcess=None, streamAttempts=0)

        def terminate(_signum, _frame):
            nonlocal stopping
            stopping = True

        def restart_stream(_signum, _frame):
            nonlocal restart_requested_flag
            restart_requested_flag = True

        def should_restart_stream():
            nonlocal restart_requested_flag
            requested = restart_requested_flag
            restart_requested_flag = False
            return requested

        def should_stop():
            trim_logs()
            # Reap exited session children even while only the stream is retried.
            for child in session_children:
                child.poll()
            return stopping

        signal.signal(signal.SIGTERM, terminate)
        signal.signal(signal.SIGINT, terminate)
        signal.signal(signal.SIGUSR1, restart_stream)
        failed = False
        try:
            if streamer_backend() != 'selkies':
                raise RuntimeError('Installed Selkies receipt is invalid; update the streamer recipe')
            account = pwd.getpwnam(USER)
            environment = dict(HOME=str(HOME), USER=USER, LOGNAME=USER,
                               PATH=os.environ.get('PATH', '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'),
                               DISPLAY=':1', XAUTHORITY=str(RUN / 'user/Xauthority'),
                               XDG_RUNTIME_DIR=str(RUN / 'user'), XDG_CURRENT_DESKTOP='XFCE',
                               PULSE_RUNTIME_PATH=str(RUN / 'user/pulse'),
                               PULSE_SERVER=f'unix:{RUN / "user/pulse/native"}')
            commands = [
                ('xvfb', ['Xvfb', ':1', '-screen', '0', XVFB_SCREEN, '+extension', 'RANDR',
                          '-noreset', '-nolisten', 'tcp', '-auth', str(RUN / 'user/Xauthority')]),
                ('pulse', ['pulseaudio', '--daemonize=no', '--exit-idle-time=-1']),
                ('xfce', ['dbus-run-session', '--', 'startxfce4']),
            ]
            start_session_processes(
                commands, environment, account, state, session_children, lambda: stopping,
                after_launch=lambda command: (set_desktop_start_size(environment, account)
                                              if command == 'xvfb' else None))
            if stopping:
                return
            state['sessionState'] = 'running'
            write_selkies_state(state)
            supervise_selkies_stream(state, account, environment,
                                     should_stop, should_restart_stream)
        except Exception as error:
            failed = True
            (RUN / 'failed').write_text('Desktop service failed; inspect /var/log/silo-desktop.log\n')
            with LOG.open('a') as output:
                output.write(f'Selkies desktop service failed ({error}); inspect the pinned recipe and session logs\n')
        finally:
            try:
                stop_selkies_processes(state, {child.pid: child for child in session_children})
            except Exception:
                failed = True
                (RUN / 'failed').write_text('Desktop service cleanup failed; inspect /var/log/silo-desktop.log\n')
            if failed:
                state.update(sessionState='failed', streamState='failed')
                write_selkies_state(state)
            (RUN / 'supervisor.json').unlink(missing_ok=True)


def restart_selkies_streamer():
    if streamer_backend() != 'selkies':
        raise RuntimeError('Selkies streamer restart requires a valid installed Selkies recipe')
    state = selkies_state()
    if selkies_session_state(state, bool(supervisor())) != 'running':
        raise RuntimeError('Desktop session is not running; restart the desktop explicitly')
    pid = supervisor()
    if not pid:
        raise RuntimeError('Desktop service supervisor is not running')
    previous = state.get('streamProcess') if isinstance(state, dict) else None
    previous_state = state.get('streamState') if isinstance(state, dict) else None
    previous_attempts = state.get('streamAttempts') if isinstance(state, dict) else None
    acknowledged = False
    os.kill(pid, signal.SIGUSR1)
    for _ in range(150):
        current = selkies_state()
        if isinstance(current, dict) and (
                current.get('streamState') != previous_state or
                current.get('streamProcess') != previous or
                current.get('streamAttempts') != previous_attempts):
            acknowledged = True
        if (isinstance(current, dict) and
                selkies_session_state(current, bool(supervisor())) == 'running' and
                selkies_stream_state(current, bool(supervisor())) == 'running' and
                current.get('streamProcess') != previous):
            return
        if acknowledged and current and current.get('streamState') == 'failed':
            raise RuntimeError('Selkies restart failed; desktop session remains running')
        time.sleep(0.1)
    raise RuntimeError('Selkies is still restarting; check status before retrying')


def status():
    config = read('config.json', {'autoStart': True})
    recipe = streamer_backend()
    running = bool(supervisor())
    legacy_state = ('running' if running and listening() else 'starting' if running else
                    'failed' if (RUN / 'failed').exists() else 'stopped')
    if recipe == 'selkies':
        current = selkies_state()
        session_state = selkies_session_state(current, running)
        stream_state = selkies_stream_state(current, running)
        state = ('failed' if 'failed' in (session_state, stream_state) else
                 'running' if session_state == stream_state == 'running' else
                 'starting' if 'starting' in (session_state, stream_state) else 'stopped')
        backend = 'selkies'
        streamer_version = '2.0.0'
        update_required = streamer_recipe_version() < STREAMER_CURRENT_RECIPE
    elif recipe == 'kasm':
        state = legacy_state
        # A crashed helper must not make an orphaned Xvnc session appear safe
        # for an explicit streamer migration.
        session_state = ('running' if not running and legacy_display_present()
                         else legacy_state)
        stream_state = legacy_state
        backend = 'kasm'
        streamer_version = None
        update_required = False
    else:
        current = selkies_state()
        session_state = (selkies_session_state(current, running) if isinstance(current, dict)
                         else legacy_state)
        stream_state = 'failed'
        state = 'failed'
        backend = None
        streamer_version = None
        update_required = True
    return dict(installed=(STATE / 'installed.json').exists(), version='1', state=state,
                autoStart=config['autoStart'], port=6901, user=USER, display=':1',
                backend=backend, streamerVersion=streamer_version,
                sessionState=session_state, streamState=stream_state,
                updateRequired=update_required, **lcu_status())


def start():
    backend = streamer_backend()
    if backend is None:
        raise RuntimeError('Installed streamer recipe is invalid; run the explicit streamer update')
    if backend == 'selkies':
        start_selkies()
        return
    if supervisor():
        return
    if not prepare_display_socket_directory():
        raise RuntimeError('The X11 socket directory is active or unsafe; inspect the desktop service log')
    (RUN / 'failed').unlink(missing_ok=True)
    account = pwd.getpwnam(USER)
    runtime = RUN / 'user'
    runtime.mkdir(mode=0o700, exist_ok=True)
    os.chown(runtime, account.pw_uid, account.pw_gid)
    os.chmod(runtime, 0o700)
    subprocess.Popen([SELF, 'supervise'], stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    for _ in range(100):
        if supervisor() and listening():
            return
        if (RUN / 'failed').exists():
            raise RuntimeError('Desktop failed to start; inspect /var/log/silo-desktop.log')
        time.sleep(0.1)
    raise RuntimeError('Desktop is still starting; check status before retrying')


def stop():
    pid = supervisor()
    if pid:
        os.kill(pid, signal.SIGTERM)
        for _ in range(150):
            if not supervisor():
                break
            time.sleep(0.1)
        else:
            raise RuntimeError('Desktop did not stop; inspect its service log')
    current = selkies_state()
    if isinstance(current, dict) and (STATE / 'streamer.json').exists():
        stop_selkies_processes(current)
    (RUN / 'failed').unlink(missing_ok=True)


def trim_log(fd):
    with os.fdopen(fd, 'r+b') as log:
        info = os.fstat(log.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size <= 1024 * 1024:
            return
        log.seek(-256 * 1024, os.SEEK_END)
        tail = log.read()
        log.seek(0)
        log.write(tail)
        log.truncate()


def trim_logs():
    # Anchor user logs to opened directories; never traverse a user-writable link.
    file_flags = os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    try:
        trim_log(os.open(LOG, file_flags))
    except OSError:
        pass
    try:
        home = os.open(HOME, directory_flags)
        try:
            directory = os.open('.vnc', directory_flags, dir_fd=home)
            try:
                for name in os.listdir(directory):
                    if not name.endswith('.log'):
                        continue
                    try:
                        trim_log(os.open(name, file_flags, dir_fd=directory))
                    except OSError:
                        continue
            finally:
                os.close(directory)
        finally:
            os.close(home)
    except OSError:
        pass


def stop_display():
    subprocess.run(['runuser', '-u', USER, '--', 'env', f'HOME={HOME}',
                    'vncserver', '-kill', ':1'], stdin=subprocess.DEVNULL,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)


def supervise():
    backend = streamer_backend()
    if backend is None:
        (RUN / 'failed').write_text('Desktop service failed; installed streamer recipe needs an update\n')
        return
    if backend == 'selkies':
        supervise_selkies()
        return
    with (RUN / 'supervisor.lock').open('w') as guard:
        try:
            fcntl.flock(guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        write(RUN / 'supervisor.json', {'pid': os.getpid(), 'start': identity(os.getpid())})
        stopping = False
        child = None

        def terminate(_signum, _frame):
            nonlocal stopping
            stopping = True
            if child and child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)

        signal.signal(signal.SIGTERM, terminate)
        signal.signal(signal.SIGINT, terminate)
        try:
            stale_recovery_used = False
            for attempt in range(3):
                if stopping:
                    break
                log = LOG
                if log.exists() and log.stat().st_size > 1024 * 1024:
                    log.replace(log.with_suffix('.log.1'))
                with log.open('ab') as output:
                    child = subprocess.Popen(['runuser', '-u', USER, '--', 'env',
                        f'HOME={HOME}', 'USER=' + USER, 'LOGNAME=' + USER,
                        'XDG_RUNTIME_DIR=/run/silo-desktop/user',
                        'vncserver', ':1', '-fg', '-autokill', '-prompt', '0',
                        '-xstartup', str(HOME / '.vnc/xstartup')],
                        stdin=subprocess.DEVNULL, stdout=output, stderr=output, start_new_session=True)
                    if stopping and child.poll() is None:
                        os.killpg(child.pid, signal.SIGTERM)
                    while child.poll() is None:
                        trim_logs()
                        try:
                            child.wait(timeout=2)
                        except subprocess.TimeoutExpired:
                            continue
                    # A desktop exit ends its own session, never unrelated terminal jobs.
                    try:
                        os.killpg(child.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                stop_display()
                if not stopping and not stale_recovery_used and remove_stale_display_artifacts():
                    stale_recovery_used = True
                    continue
                if not stopping:
                    time.sleep(attempt + 1)
            if not stopping:
                (RUN / 'failed').write_text('Desktop exited after three attempts\n')
        except Exception as error:
            (RUN / 'failed').write_text('Desktop service failed; inspect /var/log/silo-desktop.log\n')
            with LOG.open('a') as output:
                output.write(str(error) + '\n')
            raise
        finally:
            try:
                stop_display()
            finally:
                (RUN / 'supervisor.json').unlink(missing_ok=True)


def main():
    global USER, HOME
    if os.geteuid() != 0:
        raise RuntimeError('Run sudo silo-desktop to manage the desktop')
    USER, HOME = desktop_account()
    action = sys.argv[1] if len(sys.argv) > 1 else 'status'
    if action == 'prepare-install':
        prepare_configuration(HOME)
        print(USER, HOME)
        return
    RUN.mkdir(mode=0o755, parents=True, exist_ok=True)
    if action == 'supervise':
        supervise()
        return
    if action == 'supervise-selkies':
        supervise_selkies()
        return
    with (RUN / 'operation.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        boot = BOOT_ID
        if boot.exists():
            epoch = boot.read_text()
            marker = RUN / 'boot-id'
            if not marker.exists() or marker.read_text() != epoch:
                (RUN / 'failed').unlink(missing_ok=True)
                # Nothing of the previous boot runs, but /run is on the computer's disk.
                reset_session_runtime()
                marker.write_text(epoch)
        if action == 'status':
            pass
        elif action == 'connection':
            print(json.dumps(read('connection.json')))
            return
        elif action == 'start':
            start()
        elif action == 'stop':
            stop()
        elif action == 'restart':
            stop()
            start()
        elif action == 'restart-streamer':
            restart_selkies_streamer()
        elif action == 'autostart' and len(sys.argv) == 3 and sys.argv[2] in ('true', 'false'):
            enabled = sys.argv[2] == 'true'
            write(STATE / 'config.json', {'autoStart': enabled})
            if enabled:
                start()
        elif action == 'boot':
            if read('config.json', {'autoStart': True})['autoStart']:
                start()
        else:
            raise RuntimeError('Usage: silo-desktop status|connection|start|stop|restart|restart-streamer|boot|autostart true|false')
        print(json.dumps(status()))


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
