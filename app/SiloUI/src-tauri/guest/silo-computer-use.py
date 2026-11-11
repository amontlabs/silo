#!/usr/bin/python3
"""Built-in computer use for a Silo Linux guest: Cua Driver registered with the agents.

Silo pushes this helper and the pinned driver lock (`pinned.json`, the content of
`guest/cua-driver-lock.json`) into the guest, then runs `apply --approval ask|auto` after each
boot and whenever the user changes the computer's approval switch. The helper is a plain
executor: the host decides the mode, serializes the runs and keeps the result. `apply` is
idempotent and cheap when nothing changed:

* `/usr/local/bin/cua-driver` must report the pinned version. It is a small root-owned shell
  shim that switches Cua Driver's telemetry and update check off and runs the pinned binary,
  so they stay off however the driver is started. The guest image (v5 and later) ships the
  binary under /usr/local/lib/silo/cua-driver/<version>/; otherwise the pinned archive is
  downloaded once (bounded backoff), its hash is verified and only the `cua-driver` binary is
  extracted to /opt/cua-driver/<version>/;
* Silo registers the driver as a plain stdio MCP server named `cua-driver` for the agents it
  knows a generic configuration for, as the working account: Claude Code
  (`~/.claude.json`) and Codex (`~/.codex/config.toml`). The command is the launcher
  /usr/local/libexec/silo-cua-driver-mcp (written on every apply), which reads the running
  desktop session's environment (DISPLAY, DBUS_SESSION_BUS_ADDRESS, XDG_RUNTIME_DIR) each time
  the server starts, because an MCP server started from an SSH shell has none and the bus
  address changes with every desktop start; telemetry and the update check are off. An agent that is not installed yet is pending: `reconcile` (run at the end of
  every `apply`, from a login hook in /etc/profile.d and from a small `watch` process that
  polls the install directories) registers it once its binary exists;
* the approval switch adds or removes `mcp__cua-driver` in Claude Code's
  `permissions.allow` and `default_tools_approval_mode = "approve"` in Codex's section;
* leftovers of the earlier LCU setup (its registrations, approval lines, hooks and /opt/lcu)
  are removed;
* readiness is `cua-driver doctor --json` run as the working account in the desktop session.
  After a boot the helper waits for the session (bounded) and, when the session ended up
  failed or stopped although the desktop starts with the computer, asks `silo-desktop start`
  for it again a few times with backoff instead of failing the receipt.

The result is a receipt under /var/lib/silo-computer-use that `status` projects for the host
(schemaVersion 2), and `apply` prints that status plus this run's approval outcome
(`applied`, `partial` when some agents were configured and others failed, or `failed`);
agents still to be installed are pending, not a failure. Nothing here talks to the host or
holds credentials. The approval switch configures agents' own approval prompts; agents in the
computer have root, so it is a convenience and not a security boundary.
"""
import argparse
import contextlib
import fcntl
import glob
import hashlib
import json
import os
import platform
import pwd
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
from pathlib import Path

STATE = Path('/var/lib/silo-computer-use')
PINNED = STATE / 'pinned.json'
RECEIPT = STATE / 'receipt.json'
LOCK = STATE / 'lock'
STAGE = STATE / 'stage'
LOG = Path('/var/log/silo-computer-use.log')
# The driver: a shim (SHIM_TEMPLATE) that runs the image's copy under IMAGE_ROOT, else Silo's
# own install under INSTALL_ROOT.
DRIVER_LINK = Path('/usr/local/bin/cua-driver')
IMAGE_ROOT = Path('/usr/local/lib/silo/cua-driver')
INSTALL_ROOT = Path('/opt/cua-driver')
# guest/cua-driver-shim is the same text; the image installs it too. Rust pushes only this
# helper, so it is embedded here.
SHIM_TEMPLATE = '''#!/bin/sh
# /usr/local/bin/cua-driver: Cua Driver with its telemetry and update check off, however it is
# started (MCP launcher, desktop terminal, SSH shell, agent tool). Installed by the Silo guest
# image and by silo-computer-use, which put the pinned binary's path below.
export CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false DO_NOT_TRACK=1
exec @DRIVER@ "$@"
'''
LEGACY_PREFIX = Path('/opt/lcu')
LEGACY_RECEIPT = Path('/var/lib/silo-desktop/lcu.json')
DESKTOP_COMMAND = '/usr/local/bin/silo-desktop'
DESKTOP = [DESKTOP_COMMAND, 'status']
DESKTOP_CONFIG = Path('/var/lib/silo-desktop/config.json')
USER = 'silo'
HOME = '/home/silo'
HELPER = '/usr/local/libexec/silo-computer-use'
# The working account's PATH: the directories the agents' installers use.
USER_BIN = [f'{HOME}/.local/bin', f'{HOME}/.bun/bin', f'{HOME}/.cargo/bin', f'{HOME}/.npm-global/bin',
            '/usr/local/bin', '/usr/bin', '/bin']
USER_PATH = ':'.join(USER_BIN)
SERVER = 'cua-driver'
# The MCP command of every registration: it reads the running desktop session when the server
# starts, so no session address is stored. Rust pushes only this helper, so the text is embedded
# here (guest/silo-cua-driver-mcp is the same file; the image installs it too).
LAUNCHER = Path('/usr/local/libexec/silo-cua-driver-mcp')
LAUNCHER_TEXT = r'''#!/usr/bin/python3
"""Starts the Cua Driver MCP server inside the current Silo desktop session.

Agents start MCP servers from SSH shells and terminals that may predate the desktop session,
and the session's D-Bus address changes with every desktop start. The desktop session writes its
environment to a file when it starts; this launcher reads that file when the server starts, turns
off Cua Driver's telemetry and update check, and runs `cua-driver mcp`. A desktop service
installed by an earlier Silo writes no file; its session is read from its `xfce4-session` process.
Installed by the guest image and refreshed by silo-computer-use.
"""
import os
import stat
import sys

DRIVER = '/usr/local/bin/cua-driver'
KEYS = ('DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR', 'XAUTHORITY')
SESSION_FILE = '/run/silo-desktop/user/session.env'


def process_env(proc, uid):
    """The environment of this account's `xfce4-session` process, or nothing."""
    try:
        pids = sorted((n for n in os.listdir(proc) if n.isdigit()), key=int)
    except OSError:
        pids = []
    for pid in pids:
        try:
            base = os.path.join(proc, pid)
            with open(os.path.join(base, 'comm')) as handle:
                if handle.read().strip() != 'xfce4-session':
                    continue
            if os.stat(base).st_uid != uid:
                continue
            with open(os.path.join(base, 'environ'), 'rb') as handle:
                raw = handle.read()
        except OSError:
            continue
        found = {}
        for item in raw.split(b'\0'):
            key, _, value = item.decode('utf-8', 'replace').partition('=')
            if key in KEYS and value:
                found[key] = value
        if 'DBUS_SESSION_BUS_ADDRESS' in found:
            return found
    return {}


def session_env(path=SESSION_FILE, uid=None, proc='/proc'):
    uid = os.getuid() if uid is None else uid
    found = {}
    try:
        # Only a small regular file of this account, opened without following a link.
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_NOCTTY)
        try:
            info = os.fstat(fd)
            if stat.S_ISREG(info.st_mode) and info.st_uid == uid:
                text = os.read(fd, 8192).decode('utf-8', 'replace')
                for line in text.splitlines():
                    key, _, value = line.partition('=')
                    if key in KEYS and value:
                        found[key] = value
        finally:
            os.close(fd)
    except OSError:
        pass
    if 'DBUS_SESSION_BUS_ADDRESS' not in found:
        found = process_env(proc, uid) or found
    runtime = found.get('XDG_RUNTIME_DIR') or (
        '/run/silo-desktop/user' if os.path.isdir('/run/silo-desktop/user') else f'/run/user/{uid}')
    found.setdefault('DISPLAY', ':1')
    found['XDG_RUNTIME_DIR'] = runtime
    found.setdefault('DBUS_SESSION_BUS_ADDRESS', f'unix:path={runtime}/bus')
    return found


def main(argv):
    environment = dict(os.environ)
    environment.update(session_env())
    environment['CUA_DRIVER_RS_TELEMETRY_ENABLED'] = '0'
    environment['CUA_DRIVER_RS_UPDATE_CHECK'] = 'false'
    os.execve(DRIVER, [DRIVER, 'mcp', *argv[1:]], environment)


if __name__ == '__main__':
    main(sys.argv)
'''
# The agents Silo registers the driver with, and the executable that shows each is installed.
AGENTS = {'claude-code': 'claude', 'codex': 'codex'}
EXECUTABLES = AGENTS
CLAUDE_STATE = Path(HOME) / '.claude.json'
CLAUDE_SETTINGS = Path(HOME) / '.claude/settings.json'
CODEX_CONFIG = Path(HOME) / '.codex/config.toml'
# What LCU left in the working account besides agent configuration: its data, the `lcu*`
# skills it installed for the agents, and its package entry in Pi's settings.
LEGACY_DATA = Path(HOME) / '.local/share/lcu'
LEGACY_SKILL_DIRS = [Path(HOME) / d for d in ('.claude/skills', '.codex/skills', '.agents/skills', '.pi/agent/skills')]
PI_SETTINGS = Path(HOME) / '.pi/agent/settings.json'
# Where version managers put the agents' executables, besides USER_BIN (globs, no login shell).
AGENT_BIN_GLOBS = [f'{HOME}/.nvm/versions/node/*/bin', f'{HOME}/.volta/bin', f'{HOME}/.asdf/shims',
                   f'{HOME}/.local/share/pnpm', '/snap/bin']
# The default session environment of the Silo desktop (setup-desktop.sh, desktop-service.py).
SESSION_DISPLAY = ':1'
SESSION_RUNTIME_DIR = '/run/silo-desktop/user'
PRIVACY_ENV = {'CUA_DRIVER_RS_TELEMETRY_ENABLED': '0', 'CUA_DRIVER_RS_UPDATE_CHECK': 'false'}
WATCH_LOCK = STATE / 'watch.lock'
WATCH_INTERVAL = 5
# The watcher ends after this long even with agents still pending; the next boot, login or
# apply starts a new one.
WATCH_MAX = 6 * 3600
PROFILE_HOOK = Path('/etc/profile.d/silo-computer-use.sh')
# Cua Driver sends telemetry and checks for updates by default; these switch both off for any
# shell, not only for agents that go through the launcher. The v5 image ships the same files.
PRIVACY_PROFILE = Path('/etc/profile.d/silo-cua-driver.sh')
PRIVACY_PROFILE_TEXT = '''# Cua Driver: no telemetry, no update check (Silo guest image).
export CUA_DRIVER_RS_TELEMETRY_ENABLED=0
export CUA_DRIVER_RS_UPDATE_CHECK=false
'''
SYSTEM_ENVIRONMENT = Path('/etc/environment')
HOOK = '''# Registers agents installed after computer use was set up (silo-computer-use).
if [ "$(id -un)" = silo ] && grep -q '"pending": \\["' /var/lib/silo-computer-use/receipt.json 2>/dev/null; then
    (sudo -n /usr/local/libexec/silo-computer-use reconcile >/dev/null 2>&1 &)
fi
'''
# The receipt and the status this helper emits (the earlier LCU receipt was schema 1).
SCHEMA = 2
LOCK_SCHEMA = 1
VERSION = re.compile(r'[0-9][0-9A-Za-z.+~-]{0,63}')
SHA256 = re.compile(r'[0-9a-f]{64}')
APPROVALS = ('ask', 'auto')
SESSION_WAIT_BOOT = 300
SESSION_WAIT = 90
# After a boot a failed or stopped session is started again this many times, waiting
# SESSION_REPAIR_BASE * 2 ** (n - 1) seconds before attempt n.
SESSION_REPAIR_ATTEMPTS = 3
SESSION_REPAIR_BASE = 2
# Below the host's own timeout for one apply (APPLY_TIMEOUT in engine/src/computer_use.rs, 900 s),
# so a run waiting for the lock gives up before the host does.
LOCK_WAIT = 600
# The driver download: waits between attempts (so five attempts), the longest one curl
# attempt may take, and the total time after which no new attempt starts.
DOWNLOAD_BACKOFF = (5, 10, 20, 40)
DOWNLOAD_MAX_TIME = 240
DOWNLOAD_BUDGET = 420
# While a computer is being created the host holds the device-wide operation gate, so the
# helper never downloads then: without the driver in the image, setup is left to the first
# start, whose apply runs in the background and retries fully.
CREATION = False
ARCHITECTURES = {'aarch64': 'arm64', 'arm64': 'arm64', 'x86_64': 'amd64', 'amd64': 'amd64'}
# The `doctor` probes that must be ok before the driver can drive windows in the session.
DOCTOR_REQUIRED = ('display server', 'X11 connection', 'AT-SPI')


class Failure(Exception):
    """A step failed in a way the receipt can name (`reason` is a stable code)."""

    def __init__(self, reason, detail=''):
        super().__init__(detail or reason)
        self.reason = reason


class Conflict(Failure):
    """An agent already has a server of that name that Silo did not add; it is left alone."""


def now():
    return int(time.time())


def log(text):
    try:
        fd = os.open(LOG, os.O_WRONLY | os.O_CREAT | os.O_APPEND | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'a') as output:
            output.write(text if text.endswith('\n') else text + '\n')
    except OSError:
        pass


def write_json(path, value):
    path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.tmp-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(json.dumps(value, sort_keys=True) + '\n')
            output.flush()
            os.fchmod(output.fileno(), 0o644)
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


def read_json(path):
    try:
        info = path.lstat()
        if info.st_uid != 0 or info.st_mode & 0o022 or not path.is_file():
            return None
        value = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    return value if isinstance(value, dict) else None


def release_prefix(version):
    """Where every pinned archive must come from: the driver's own GitHub release."""
    return f'https://github.com/trycua/cua/releases/download/cua-driver-rs-v{version}/'


def load_pinned():
    """The driver lock the host pinned (`guest/cua-driver-lock.json`), or None when absent or
    malformed."""
    value = read_json(PINNED)
    if not value or value.get('schemaVersion') != LOCK_SCHEMA:
        return None
    try:
        assets = value['assets']
        if not VERSION.fullmatch(value['version']) or not isinstance(assets, dict):
            return None
        for arch in ('arm64', 'amd64'):
            asset = assets[arch]
            if (not str(asset['url']).startswith(release_prefix(value['version'])) or not SHA256.fullmatch(asset['sha256']) or
                    not SHA256.fullmatch(asset['binarySha256']) or
                    not isinstance(asset['bytes'], int) or isinstance(asset['bytes'], bool) or
                    asset['bytes'] <= 0):
                return None
    except (KeyError, TypeError):
        return None
    return value


def architecture():
    return ARCHITECTURES.get(platform.machine())


def pinned_asset(pinned):
    arch = architecture()
    if arch is None:
        raise Failure('driver-archive-unavailable', f'no driver build for {platform.machine()}')
    return pinned['assets'][arch]


def lock_held():
    """True while another process runs `sync` (the lock is taken without waiting)."""
    try:
        STATE.mkdir(mode=0o755, parents=True, exist_ok=True)
        with open(LOCK, 'a') as handle:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                return True
            fcntl.flock(handle, fcntl.LOCK_UN)
    except OSError:
        return False
    return False


def matches_pair(receipt, pinned):
    return receipt.get('schemaVersion') == SCHEMA and receipt.get('driverVersion') == pinned['version']


def matches(receipt, pinned, approval):
    """Whether the receipt shows a run for this driver that applied `approval` completely."""
    return (matches_pair(receipt, pinned) and receipt.get('approval') == approval
            and receipt.get('approvalOutcome') == 'applied')


def clean(value, pattern=VERSION):
    return value if isinstance(value, str) and pattern.fullmatch(value) else None


def clean_names(value):
    if not isinstance(value, list):
        return None
    return sorted({a for a in value if isinstance(a, str) and re.fullmatch(r'[a-z0-9-]{1,32}', a)})


def status(pinned=None, receipt=None):
    """What the host shows, from the receipt alone. Never runs the driver."""
    pinned = pinned or load_pinned()
    result = {'schemaVersion': SCHEMA, 'state': 'not-set-up', 'reason': None, 'driverVersion': None,
              'approval': None, 'approvalOutcome': None, 'agents': None, 'pending': None,
              'verifiedAt': None}
    if pinned is None:
        result['reason'] = 'not-configured'
        return result
    receipt = receipt if receipt is not None else read_json(RECEIPT)
    held = lock_held()
    # The receipt describes the last run, whatever mode it applied. A receipt of another
    # schema (the earlier LCU setup) or of another driver version counts for nothing.
    if receipt and matches_pair(receipt, pinned):
        state = receipt.get('state')
        if state == 'installing' and not held:
            result.update(state='failed', reason='interrupted')
        elif state in ('installing', 'ready', 'failed'):
            result.update(state=state, reason=clean(receipt.get('reason'), re.compile(r'[a-z0-9-]{1,40}')))
        verified = receipt.get('verifiedAt')
        result.update(
            driverVersion=clean(receipt.get('driverVersion')),
            approval=receipt.get('approval') if receipt.get('approval') in APPROVALS else None,
            approvalOutcome=(receipt.get('approvalOutcome')
                             if receipt.get('approvalOutcome') in ('applied', 'partial', 'failed') else None),
            verifiedAt=verified if isinstance(verified, int) and not isinstance(verified, bool) else None)
        if state in ('ready', 'failed'):
            result.update(agents=clean_names(receipt.get('agents')), pending=clean_names(receipt.get('pending')))
    elif held:
        result.update(state='installing')
    return result


def run(argv, *, user=False, timeout=900, check=True, cwd=None, extra_env=None, quiet=False):
    """Runs a command with no stdin, appending its output to the log (except a listing)."""
    if user:
        argv = ['runuser', '-u', USER, '--', 'env', f'HOME={HOME}', f'USER={USER}',
                f'LOGNAME={USER}', f'PATH={USER_PATH}',
                *(f'{key}={value}' for key, value in (extra_env or {}).items()), *argv]
    environment = dict(os.environ, DEBIAN_FRONTEND='noninteractive')
    if extra_env:
        environment.update(extra_env)
    try:
        result = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, timeout=timeout,
                                cwd=cwd, env=environment, errors='replace')
    except subprocess.TimeoutExpired:
        log(f'$ {" ".join(argv)}\ntimed out after {timeout}s')
        raise Failure('timed-out', ' '.join(argv[:3])) from None
    log(f'$ {" ".join(argv)}\n{"" if quiet else result.stdout[-4000:]}exit {result.returncode}')
    if check and result.returncode != 0:
        raise Failure('command-failed', f'{argv[0]} exited {result.returncode}')
    return result


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def download(url, target):
    """Downloads `url` over HTTPS with bounded exponential backoff.

    curl retries transient errors itself (an empty reply, a reset, a refused connection);
    the outer loop covers what it gives up on, such as a DNS timeout. Every attempt is logged.
    Only a failure to fetch is retried here: the caller verifies the hash and never retries
    a mismatch. Raises `driver-archive-unavailable` once the attempts or the time run out.
    """
    if CREATION:
        log('not downloading the Cua Driver archive while the computer is being created')
        raise Failure('driver-archive-unavailable', 'left to the first start')
    attempts = len(DOWNLOAD_BACKOFF) + 1
    max_time = DOWNLOAD_MAX_TIME
    deadline = time.monotonic() + DOWNLOAD_BUDGET
    for attempt in range(1, attempts + 1):
        log(f'downloading the Cua Driver archive (attempt {attempt} of {attempts})')
        try:
            run(['curl', '--silent', '--show-error', '--fail', '--location',
                 '--retry', '3', '--retry-delay', '2', '--retry-all-errors', '--retry-connrefused',
                 '--connect-timeout', '30', '--max-time', str(max_time),
                 '--proto', '=https', '--proto-redir', '=https', '--tlsv1.2', url, '--output', str(target)],
                timeout=max_time + 60)
            return
        except Failure as failure:
            log(f'driver download attempt {attempt} failed: {failure}')
        target.unlink(missing_ok=True)
        if attempt == attempts:
            break
        delay = DOWNLOAD_BACKOFF[attempt - 1]
        if time.monotonic() + delay >= deadline:
            break
        log(f'retrying the driver download in {delay}s')
        time.sleep(delay)
    raise Failure('driver-archive-unavailable', 'could not download the Cua Driver archive (network)')




def archive_path(pinned, stage):
    """The pinned archive, downloaded to `stage` and checked against the lock's size and hash."""
    asset = pinned_asset(pinned)
    target = stage / 'cua-driver.tar.gz'
    download(asset['url'], target)
    if target.stat().st_size != asset['bytes'] or sha256_file(target) != asset['sha256']:
        target.unlink(missing_ok=True)
        raise Failure('driver-archive-mismatch')
    return target


def member_is_safe(name):
    parts = Path(name).parts
    return bool(parts) and not name.startswith('/') and '..' not in parts


def extract_driver(archive, destination):
    """Writes the archive's `cua-driver` binary to `destination` and nothing else of it.

    Every member name must be a safe relative path; the binary must be a regular file. The
    other files in the archive (an SDK library, a cursor theme, helpers) are never extracted."""
    try:
        with tarfile.open(archive, 'r:gz') as bundle:
            members = bundle.getmembers()
            if not members or any(not member_is_safe(member.name) for member in members):
                raise Failure('driver-archive-invalid')
            matching = [m for m in members if os.path.normpath(m.name) == 'cua-driver']
            if len(matching) != 1 or not matching[0].isreg() or matching[0].size > 512 << 20:
                raise Failure('driver-archive-invalid')
            source = bundle.extractfile(matching[0])
            with open(destination, 'wb') as output:
                shutil.copyfileobj(source, output)
    except (tarfile.TarError, OSError, EOFError) as error:
        raise Failure('driver-archive-invalid', str(error)) from None
    os.chmod(destination, 0o755)


def driver_version(path=DRIVER_LINK):
    """The version `path --version` prints (`cua-driver 0.38.0`), or None when it cannot run.
    Runs as the working account, with the driver's telemetry and update check off."""
    try:
        if not Path(path).exists():
            return None
        result = run([str(path), '--version'], user=True, timeout=60, check=False, quiet=True,
                     extra_env=PRIVACY_ENV)
    except (Failure, OSError):
        return None
    match = re.search(r'^cua-driver (\S+)\s*$', result.stdout or '', re.MULTILINE) if result.returncode == 0 else None
    return match.group(1) if match else None


def shim_text(target):
    return SHIM_TEMPLATE.replace('@DRIVER@', str(target))


def write_shim(target):
    """Makes /usr/local/bin/cua-driver the shim that runs `target`, atomically; root-owned, executable
    by everyone. A symlink left there by an earlier helper is replaced."""
    text = shim_text(target)
    try:
        if (not DRIVER_LINK.is_symlink() and DRIVER_LINK.is_file() and DRIVER_LINK.read_text() == text
                and os.access(DRIVER_LINK, os.X_OK)):
            return
    except (OSError, UnicodeDecodeError):
        pass
    DRIVER_LINK.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.cua-driver-', dir=DRIVER_LINK.parent)
    try:
        with os.fdopen(fd, 'w') as handle:
            handle.write(text)
        os.chmod(temporary, 0o755)
        os.replace(temporary, DRIVER_LINK)
    except BaseException:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temporary)
        raise


def shim_is_current(version):
    """True when /usr/local/bin/cua-driver is the shim for a pinned binary of `version`."""
    try:
        text = DRIVER_LINK.read_text() if not DRIVER_LINK.is_symlink() else None
    except (OSError, UnicodeDecodeError):
        return False
    return text in {shim_text(root / version / 'cua-driver') for root in (IMAGE_ROOT, INSTALL_ROOT)}


def ensure_driver(pinned, stage):
    """Makes /usr/local/bin/cua-driver the shim for the pinned version and returns its path: the
    image's copy when it matches, a previous install of Silo's own, else the verified download."""
    wanted = pinned['version']
    binary_sha256 = pinned_asset(pinned)['binarySha256']
    for candidate in (IMAGE_ROOT / wanted / 'cua-driver', INSTALL_ROOT / wanted / 'cua-driver'):
        if (candidate.is_file() and not candidate.is_symlink() and sha256_file(candidate) == binary_sha256
                and driver_version(candidate) == wanted):
            write_shim(candidate)
            return DRIVER_LINK
    installed = INSTALL_ROOT / wanted / 'cua-driver'
    archive = archive_path(pinned, stage)
    directory = INSTALL_ROOT / wanted
    directory.mkdir(mode=0o755, parents=True, exist_ok=True)
    temporary = directory / '.cua-driver.tmp'
    try:
        extract_driver(archive, temporary)
        if sha256_file(temporary) != binary_sha256:
            raise Failure('driver-archive-invalid', 'the extracted driver is not the pinned binary')
        os.replace(temporary, installed)
    finally:
        temporary.unlink(missing_ok=True)
    if driver_version(installed) != wanted:
        installed.unlink(missing_ok=True)
        raise Failure('driver-archive-invalid', 'the extracted driver is not the pinned version')
    write_shim(installed)
    return DRIVER_LINK


# ----------------------------------------------------------------- the desktop session

def read_session_file(path, uid):
    """The session's environment file. The working account owns it and writes it, so root opens
    it without following a link, never blocking on a pipe, and uses it only if it is a small
    regular file of that account: root does not hang, run out of memory or take values from
    what another owner made of it."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_NOCTTY)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != uid:
            raise OSError('not a regular file of the working account')
        return os.read(fd, 8192).decode('utf-8', 'replace')
    finally:
        os.close(fd)


SESSION_KEYS = ('DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR', 'XAUTHORITY')
SESSION_PROCESS = 'xfce4-session'


def session_process_env(proc, uid):
    """The session's environment, read from the working account's `xfce4-session` process. A
    desktop service installed by an earlier Silo writes no session file, and a computer keeps
    the service it was made with until its desktop is updated."""
    try:
        entries = sorted((p for p in proc.iterdir() if p.name.isdigit()), key=lambda p: int(p.name))
    except OSError:
        entries = []
    for entry in entries:
        try:
            if (entry / 'comm').read_text().strip() != SESSION_PROCESS or entry.stat().st_uid != uid:
                continue
            raw = (entry / 'environ').read_bytes()
        except OSError:
            continue
        found = {}
        for item in raw.split(b'\0'):
            key, _, value = item.decode('utf-8', 'replace').partition('=')
            if key in SESSION_KEYS and value:
                found[key] = value
        if 'DBUS_SESSION_BUS_ADDRESS' in found:
            return found
    return {}


def session_env(strict=False, directory=SESSION_RUNTIME_DIR, proc=Path('/proc')):
    """The environment of the desktop session, read from the file the session wrote when it
    started: DISPLAY, DBUS_SESSION_BUS_ADDRESS, XDG_RUNTIME_DIR (and XAUTHORITY when the session
    has one). Without the file, a session an earlier desktop service started is read from its
    process (`session_process_env`).

    An MCP server an agent starts from an SSH shell has none of these. The bus address is
    chosen by `dbus-run-session` and changes with every desktop start. Without a session (the
    desktop is off) the bus address is absent; `strict` then raises `session-unavailable`."""
    found = {}
    try:
        uid = pwd.getpwnam(USER).pw_uid
    except KeyError:
        uid = None
    try:
        if uid is not None:
            for line in read_session_file(Path(directory) / 'session.env', uid).splitlines():
                key, _, value = line.partition('=')
                if key in SESSION_KEYS and value:
                    found[key] = value
    except OSError:
        pass
    if 'DBUS_SESSION_BUS_ADDRESS' not in found and uid is not None:
        found = session_process_env(proc, uid) or found
    if 'DBUS_SESSION_BUS_ADDRESS' not in found and strict:
        raise Failure('session-unavailable', 'no desktop session')
    environment = {'DISPLAY': found.get('DISPLAY', SESSION_DISPLAY),
                   'XDG_RUNTIME_DIR': found.get('XDG_RUNTIME_DIR', SESSION_RUNTIME_DIR)}
    for key in ('DBUS_SESSION_BUS_ADDRESS', 'XAUTHORITY'):
        if key in found:
            environment[key] = found[key]
    return environment


def write_launcher():
    """Installs (or refreshes) the MCP launcher; root-owned and executable by everyone."""
    try:
        if LAUNCHER.is_file() and LAUNCHER.read_text() == LAUNCHER_TEXT and os.access(LAUNCHER, os.X_OK):
            return
        LAUNCHER.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
        fd, temporary = tempfile.mkstemp(prefix='.tmp-', dir=LAUNCHER.parent)
        with os.fdopen(fd, 'w') as output:
            output.write(LAUNCHER_TEXT)
            os.fchmod(output.fileno(), 0o755)
        os.replace(temporary, LAUNCHER)
    except OSError as error:
        raise Failure('setup-failed', f'could not write {LAUNCHER}: {error}') from None


def write_privacy_env():
    """Switches Cua Driver's telemetry and update check off for every account: a profile.d
    script for login shells and /etc/environment for PAM sessions. Lines already present are
    kept; nothing else in /etc/environment changes."""
    try:
        if not (PRIVACY_PROFILE.is_file() and PRIVACY_PROFILE.read_text() == PRIVACY_PROFILE_TEXT):
            PRIVACY_PROFILE.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
            fd, temporary = tempfile.mkstemp(prefix='.tmp-', dir=PRIVACY_PROFILE.parent)
            with os.fdopen(fd, 'w') as output:
                output.write(PRIVACY_PROFILE_TEXT)
                os.fchmod(output.fileno(), 0o644)
            os.replace(temporary, PRIVACY_PROFILE)
        current = SYSTEM_ENVIRONMENT.read_text() if SYSTEM_ENVIRONMENT.is_file() else ''
        lines = current.splitlines()
        missing = [f'{key}={value}' for key, value in PRIVACY_ENV.items()
                   if not any(line.strip().startswith(f'{key}=') for line in lines)]
        if missing:
            text = current + ('' if not current or current.endswith('\n') else '\n') + '\n'.join(missing) + '\n'
            fd, temporary = tempfile.mkstemp(prefix='.tmp-', dir=SYSTEM_ENVIRONMENT.parent)
            with os.fdopen(fd, 'w') as output:
                output.write(text)
                os.fchmod(output.fileno(), 0o644)
            os.replace(temporary, SYSTEM_ENVIRONMENT)
    except OSError as error:
        log(f'could not switch off Cua Driver telemetry system-wide: {error}')


BROWSER_LAUNCHER_DIR = Path('/usr/local/bin')
APPLICATIONS_DIR = Path('/usr/local/share/applications')
# The wrapper starts the first of these that exists. The names are what an agent, `xdg-open`
# or the desktop's launcher runs (`google-chrome-stable` and `google-chrome` are Google
# Chrome's own, `chromium` and `chromium-browser` the distributions').
BROWSER_BINARIES = ('/opt/google/chrome/google-chrome', '/usr/lib/chromium/chromium',
                    '/usr/lib/chromium-browser/chromium-browser')
BROWSER_NAMES = ('google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser')
BROWSER_LAUNCHER_MARK = '# silo-container-browser-launcher'
BROWSER_LAUNCHER_TEXT = '''#!/bin/sh
# silo-container-browser-launcher
# Written by silo-computer-use in a Silo container computer. A browser's own sandbox needs a
# nested user namespace, which a container computer's seccomp profile denies (the baseline has
# no exception for the browser), so the browser runs without it: a web page that compromises
# the browser reaches everything in this computer.
for browser in %s; do
    if [ -x "$browser" ]; then
        exec "$browser" --no-sandbox "$@"
    fi
done
echo "No browser is installed in this computer (looked for: %s)." >&2
exit 127
''' % (' '.join(BROWSER_BINARIES), ', '.join(BROWSER_BINARIES))
# The desktop entries the packages install (Google Chrome's, Debian's Chromium) and the launcher
# each one runs instead; the launcher adds the flag itself.
BROWSER_ENTRIES = {'google-chrome.desktop': 'google-chrome-stable', 'chromium.desktop': 'chromium'}
BROWSER_DESKTOP_NAME = 'Web Browser (without its sandbox)'
BROWSER_DESKTOP_TEXT = '''[Desktop Entry]
Version=1.0
Name=%s
Comment=Silo container computer: starts the browser without its sandbox
Exec=%%s %%%%U
Terminal=false
Type=Application
Categories=Network;WebBrowser;
MimeType=text/html;text/xml;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;
StartupNotify=true
''' % BROWSER_DESKTOP_NAME


def is_container():
    """Whether this guest is a Silo container computer (the engine sets the variable)."""
    return os.environ.get('SILO_RUNTIME') == 'container'


def write_if_changed(path, text, mode):
    """Writes `path` atomically unless it already holds exactly `text`; never replaces a file
    that is not ours (`owned` is checked by the caller)."""
    if path.is_file() and path.read_text() == text and os.access(path, os.X_OK if mode & 0o100 else os.R_OK):
        return
    path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.tmp-', dir=path.parent)
    with os.fdopen(fd, 'w') as output:
        output.write(text)
        os.fchmod(output.fileno(), mode)
    os.replace(temporary, path)


def write_browser_launchers(directory=None, applications=None):
    """In a container computer, puts a `--no-sandbox` launcher ahead of each browser command
    on PATH and a desktop entry for the same name ahead of the packaged one, so a browser
    started by an agent, the desktop or `xdg-open` runs the way the baseline allows. A file of
    the same name that this helper did not write is left alone. A virtual machine keeps its
    browser's sandbox."""
    if not is_container():
        return
    directory = directory or BROWSER_LAUNCHER_DIR
    applications = applications or APPLICATIONS_DIR
    try:
        for name in BROWSER_NAMES:
            target = directory / name
            if target.exists() and BROWSER_LAUNCHER_MARK not in target.read_text(errors='replace'):
                continue
            write_if_changed(target, BROWSER_LAUNCHER_TEXT, 0o755)
        for entry_name, launcher in BROWSER_ENTRIES.items():
            entry = applications / entry_name
            if not entry.exists() or f'Name={BROWSER_DESKTOP_NAME}' in entry.read_text(errors='replace'):
                write_if_changed(entry, BROWSER_DESKTOP_TEXT % (directory / launcher), 0o644)
    except OSError as error:
        log(f'could not write the browser launchers: {error}')


@contextlib.contextmanager
def as_user():
    """Runs file edits as the working account, so nothing it owns is written through root."""
    if os.geteuid() != 0:
        yield
        return
    account = pwd.getpwnam(USER)
    os.setegid(account.pw_gid)
    os.seteuid(account.pw_uid)
    try:
        yield
    finally:
        os.seteuid(0)
        os.setegid(0)


def repair_agent_files():
    """As root: gives the working account back the agent configuration files it manages (and the
    directories holding them) when something running as root, such as a keep, left them root-owned.
    The account controls everything below HOME, so each step goes through directory descriptors
    opened without following links: a parent that is a link, or not a directory directly below HOME,
    is left alone."""
    if os.geteuid() != 0:
        return
    try:
        account = pwd.getpwnam(USER)
    except KeyError:
        return
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    try:
        home_fd = os.open(HOME, flags)
    except OSError:
        return
    try:
        for path in (CLAUDE_STATE, CLAUDE_SETTINGS, CODEX_CONFIG):
            parts = path.relative_to(HOME).parts
            if len(parts) == 1:
                repair_entry(home_fd, parts[0], account)
            elif len(parts) == 2:
                try:
                    parent_fd = os.open(parts[0], flags, dir_fd=home_fd)
                except OSError:
                    continue
                try:
                    if stat.S_ISDIR(os.fstat(parent_fd).st_mode):
                        if os.fstat(parent_fd).st_uid == 0:
                            os.fchown(parent_fd, account.pw_uid, account.pw_gid)
                        repair_entry(parent_fd, parts[1], account)
                except OSError:
                    pass
                finally:
                    os.close(parent_fd)
    finally:
        os.close(home_fd)


def repair_entry(dir_fd, name, account):
    """Chowns `name` below `dir_fd` to the account when it is a root-owned file or folder."""
    try:
        info = os.stat(name, dir_fd=dir_fd, follow_symlinks=False)
        if info.st_uid == 0 and not stat.S_ISLNK(info.st_mode):
            os.chown(name, account.pw_uid, account.pw_gid, dir_fd=dir_fd, follow_symlinks=False)
    except OSError:
        pass


def write_text(path, text, mode=0o600):
    """Atomically replaces `path` (an existing file keeps its permissions)."""
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        mode = path.stat().st_mode & 0o777
    except OSError:
        pass
    fd, temporary = tempfile.mkstemp(prefix='.silo-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(text)
            output.flush()
            os.fchmod(output.fileno(), mode)
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


# ------------------------------------------------------------- agent registrations

# What the earlier LCU setup left in an agent's configuration.
# Only what LCU itself wrote: its MCP tools (`mcp__lcu`, `mcp__lcu__<tool>`) and commands under
# /opt/lcu or /usr/local/bin/lcu. A user's own rule that merely mentions "lcu" is kept.
LEGACY = re.compile(r'(?<![A-Za-z0-9_])mcp__lcu(?:__[A-Za-z0-9_-]+)?(?![A-Za-z0-9_-])'
                    r'|/opt/lcu(?=/|$|[\s"\':])|/usr/local/bin/lcu(?=$|[\s"\':])')
APPROVAL_RULE = 'mcp__' + SERVER


def server_entry(command=None):
    return {'type': 'stdio', 'command': str(command or LAUNCHER), 'args': [], 'env': dict(sorted(PRIVACY_ENV.items()))}


def read_object(path):
    """The JSON object in `path`; `{}` when the file is absent; `None` when it is not an object."""
    try:
        text = path.read_text()
    except FileNotFoundError:
        return {}
    value = json.loads(text) if text.strip() else {}
    return value if isinstance(value, dict) else None


def strip_legacy_rules(settings):
    """Removes LCU's permission rules and hooks from Claude Code settings; True when changed."""
    changed = False
    permissions = settings.get('permissions')
    if isinstance(permissions, dict):
        for kind in ('allow', 'deny', 'ask'):
            rules = permissions.get(kind)
            if isinstance(rules, list):
                kept = [rule for rule in rules if not (isinstance(rule, str) and LEGACY.search(rule))]
                if kept != rules:
                    changed = True
                    permissions[kind] = kept
    hooks = settings.get('hooks')
    if isinstance(hooks, dict):
        for event in list(hooks):
            entries = hooks[event]
            if not isinstance(entries, list):
                continue
            kept = []
            for entry in entries:
                inner = entry.get('hooks') if isinstance(entry, dict) else None
                if isinstance(inner, list):
                    left = [hook for hook in inner if not LEGACY.search(json.dumps(hook))]
                    if len(left) != len(inner):
                        changed = True
                        if not left:
                            continue
                        entry = dict(entry, hooks=left)
                elif LEGACY.search(json.dumps(entry)):
                    changed = True
                    continue
                kept.append(entry)
            if kept:
                hooks[event] = kept
            else:
                del hooks[event]
                changed = True
        if not hooks:
            del settings['hooks']
    return changed


MANAGED_ENV = {'SILO_MANAGED': '1'}
MANAGED_TOML = re.compile(r'\bSILO_MANAGED\s*=\s*"1"')


def claude_owns(present, known):
    """Whether `present`, Claude Code's entry for one of Silo's servers, is Silo's: marked, or the
    unmarked entry an earlier Silo wrote (the same command and arguments as `known`)."""
    if not isinstance(present, dict):
        return False
    env = present.get('env')
    if isinstance(env, dict) and env.get('SILO_MANAGED') == '1':
        return True
    return (known is not None and present.get('command') == known['command']
            and present.get('args') == known['args'])


def claude_servers(state, entry, server, remove, known):
    """Applies the change to the servers in the parsed ~/.claude.json."""
    servers = state.get('mcpServers')
    if servers is not None and not isinstance(servers, dict):
        raise Failure('setup-failed', 'mcpServers is not an object')
    if isinstance(servers, dict):
        servers.pop('lcu', None)
        if (server != SERVER and (entry is not None or remove) and server in servers
                and not claude_owns(servers[server], known)):
            raise Conflict('conflict', f'Claude Code has its own {server} server')
        if remove:
            servers.pop(server, None)
    if entry is not None:
        if servers is None:
            # Absent or `null`: both mean no servers yet.
            servers = state['mcpServers'] = {}
        servers[server] = entry


def edit_claude(entry, approval, server=SERVER, remove=False, known=None):
    """Claude Code: `mcpServers.cua-driver` in ~/.claude.json (`entry`; None leaves the
    registration alone) and `mcp__cua-driver` in ~/.claude/settings.json `permissions.allow`
    for the auto mode. LCU's entries are removed from both. Another `server` (Silo's own MCP
    server) is only added to or, with `remove`, taken out of ~/.claude.json, and only when the
    entry there is Silo's (`known` is the entry an earlier Silo wrote); the settings file is left
    alone. The file is read again right before it is replaced, so a change Claude Code made in
    between is kept."""
    state = read_object(CLAUDE_STATE)
    if state is None:
        raise Failure('setup-failed', f'{CLAUDE_STATE} is not a JSON object')
    original = json.dumps(state, sort_keys=True)
    claude_servers(state, entry, server, remove, known)
    if json.dumps(state, sort_keys=True) != original:
        fresh = read_object(CLAUDE_STATE)
        if fresh is None:
            raise Failure('setup-failed', f'{CLAUDE_STATE} is not a JSON object')
        fresh_original = json.dumps(fresh, sort_keys=True)
        claude_servers(fresh, entry, server, remove, known)
        if json.dumps(fresh, sort_keys=True) != fresh_original:
            write_text(CLAUDE_STATE, json.dumps(fresh, indent=2) + '\n')
    if server != SERVER:
        return
    settings = read_object(CLAUDE_SETTINGS)
    if settings is None:
        raise Failure('setup-failed', f'{CLAUDE_SETTINGS} is not a JSON object')
    original = json.dumps(settings, sort_keys=True)
    strip_legacy_rules(settings)
    permissions = settings.get('permissions')
    if permissions is not None and not isinstance(permissions, dict):
        raise Failure('setup-failed', 'permissions is not an object')
    allow = (permissions or {}).get('allow')
    if allow is not None and not isinstance(allow, list):
        raise Failure('setup-failed', 'permissions.allow is not a list')
    if approval == 'auto' and entry is not None:
        if permissions is None:
            # Absent or `null`: both mean no rules yet.
            permissions = settings['permissions'] = {}
        if APPROVAL_RULE not in (allow or []):
            permissions['allow'] = [*(allow or []), APPROVAL_RULE]
    elif approval == 'ask' and allow and APPROVAL_RULE in allow:
        permissions['allow'] = [rule for rule in allow if rule != APPROVAL_RULE]
        # Leave no empty containers behind for a rule only Silo added.
        if not permissions['allow']:
            del permissions['allow']
        if not permissions:
            del settings['permissions']
    if json.dumps(settings, sort_keys=True) != original:
        write_text(CLAUDE_SETTINGS, json.dumps(settings, indent=2) + '\n')


TOML_HEADER = re.compile(r'^\s*\[\[?\s*([^\[\]]+?)\s*\]\]?\s*(?:#.*)?$')
LEGACY_SERVER = re.compile(r'^mcp_servers\.("?)lcu\1(\.|$)')


def own_server(server):
    """The header of Codex's table for `server` (or one of its sub-tables)."""
    return re.compile(r'^mcp_servers\.("?)' + re.escape(server) + r'\1(\.|$)')


OWN_SERVER = own_server(SERVER)


def toml_sections(text):
    """The file as `(header name or None, lines)` chunks; the first has no header."""
    sections = [(None, [])]
    for line in text.splitlines():
        match = TOML_HEADER.match(line)
        if match:
            sections.append((match.group(1), [line]))
        else:
            sections[-1][1].append(line)
    return sections


def toml_string(value):
    return json.dumps(value)


def codex_block(entry, approval, server=SERVER):
    lines = [f'[mcp_servers.{server}]', f'command = {toml_string(entry["command"])}',
             'args = [' + ', '.join(toml_string(a) for a in entry['args']) + ']']
    if approval == 'auto':
        lines.append('default_tools_approval_mode = "approve"')
    if entry['env']:
        lines += ['', f'[mcp_servers.{server}.env]']
        lines += [f'{key} = {toml_string(value)}' for key, value in entry['env'].items()]
    return lines


def codex_owns(sections, server, known):
    """Whether the tables of one server in Codex's configuration are Silo's: marked, or the
    unmarked table an earlier Silo wrote (the same command and arguments as `known`)."""
    if MANAGED_TOML.search('\n'.join(line for _, lines in sections for line in lines)):
        return True
    if known is None:
        return False
    command = args = None
    for name, lines in sections:
        if name.replace('"', '') != f'mcp_servers.{server}':
            continue
        for line in lines:
            found = re.match(r'\s*command\s*=\s*("(?:[^"\\]|\\.)*")\s*(?:#.*)?$', line)
            if found:
                command = found.group(1)
            found = re.match(r'\s*args\s*=\s*(\[.*\])\s*(?:#.*)?$', line)
            if found:
                args = found.group(1)
    try:
        return (command is not None and args is not None and json.loads(command) == known['command']
                and json.loads(args) == known['args'])
    except ValueError:
        return False


def edit_codex(entry, approval, server=SERVER, remove=False, known=None):
    """Codex: `[mcp_servers.cua-driver]` in ~/.codex/config.toml with its env table, and
    `default_tools_approval_mode = "approve"` for the auto mode. Silo owns that section and
    rewrites it; LCU's server table and hooks are removed. Another `server` has its own table
    written (or, with `remove`, taken out) the same way, and only when the tables there are Silo's
    (`known` is the entry an earlier Silo wrote). The result must parse as TOML."""
    own = own_server(server)
    try:
        original = CODEX_CONFIG.read_text()
    except FileNotFoundError:
        original = ''
    if server != SERVER and (entry is not None or remove):
        present = [(name, lines) for name, lines in toml_sections(original)
                   if name is not None and own.match(name)]
        if present and not codex_owns(present, server, known):
            raise Conflict('conflict', f'Codex has its own {server} server')
    kept = []
    for name, lines in toml_sections(original):
        if name is not None:
            if LEGACY_SERVER.match(name) or ((entry is not None or remove) and own.match(name)):
                continue
            if entry is None and approval == 'ask' and own.match(name):
                # No registration to write (the agent is not installed), but an auto line
                # that came with a kept fork must not outlive the ask mode.
                lines = [line for line in lines
                         if not re.match(r'\s*default_tools_approval_mode\s*=', line)]
            if name.split('.')[0] == 'hooks' and name != 'hooks' and LEGACY.search('\n'.join([name, *lines])):
                continue
            if name == 'hooks':
                lines = [line for line in lines if not LEGACY.search(line)]
        kept.append((name, lines))
    text = '\n'.join(line for _, lines in kept for line in lines).rstrip()
    if entry is not None:
        text = (text + '\n\n' if text else '') + '\n'.join(codex_block(entry, approval, server))
    text = text + '\n' if text else ''
    try:
        tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise Failure('setup-failed', f'the Codex configuration would not parse: {error}') from None
    if text != (original.rstrip() + '\n' if original.strip() else ''):
        write_text(CODEX_CONFIG, text)


EDITORS = {'claude-code': edit_claude, 'codex': edit_codex}


def agent_directories():
    """USER_BIN and the version managers' bin directories that exist (newest nvm Node first)."""
    found = []
    for pattern in AGENT_BIN_GLOBS:
        found += sorted(glob.glob(pattern), reverse=True)
    return [*USER_BIN, *found]


def binary_installed(name, directories=None):
    """The `os.stat_result` of the executable `name` in the agents' install directories, or None."""
    for directory in directories or agent_directories():
        path = Path(directory) / name
        if path.is_file() and os.access(path, os.X_OK):
            return path.stat()
    return None


@contextlib.contextmanager
def editing():
    """Takes the lock for editing the agents' configuration files, so a registration of the driver
    and one of another server never overwrite each other's change (waits at most a minute)."""
    STATE.mkdir(mode=0o755, parents=True, exist_ok=True)
    with open(STATE / 'edit.lock', 'a') as handle:
        deadline = time.monotonic() + 60
        while True:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise Failure('busy', 'another registration is running') from None
                time.sleep(0.2)
        yield


def configure(names, approval):
    """Registers the driver with the agents in `names` that are installed. Returns
    `(registered, pending, failed)`: pending agents are not installed yet."""
    entry = server_entry()
    registered, pending, failed = [], [], []
    repair_agent_files()
    for name in names:
        if not binary_installed(AGENTS[name]):
            pending.append(name)
            if approval == 'ask':
                # Registers nothing; only removes the auto approval a kept fork may have left.
                try:
                    with editing(), as_user():
                        EDITORS[name](None, 'ask')
                except (Failure, OSError, ValueError, TypeError, tomllib.TOMLDecodeError) as error:
                    log(f'could not clear the auto approval for {name}: {type(error).__name__}: {error}')
            continue
        try:
            with editing(), as_user():
                EDITORS[name](entry, approval)
            registered.append(name)
        except (Failure, OSError, ValueError, TypeError, tomllib.TOMLDecodeError) as error:
            # One agent's unusual configuration fails only that agent.
            log(f'could not register {SERVER} with {name}: {type(error).__name__}: {error}')
            failed.append(name)
    return registered, pending, failed


# ------------------------------------------------- other MCP servers (Silo's own tools)

SERVER_NAME = re.compile(r'[a-z][a-z0-9-]{0,31}')


def plain_entry(command, args):
    """A stdio server entry marked as Silo's, in the shape both agents take."""
    return {'type': 'stdio', 'command': str(command), 'args': [str(a) for a in args],
            'env': dict(MANAGED_ENV)}


def valid_server(name, command=None, args=()):
    if name == SERVER or not SERVER_NAME.fullmatch(name):
        raise Failure('invalid-server', f'{name!r} is not a server name Silo may register')
    if command is not None:
        if not command.startswith('/') or '\0' in command or any('\0' in a for a in args):
            raise Failure('invalid-server', 'the command must be an absolute path')


def configure_server(name, command, args):
    """Registers the plain stdio MCP server `name` with the installed agents, the way the driver
    is registered (`configure`): returns `(registered, pending, failed, conflicts)`, pending being
    agents that are not installed yet and a conflict an agent that has a server of that name
    which is not Silo's (left as it is)."""
    valid_server(name, command, args)
    entry = plain_entry(command, args)
    registered, pending, failed, conflicts = [], [], [], []
    repair_agent_files()
    for agent in sorted(AGENTS):
        if not binary_installed(AGENTS[agent]):
            pending.append(agent)
            continue
        try:
            with editing(), as_user():
                EDITORS[agent](entry, None, server=name, known=entry)
            registered.append(agent)
        except Conflict as error:
            log(f'could not register {name} with {agent}: {error}')
            conflicts.append(agent)
        except (Failure, OSError, ValueError, TypeError, tomllib.TOMLDecodeError) as error:
            log(f'could not register {name} with {agent}: {type(error).__name__}: {error}')
            failed.append(agent)
    return registered, pending, failed, conflicts


def remove_server(name, command=None, args=()):
    """Takes the server `name` out of every agent's configuration when it is Silo's (a file that
    does not exist is not created; `command` and `args` name the unmarked entry an earlier Silo
    wrote). Returns `(removed, failed, conflicts)`."""
    valid_server(name, command, args)
    known = plain_entry(command, args) if command else None
    removed, failed, conflicts = [], [], []
    repair_agent_files()
    for agent in sorted(AGENTS):
        try:
            with editing(), as_user():
                EDITORS[agent](None, None, server=name, remove=True, known=known)
            removed.append(agent)
        except Conflict as error:
            log(f'could not remove {name} from {agent}: {error}')
            conflicts.append(agent)
        except (Failure, OSError, ValueError, TypeError, tomllib.TOMLDecodeError) as error:
            log(f'could not remove {name} from {agent}: {type(error).__name__}: {error}')
            failed.append(agent)
    return removed, failed, conflicts


def remove_legacy_user_files():
    """As the working account: LCU's data, its `lcu*` skills and its Pi package entry. Pi's
    settings file keeps everything else and is never created."""
    if LEGACY_DATA.is_symlink() or LEGACY_DATA.is_file():
        LEGACY_DATA.unlink()
    elif LEGACY_DATA.is_dir():
        shutil.rmtree(LEGACY_DATA)
    for directory in LEGACY_SKILL_DIRS:
        if directory.is_symlink() or not directory.is_dir():
            continue
        for path in directory.glob('lcu*'):
            if path.is_symlink() or path.is_file():
                path.unlink()
            elif path.is_dir():
                shutil.rmtree(path)
    if PI_SETTINGS.is_symlink() or not PI_SETTINGS.is_file():
        return
    settings = read_object(PI_SETTINGS)
    packages = settings.get('packages') if settings else None
    if isinstance(packages, list):
        kept = [p for p in packages if '/lcu/' not in (p if isinstance(p, str) else json.dumps(p))]
        if kept != packages:
            settings['packages'] = kept
            write_text(PI_SETTINGS, json.dumps(settings, indent=2) + '\n')


def remove_legacy():
    """Removes what the earlier LCU setup left: its agent entries (a file that does not exist
    is not created; Silo's own registration and approval stay as they are), its skills, data
    and Pi package, /opt/lcu, its launchers and its receipt."""
    repair_agent_files()
    for name, editor in EDITORS.items():
        try:
            with as_user():
                editor(None, None)
        except (Failure, OSError, ValueError, TypeError, tomllib.TOMLDecodeError) as error:
            log(f'could not remove the LCU entries for {name}: {type(error).__name__}: {error}')
    try:
        with as_user():
            remove_legacy_user_files()
    except (OSError, ValueError, TypeError) as error:
        log(f'could not remove the LCU files of the working account: {type(error).__name__}: {error}')
    try:
        if LEGACY_PREFIX.is_symlink() or LEGACY_PREFIX.is_file():
            LEGACY_PREFIX.unlink()
        elif LEGACY_PREFIX.is_dir():
            shutil.rmtree(LEGACY_PREFIX)
        LEGACY_RECEIPT.unlink(missing_ok=True)
        for path in DRIVER_LINK.parent.glob('lcu*'):
            if path.is_symlink() and str(LEGACY_PREFIX) in os.readlink(path):
                path.unlink()
        for path in PROFILE_HOOK.parent.glob('lcu*.sh'):
            path.unlink()
    except OSError as error:
        log(f'could not remove the LCU files: {error}')


def write_hook():
    """The login-shell fallback: a shell of the working account registers pending agents."""
    try:
        if PROFILE_HOOK.is_file() and PROFILE_HOOK.read_text() == HOOK:
            return
        PROFILE_HOOK.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
        fd, temporary = tempfile.mkstemp(prefix='.tmp-', dir=PROFILE_HOOK.parent)
        with os.fdopen(fd, 'w') as output:
            output.write(HOOK)
            os.fchmod(output.fileno(), 0o644)
        os.replace(temporary, PROFILE_HOOK)
    except OSError as error:
        log(f'could not write {PROFILE_HOOK}: {error}')


def spawn_watcher():
    subprocess.Popen([HELPER, 'watch'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)


def watcher_running():
    try:
        STATE.mkdir(mode=0o755, parents=True, exist_ok=True)
        with open(WATCH_LOCK, 'a') as handle:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                return True
            fcntl.flock(handle, fcntl.LOCK_UN)
    except OSError:
        return True
    return False


def ensure_triggers(pending):
    """Keeps a pending agent from waiting for the next boot: the login hook is in place and
    one watcher polls for the agents' binaries."""
    write_hook()
    if pending and not watcher_running():
        try:
            spawn_watcher()
        except OSError as error:
            log(f'could not start the watcher: {error}')


def watch(interval=WATCH_INTERVAL, directories=None, rounds=None):
    """Runs until nothing is pending, the receipt is no longer `ready`, or WATCH_MAX has passed:
    when a pending agent's binary appears (or is replaced), runs `reconcile` once for it.
    Polling a few paths is cheap and needs neither an init system nor inotify tools in the
    image. One instance holds WATCH_LOCK."""
    STATE.mkdir(mode=0o755, parents=True, exist_ok=True)
    attempted = {}
    with open(WATCH_LOCK, 'a') as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        deadline = time.monotonic() + WATCH_MAX
        while rounds is None or rounds > 0:
            receipt = read_json(RECEIPT) or {}
            if receipt.get('state') != 'ready' or time.monotonic() >= deadline:
                return
            pending = [n for n in receipt.get('pending') or [] if isinstance(n, str)]
            if not pending:
                return
            for name in sorted({EXECUTABLES.get(n, n) for n in pending}):
                info = binary_installed(name, directories)
                identity = (info.st_ino, info.st_mtime_ns) if info else None
                if identity and attempted.get(name) != identity:
                    attempted[name] = identity
                    log(f'{name} appeared; reconciling')
                    try:
                        subprocess.run([HELPER, 'reconcile'], stdin=subprocess.DEVNULL,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       check=False, timeout=900)
                    except (OSError, subprocess.TimeoutExpired) as error:
                        log(f'reconcile after {name} appeared failed: {error}')
            if rounds is not None:
                rounds -= 1
            time.sleep(interval)




def registrations_intact(agents, mode):
    """True when every agent in `agents` still has Silo's registration, with the approval of
    `mode`, in its configuration: a cheap read, so a file the user (or an agent update)
    rewrote sends the apply through the full configuration again."""
    entry = server_entry()
    try:
        with as_user():
            for name in agents:
                if name == 'claude-code':
                    state = read_object(CLAUDE_STATE) or {}
                    if (state.get('mcpServers') or {}).get(SERVER) != entry:
                        return False
                    settings = read_object(CLAUDE_SETTINGS) or {}
                    allow = (settings.get('permissions') or {}).get('allow') or []
                    if (APPROVAL_RULE in allow) != (mode == 'auto'):
                        return False
                elif name == 'codex':
                    config = tomllib.loads(CODEX_CONFIG.read_text())
                    server = (config.get('mcp_servers') or {}).get(SERVER) or {}
                    if server.get('command') != entry['command'] or server.get('env') != entry['env']:
                        return False
                    if (server.get('default_tools_approval_mode') == 'approve') != (mode == 'auto'):
                        return False
                else:
                    return False
    except (OSError, ValueError, TypeError, AttributeError, tomllib.TOMLDecodeError):
        return False
    return True


def reconcile_agents(receipt):
    """Registers the agents in the receipt's `pending` list whose binary now exists. Returns
    `(agents, pending)`; an agent that could not be registered stays pending."""
    approval = receipt.get('approval') if receipt.get('approval') in APPROVALS else 'ask'
    agents, pending = list(receipt.get('agents') or []), list(receipt.get('pending') or [])
    names = [name for name in sorted(pending) if name in AGENTS]
    registered, still_pending, failed = configure(names, approval) if names else ([], [], [])
    agents = sorted(set(agents) | set(registered))
    pending = sorted((set(pending) - set(registered)) | set(still_pending) | set(failed))
    return agents, pending


def reconcile():
    """The `reconcile` command: registers pending agents that are now installed and updates
    the receipt. Skips quietly while another run holds the lock (that run reconciles)."""
    pinned = load_pinned()
    receipt = read_json(RECEIPT)
    if pinned is None or not receipt or receipt.get('state') != 'ready' or not matches_pair(receipt, pinned):
        return {'reconcile': 'skipped'}
    STATE.mkdir(mode=0o755, parents=True, exist_ok=True)
    with open(LOCK, 'a') as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return {'reconcile': 'busy'}
        write_launcher()
        write_privacy_env()
        write_browser_launchers()
        agents, pending = reconcile_agents(receipt)
        if (agents, pending) != (receipt.get('agents') or [], receipt.get('pending') or []):
            write_json(RECEIPT, dict(receipt, agents=agents, pending=pending, updatedAt=now()))
    return {'reconcile': 'done', 'agents': agents, 'pending': pending}


def desktop_session():
    """The session state `silo-desktop status` reports (`running`, `starting`, `failed`,
    `stopped`), or None when it cannot be read."""
    try:
        result = subprocess.run(DESKTOP, stdin=subprocess.DEVNULL, capture_output=True,
                                text=True, timeout=30)
        state = json.loads(result.stdout)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return None
    value = state.get('sessionState', state.get('state')) if isinstance(state, dict) else None
    return value if isinstance(value, str) else None


def desktop_autostart():
    """Whether the desktop starts with the computer (the default when never configured)."""
    value = read_json(DESKTOP_CONFIG)
    return not value or value.get('autoStart') is not False


def start_desktop():
    run([DESKTOP_COMMAND, 'start'], timeout=180, check=False, quiet=True)


def wait_for_session(wait, repair):
    """Waits (bounded) for the desktop session; with `repair`, a session that failed or
    stopped is started again up to SESSION_REPAIR_ATTEMPTS times, then the wait ends."""
    deadline = time.monotonic() + wait
    repairs = 0
    while True:
        state = desktop_session()
        if state == 'running':
            return
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise Failure('session-unavailable')
        if repair and state in ('failed', 'stopped') and desktop_autostart():
            if repairs >= SESSION_REPAIR_ATTEMPTS:
                raise Failure('session-unavailable')
            repairs += 1
            log(f'desktop session is {state}; starting it again ({repairs} of {SESSION_REPAIR_ATTEMPTS})')
            time.sleep(min(SESSION_REPAIR_BASE * 2 ** (repairs - 1), remaining))
            if time.monotonic() >= deadline:
                raise Failure('session-unavailable')
            start_desktop()
            continue
        if time.monotonic() >= deadline:
            raise Failure('session-unavailable')
        time.sleep(2)


def doctor(wait, repair=False):
    """Waits for the desktop session, then runs `cua-driver doctor --json` as the working
    account in it. True when the report is ok and the probes of the display server, the X11
    connection and the accessibility bus are ok (the driver cannot drive windows otherwise)."""
    wait_for_session(wait, repair)
    session = session_env(strict=True)
    result = run([str(DRIVER_LINK), 'doctor', '--json'], user=True, timeout=300, check=False,
                 extra_env={**session, **PRIVACY_ENV})
    if result.returncode != 0:
        return False
    try:
        report = json.loads(result.stdout)
    except ValueError:
        return False
    if not isinstance(report, dict) or report.get('ok') is not True:
        return False
    states = {probe.get('label'): probe.get('status') for probe in report.get('probes') or []
              if isinstance(probe, dict)}
    return all(states.get(label) == 'ok' for label in DOCTOR_REQUIRED)


def write_receipt(pinned, approval, state, reason=None, **fields):
    receipt = {'schemaVersion': SCHEMA, 'state': state, 'reason': reason,
               'driverVersion': pinned['version'], 'approval': approval, 'updatedAt': now()}
    receipt.update(fields)
    write_json(RECEIPT, receipt)
    return receipt


def report(approval, outcome, reason=None):
    """This run's approval outcome, as the host records it."""
    return {'approval': approval, 'outcome': outcome, 'reason': reason}


def apply(approval, force=False, boot=False):
    """Brings computer use up to date for the pinned driver with `approval` configured in
    the agents. Returns the public status plus `apply`, this run's approval outcome.

    The host decides the mode, serializes runs for the computer and keeps the result; nothing is
    remembered here beyond the receipt, so a request is never ignored as stale."""
    pinned = load_pinned()
    if pinned is None:
        return dict(status(None), apply=report(approval, 'failed', 'not-configured'))
    STATE.mkdir(mode=0o755, parents=True, exist_ok=True)
    with open(LOCK, 'a') as handle:
        deadline = time.monotonic() + LOCK_WAIT
        while True:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise Failure('busy') from None
                time.sleep(1)
        outcome = update(pinned, approval, force, boot)
    # The lock is released: `status` reports `installing` only while another run holds it.
    return dict(status(pinned), apply=outcome)


def update(pinned, mode, force, boot):
    """One run for `mode`; returns its approval report (`report`)."""
    existing = read_json(RECEIPT)
    configured = bool(not force and existing and existing.get('state') == 'ready'
                      and matches(existing, pinned, mode))
    if (configured and not boot and shim_is_current(pinned['version'])
            and driver_version(DRIVER_LINK) == pinned['version']
            and registrations_intact(existing.get('agents') or [], mode)):
        # The receipt shows this mode applied completely. An agent installed since then is
        # registered, and registrations follow a desktop session that was started again.
        write_launcher()
        write_privacy_env()
        write_browser_launchers()
        agents, pending = reconcile_agents(existing)
        if (agents, pending) != (existing.get('agents') or [], existing.get('pending') or []):
            write_json(RECEIPT, dict(existing, agents=agents, pending=pending, updatedAt=now()))
        ensure_triggers(pending)
        return report(mode, 'applied')
    write_receipt(pinned, mode, 'installing')
    # Set once the registration is made: later failures (the readiness check) do not change it.
    result = fields = None
    try:
        STAGE.mkdir(mode=0o700, parents=True, exist_ok=True)
        ensure_driver(pinned, STAGE)
        remove_legacy()
        # The launcher finds the session when an agent starts the server, so the registration
        # does not wait for the desktop; a session that is not running fails the run after it.
        session_error = None
        try:
            wait_for_session(SESSION_WAIT_BOOT if boot else SESSION_WAIT, repair=boot)
        except Failure as failure:
            session_error = failure
        write_launcher()
        write_privacy_env()
        write_browser_launchers()
        registered, pending, failed = configure(sorted(AGENTS), mode)
        if failed and not registered:
            raise Failure('setup-failed', 'could not register the driver with ' + ', '.join(failed))
        outcome = 'partial' if failed else 'applied'
        result = report(mode, outcome, 'setup-partial' if failed else None)
        pending = sorted(set(pending) | set(failed))
        ensure_triggers(pending)
        fields = dict(agents=registered, pending=pending, approvalOutcome=outcome)
        if session_error is not None:
            raise Failure('session-unavailable', str(session_error))
        if not doctor(0):
            raise Failure('doctor-failed')
        write_receipt(pinned, mode, 'ready', verifiedAt=now(), **fields)
    except Failure as failure:
        log(f'computer use setup failed: {failure.reason}: {failure}')
        result = result or report(mode, 'failed', failure.reason)
        write_receipt(pinned, mode, 'failed', failure.reason,
                      **(fields or {'approvalOutcome': result['outcome']}))
    except Exception as error:  # noqa: BLE001 - the receipt must always record the end state
        log(f'computer use setup failed: {type(error).__name__}: {error}')
        result = result or report(mode, 'failed', 'setup-failed')
        write_receipt(pinned, mode, 'failed', 'setup-failed', approvalOutcome=result['outcome'])
    finally:
        subprocess.run(['rm', '-rf', str(STAGE)], check=False)
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('status')
    commands.add_parser('reconcile')
    commands.add_parser('watch')
    add_parser = commands.add_parser('server-add', help="register another stdio MCP server with the agents")
    add_parser.add_argument('--name', required=True)
    add_parser.add_argument('--command', dest='command_path', required=True)
    add_parser.add_argument('--arg', action='append', default=[])
    remove_parser = commands.add_parser('server-remove', help="take a server registered by server-add out of the agents")
    remove_parser.add_argument('--name', required=True)
    remove_parser.add_argument('--command', dest='command_path')
    remove_parser.add_argument('--arg', action='append', default=[])
    apply_parser = commands.add_parser('apply')
    apply_parser.add_argument('--approval', choices=APPROVALS, required=True)
    apply_parser.add_argument('--force', action='store_true')
    apply_parser.add_argument('--boot', action='store_true')
    apply_parser.add_argument('--creation', action='store_true',
                              help='never download (the host holds its operation gate)')
    args = parser.parse_args(argv)
    global CREATION
    CREATION = getattr(args, 'creation', False)
    if os.geteuid() != 0:
        print('Run as root', file=sys.stderr)
        return 1
    if args.command == 'watch':
        watch()
        return 0
    try:
        if args.command == 'server-add':
            registered, pending, failed, conflicts = configure_server(args.name, args.command_path, args.arg)
            result = {'server': args.name, 'registered': registered, 'pending': pending, 'failed': failed,
                      'conflicts': conflicts}
            print(json.dumps(result, sort_keys=True))
            return 1 if failed or conflicts else 0
        if args.command == 'server-remove':
            removed, failed, conflicts = remove_server(args.name, args.command_path, args.arg)
            print(json.dumps({'server': args.name, 'removed': removed, 'failed': failed,
                              'conflicts': conflicts}, sort_keys=True))
            return 1 if failed or conflicts else 0
        if args.command == 'status':
            result = status()
        elif args.command == 'reconcile':
            result = reconcile()
        else:
            result = apply(args.approval, args.force, args.boot)
    except Failure as failure:
        # The run could not even start (another run held the lock): still a report.
        result = {'schemaVersion': SCHEMA, 'state': 'failed', 'reason': failure.reason}
        if args.command in ('server-add', 'server-remove'):
            print(json.dumps({'server': args.name, 'error': failure.reason}, sort_keys=True))
            return 1
        if args.command == 'apply':
            result['apply'] = report(args.approval, 'failed', failure.reason)
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
