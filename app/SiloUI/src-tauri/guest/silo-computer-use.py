#!/usr/bin/python3
"""Built-in computer use for a Silo guest: LCU against the host's read-only ChatGPT app.

Silo pushes this helper and the pinned pair (`pinned.json`) into the guest, then runs
`apply --approval ask|auto` after each boot, whenever the app becomes ready and when the
user changes the computer's approval switch. The helper is a plain executor: the host decides
the mode, serializes the runs and keeps the result. `apply` is idempotent and cheap when
nothing changed:

* the pinned ChatGPT app folder must be mounted read-only at /opt/silo/chatgpt;
* the pinned LCU archive (staged in the guest image, or downloaded and hash-checked)
  is extracted to local disk and installed in place against that folder;
* `lcu setup --agent all --allow-missing --session direct --yes --approval <mode>` runs as
  `silo` (LCU 0.8.8 and later: every supported agent is registered, and those not installed
  yet are recorded as pending; an older LCU falls back to `--agent auto`). From LCU 0.11.0 it
  also passes `--cross-turn on --unattended`, which keeps Computer Use across turns;
* `lcu setup --reconcile` registers a pending agent once its binary exists. It runs at the
  end of every `apply` (a boot included), from the `reconcile` command, from a login hook in
  /etc/profile.d and from a small `watch` process that polls the install directories while
  agents are pending (the guest has no init system to host a `.path` unit);
* `lcu status --json` and `lcu doctor` (inside the desktop session) are recorded. After a
  boot the helper waits for the session (bounded) and, when the session ended up failed
  or stopped although the desktop starts with the computer, asks `silo-desktop start` for it
  again a few times with backoff instead of failing the receipt.

The result is a receipt under /var/lib/silo-computer-use that `status` projects
for the host, and `apply` prints that status plus this run's approval outcome
(`applied`, `partial` when `lcu setup` configured some agents and failed for others, or
`failed`); agents still to be installed are pending, not a failure. Nothing here talks to the host or holds credentials. The approval switch
configures agents' own approval prompts; agents in the computer have root, so it is a
convenience and not a security boundary, and the helper keeps no record to defend.
"""
import argparse
import fcntl
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

STATE = Path('/var/lib/silo-computer-use')
PINNED = STATE / 'pinned.json'
RECEIPT = STATE / 'receipt.json'
LOCK = STATE / 'lock'
STAGE = STATE / 'stage'
LOG = Path('/var/log/silo-computer-use.log')
IMAGE_DIR = Path('/usr/local/share/silo/lcu')
# The host lends its verified archive here (read-only) to sandboxes created with it.
HOST_DIR = Path('/opt/silo/lcu')
PREFIX = Path('/opt/lcu')
MOUNT = Path('/opt/silo/chatgpt')
DESKTOP_COMMAND = '/usr/local/bin/silo-desktop'
DESKTOP = [DESKTOP_COMMAND, 'status']
DESKTOP_CONFIG = Path('/var/lib/silo-desktop/config.json')
USER = 'silo'
HOME = '/home/silo'
HELPER = '/usr/local/libexec/silo-computer-use'
# The working account's PATH for LCU: the directories the agents' installers use.
USER_BIN = [f'{HOME}/.local/bin', f'{HOME}/.bun/bin', f'{HOME}/.cargo/bin', f'{HOME}/.npm-global/bin',
            '/usr/local/bin', '/usr/bin', '/bin']
USER_PATH = ':'.join(USER_BIN)
# Executable names of agents whose LCU id differs from them.
EXECUTABLES = {'oh-my-pi': 'omp'}
WATCH_LOCK = STATE / 'watch.lock'
WATCH_INTERVAL = 5
PROFILE_HOOK = Path('/etc/profile.d/silo-computer-use.sh')
HOOK = '''# Registers agents installed after computer use was set up (silo-computer-use).
if [ "$(id -un)" = silo ] && grep -q '"pending": \\["' /var/lib/silo-computer-use/receipt.json 2>/dev/null; then
    (sudo -n /usr/local/libexec/silo-computer-use reconcile >/dev/null 2>&1 &)
fi
'''
SCHEMA = 1
APP_NAME = re.compile(r'[0-9][0-9A-Za-z.+~-]{0,63}-(arm64|amd64)')
VERSION = re.compile(r'[0-9][0-9A-Za-z.+~-]{0,63}')
SHA256 = re.compile(r'[0-9a-f]{64}')
ARCHIVE_NAME = re.compile(r'lcu-[0-9][0-9A-Za-z.+~-]{0,31}-linux-(arm64|x64)\.tar\.gz')
APPROVALS = ('ask', 'auto')
COMPATIBILITY = ('tested', 'untested', 'unknown')
SESSION_WAIT_BOOT = 300
SESSION_WAIT = 90
# After a boot a failed or stopped session is started again this many times, waiting
# SESSION_REPAIR_BASE * 2 ** (n - 1) seconds before attempt n.
SESSION_REPAIR_ATTEMPTS = 3
SESSION_REPAIR_BASE = 2
LOCK_WAIT = 1800
# The LCU download: waits between attempts (so five attempts), the longest one curl
# attempt may take, and the total time after which no new attempt starts.
DOWNLOAD_BACKOFF = (5, 10, 20, 40)
DOWNLOAD_MAX_TIME = 240
DOWNLOAD_BUDGET = 420


class Failure(Exception):
    """A step failed in a way the receipt can name (`reason` is a stable code)."""

    def __init__(self, reason, detail=''):
        super().__init__(detail or reason)
        self.reason = reason


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


def load_pinned():
    """The pair the host pinned, or None when absent or malformed."""
    value = read_json(PINNED)
    if not value or value.get('schemaVersion') != SCHEMA:
        return None
    app, lcu = value.get('app'), value.get('lcu')
    try:
        if (not APP_NAME.fullmatch(app['dir']) or not VERSION.fullmatch(app['version']) or
                not VERSION.fullmatch(lcu['version']) or not SHA256.fullmatch(lcu['sha256']) or
                not ARCHIVE_NAME.fullmatch(lcu['archive']) or
                not str(lcu['url']).startswith('https://') or
                not (app.get('runtime') is None or isinstance(app['runtime'], str))):
            return None
    except (KeyError, TypeError):
        return None
    return value


def mount_state(mount=MOUNT, mounts=Path('/proc/mounts')):
    """`ok` for a read-only mount at the app folder, else `missing` or `writable`."""
    try:
        lines = mounts.read_text().splitlines()
    except OSError:
        return 'missing'
    found = None
    for line in lines:
        fields = line.split()
        if len(fields) >= 4 and fields[1] == str(mount):
            found = fields[3].split(',')
    if found is None:
        return 'missing'
    return 'ok' if 'ro' in found else 'writable'


def app_folder(pinned):
    return MOUNT / pinned['app']['dir']


def app_present(pinned):
    folder = app_folder(pinned)
    try:
        info = folder.lstat()
    except OSError:
        return False
    return folder.is_dir() and not folder.is_symlink() and bool(info)


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
    return (receipt.get('schemaVersion') == SCHEMA and receipt.get('appDir') == pinned['app']['dir']
            and receipt.get('lcuVersion') == pinned['lcu']['version']
            and receipt.get('archiveSha256') == pinned['lcu']['sha256'])


def matches(receipt, pinned, approval):
    """Whether the receipt shows a run for this pair that applied `approval` completely."""
    return (matches_pair(receipt, pinned) and receipt.get('approval') == approval
            and receipt.get('approvalOutcome') == 'applied')


def clean(value, pattern=VERSION):
    return value if isinstance(value, str) and pattern.fullmatch(value) else None


def clean_text(value, limit=300):
    if not isinstance(value, str):
        return None
    value = ''.join(ch for ch in value if ch.isprintable())[:limit].strip()
    return value or None


def status(pinned=None, receipt=None):
    """What the host shows, from the receipt alone. Never runs LCU."""
    pinned = pinned or load_pinned()
    mount = mount_state()
    result = {'schemaVersion': SCHEMA, 'state': 'not-set-up', 'reason': None, 'mount': mount,
              'compatibility': None, 'warning': None, 'appVersion': None,
              'runtimeVersion': None, 'lcuVersion': None, 'agents': None, 'readiness': None}
    if pinned is None:
        result['reason'] = 'not-configured'
        return result
    if not app_present(pinned):
        result.update(state='needs-app', reason='app-missing')
        return result
    receipt = receipt if receipt is not None else read_json(RECEIPT)
    held = lock_held()
    # The receipt describes the last run, whatever mode it applied.
    if receipt and matches_pair(receipt, pinned):
        state = receipt.get('state')
        if state == 'installing' and not held:
            result.update(state='failed', reason='interrupted')
        elif state in ('installing', 'ready', 'failed'):
            result.update(state=state, reason=clean(receipt.get('reason'), re.compile(r'[a-z0-9-]{1,40}')))
        if state in ('ready', 'failed'):
            compat = receipt.get('compatibility')
            result.update(
                compatibility=compat if compat in COMPATIBILITY else None,
                warning=clean_text(receipt.get('warning')),
                appVersion=clean(receipt.get('appVersion')),
                runtimeVersion=clean_text(receipt.get('runtimeVersion'), 64),
                lcuVersion=clean(receipt.get('lcuVersion')),
                readiness=receipt.get('readiness') if receipt.get('readiness') in ('ready', 'failed', 'unverified') else None,
                agents=sorted(a for a in receipt.get('agents') or []
                              if isinstance(a, str) and re.fullmatch(r'[a-z0-9-]{1,32}', a)))
    elif held:
        result.update(state='installing')
    if mount != 'ok' and result['state'] in ('ready', 'not-set-up'):
        result.update(state='failed', reason='mount-' + mount)
    return result


def run(argv, *, user=False, timeout=900, check=True, cwd=None, extra_env=None, quiet=False):
    """Runs a command with no stdin, appending its output to the log (except a listing)."""
    if user:
        argv = ['runuser', '-u', USER, '--', 'env', f'HOME={HOME}', f'USER={USER}',
                f'LOGNAME={USER}', f'PATH={USER_PATH}', *argv]
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
    a mismatch. Raises `lcu-archive-unavailable` once the attempts or the time run out.
    """
    attempts = len(DOWNLOAD_BACKOFF) + 1
    deadline = time.monotonic() + DOWNLOAD_BUDGET
    for attempt in range(1, attempts + 1):
        log(f'downloading the LCU archive (attempt {attempt} of {attempts})')
        try:
            run(['curl', '--silent', '--show-error', '--fail', '--location',
                 '--retry', '3', '--retry-delay', '2', '--retry-all-errors', '--retry-connrefused',
                 '--connect-timeout', '30', '--max-time', str(DOWNLOAD_MAX_TIME),
                 '--proto', '=https', '--tlsv1.2', url, '--output', str(target)],
                timeout=DOWNLOAD_MAX_TIME + 60)
            return
        except Failure as failure:
            log(f'LCU download attempt {attempt} failed: {failure}')
        target.unlink(missing_ok=True)
        if attempt == attempts:
            break
        delay = DOWNLOAD_BACKOFF[attempt - 1]
        if time.monotonic() + delay >= deadline:
            break
        log(f'retrying the LCU download in {delay}s')
        time.sleep(delay)
    raise Failure('lcu-archive-unavailable', 'could not download the LCU archive (network)')


def archive_path(pinned, stage):
    """The pinned archive: the host's read-only copy, else the one staged in the image, each
    only when its hash matches, else a hash-checked download."""
    lcu = pinned['lcu']
    for directory in (HOST_DIR, IMAGE_DIR):
        candidate = directory / lcu['archive']
        if candidate.is_file() and not candidate.is_symlink() and sha256_file(candidate) == lcu['sha256']:
            return candidate
    target = stage / lcu['archive']
    download(lcu['url'], target)
    if sha256_file(target) != lcu['sha256']:
        target.unlink(missing_ok=True)
        raise Failure('lcu-archive-mismatch')
    return target


def member_is_safe(name):
    parts = Path(name).parts
    return bool(parts) and not name.startswith('/') and '..' not in parts


def extract(archive, stage, root_name):
    listing = run(['tar', '-tzf', str(archive)], timeout=300, quiet=True).stdout.splitlines()
    if not listing or any(not member_is_safe(line) or line.split('/')[0] != root_name for line in listing):
        raise Failure('lcu-archive-invalid')
    run(['tar', '-xzf', str(archive), '-C', str(stage), '--no-same-owner'], timeout=600)
    source = stage / root_name
    if not (source / 'scripts/install.sh').is_file():
        raise Failure('lcu-archive-invalid')
    return source


def lcu_command(name):
    return str(PREFIX / 'current/bin' / name)


def lcu_status():
    """`lcu status --json` as the working account, or None when LCU is not usable."""
    try:
        result = run([lcu_command('lcu'), 'status', '--json'], user=True, timeout=120, check=False)
    except (Failure, OSError):
        return None
    if result.returncode != 0:
        return None
    try:
        value = json.loads(result.stdout)
    except ValueError:
        return None
    return value if isinstance(value, dict) else None


def install(pinned, stage):
    lcu = pinned['lcu']
    root_name = lcu['archive'][:-len('.tar.gz')]
    archive = archive_path(pinned, stage)
    source = extract(archive, stage, root_name)
    run(['./scripts/install.sh', '--user', USER, '--runtime-only', '--skip-system', '--offline',
         '--existing-app', str(app_folder(pinned)), '--yes'], cwd=source, timeout=900)


def lcu_supports(flag):
    """Whether this LCU's `setup` documents `flag` (`--allow-missing` and `--reconcile`: LCU 0.8.8)."""
    result = run([lcu_command('lcu'), 'setup', '--help'], user=True, timeout=60, check=False, quiet=True)
    return result.returncode == 0 and flag in (result.stdout or '')


def setup(approval):
    """Runs `lcu setup` for the working account and reports this run's outcome:
    `(outcome, agents, reason, stored)`, see `classify_setup`; `stored` is false when
    cross-turn was requested and LCU does not report it on.

    Every supported agent is registered, and one that is not installed yet is recorded as
    pending (`--agent all --allow-missing`). An LCU without that flag registers the agents
    it detects instead. An LCU with `--cross-turn` (0.11.0) also keeps Computer Use available
    across turns; `--unattended` skips the owner prompt, as no person is present in the
    computer. Running it again leaves the setting on."""
    agents = ['--agent', 'all', '--allow-missing'] if lcu_supports('--allow-missing') else ['--agent', 'auto']
    cross_turn = ['--cross-turn', 'on', '--unattended'] if lcu_supports('--cross-turn') else []
    result = run([lcu_command('lcu'), 'setup', *agents, *cross_turn, '--session', 'direct', '--yes',
                  '--approval', approval], user=True, timeout=600, check=False)
    outcome, agents, reason = classify_setup(result.returncode, result.stdout, approval)
    # `lcu setup` reports a failure to store the setting as a warning and still exits 0.
    stored = not cross_turn or outcome == 'failed' or cross_turn_enabled(lcu_status())
    return outcome, agents, reason, stored


def cross_turn_enabled(report):
    """Whether `lcu status --json` shows Computer Use kept across turns."""
    value = report.get('cross_turn') if isinstance(report, dict) else None
    return isinstance(value, dict) and value.get('enabled') is True


def pending_agents(report):
    """The agents `lcu status --json` lists as pending (not installed yet), or None when this
    LCU does not report them."""
    value = report.get('pending') if isinstance(report, dict) else None
    if not isinstance(value, list):
        return None
    names = []
    for item in value:
        name = item.get('id', item.get('name')) if isinstance(item, dict) else item
        if isinstance(name, str) and re.fullmatch(r'[a-z0-9-]{1,32}', name) and name not in names:
            names.append(name)
    return sorted(names)


def reconcile_agents(agents, pending, always):
    """Registers the pending agents whose binary now exists (`lcu setup --reconcile`, a quiet
    no-op otherwise). Returns the registered and the still pending agents. Without `always`
    it runs only while something is pending."""
    if not (always or pending) or not Path(lcu_command('lcu')).exists():
        return agents, pending
    if not lcu_supports('--reconcile'):
        return agents, pending
    result = run([lcu_command('lcu'), 'setup', '--reconcile'], user=True, timeout=600, check=False,
                 quiet=True)
    if result.returncode != 0:
        log(f'lcu setup --reconcile failed: {(result.stdout or "")[-300:]}')
        return agents, pending
    after = pending_agents(lcu_status())
    if after is None:
        return agents, pending
    return sorted(set(agents) | {name for name in pending if name not in after}), after


def binary_installed(name, directories=None):
    """The `os.stat_result` of the executable `name` in the agents' install directories, or None."""
    for directory in directories or USER_BIN:
        path = Path(directory) / name
        if path.is_file() and os.access(path, os.X_OK):
            return path.stat()
    return None


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
    """Runs until nothing is pending: when a pending agent's binary appears (or is replaced),
    runs `reconcile` once for it. Polling a few paths is cheap and needs neither an init
    system nor inotify tools in the image. One instance holds WATCH_LOCK."""
    STATE.mkdir(mode=0o755, parents=True, exist_ok=True)
    attempted = {}
    with open(WATCH_LOCK, 'a') as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        while rounds is None or rounds > 0:
            pending = [n for n in (read_json(RECEIPT) or {}).get('pending') or [] if isinstance(n, str)]
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
        agents, pending = list(receipt.get('agents') or []), list(receipt.get('pending') or [])
        registered, remaining = reconcile_agents(agents, pending, always=True)
        if (registered, remaining) != (agents, pending):
            write_json(RECEIPT, dict(receipt, agents=registered, pending=remaining, updatedAt=now()))
    return {'reconcile': 'done', 'agents': registered, 'pending': remaining}


AGENT_LABEL = r'([A-Z][A-Za-z ]{1,24})'
# LCU v0.8.1 `configure` (lcu/setup.py) prints `<label>: <phase> failed: <error>` for each
# failed phase to stderr (merged into the output here), and `<label>: approval <mode>:
# <outcome>.` once the approval of an agent whose final registration phase succeeded was
# applied.
FAILED_LINE = re.compile(AGENT_LABEL + r': ([A-Za-z ]{2,24}) failed: ')
APPROVED_LINE = re.compile(AGENT_LABEL + r': approval (?:ask|auto): ')
# `configure` carries on after this phase fails: the agent's registration and approval
# still run, so its failure says nothing about the approval.
UNRELATED_PHASES = {'old skill cleanup'}


def agent_name(label):
    return re.sub(r'[^a-z0-9]+', '-', label.lower()).strip('-')


def classify_setup(returncode, output, approval):
    """`(outcome, agents, reason)` of one `lcu setup` run from its exit status and its
    per-agent lines (`Codex: MCP registered.`, `Codex: approval ask: ...`,
    `Codex: approval failed: ...`, `Codex: old skill cleanup failed: ...`).

    The outcome is about the approval, tracked per agent and phase. `applied`: no agent
    failed in a phase that matters (the old skill cleanup does not, nor does a failure
    after the configuration was saved), and it either exited 0 or every agent it names had
    its approval applied. `partial`: some agents were configured and others failed, or it
    failed in a way the lines do not explain after configuring some (when unsure, partial:
    a guess of `failed` would claim nothing changed). `failed`: nothing was configured."""
    failed, approved, seen = set(), set(), set()
    for line in (output or '').splitlines():
        line = line.strip()
        if match := FAILED_LINE.match(line):
            name = agent_name(match.group(1))
            seen.add(name)
            if match.group(2).strip() not in UNRELATED_PHASES:
                failed.add(name)
        elif match := APPROVED_LINE.match(line):
            approved.add(agent_name(match.group(1)))
    agents = registered_agents(output)
    seen |= approved | set(agents)
    if not failed and (returncode == 0 or (approved and seen <= approved)):
        # An explicit approval success is kept whatever else went wrong around it.
        return 'applied', agents, None
    configured = (approved | set(agents)) - failed
    if configured:
        return 'partial', agents, 'setup-partial'
    return 'failed', agents, 'setup-failed'


def registered_agents(output):
    """Agents whose registration `lcu setup` confirmed (`Codex: MCP registered.`)."""
    names = []
    for line in (output or '').splitlines():
        match = re.fullmatch(AGENT_LABEL + r': [A-Za-z ]{2,24} registered\.', line.strip())
        if match:
            name = agent_name(match.group(1))
            if name not in names:
                names.append(name)
    return sorted(names)


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


def session_running():
    return desktop_session() == 'running'


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
            raise Failure('desktop-session-not-running')
        if repair and state in ('failed', 'stopped') and desktop_autostart():
            if repairs >= SESSION_REPAIR_ATTEMPTS:
                raise Failure('desktop-session-not-running')
            repairs += 1
            log(f'desktop session is {state}; starting it again ({repairs} of {SESSION_REPAIR_ATTEMPTS})')
            time.sleep(min(SESSION_REPAIR_BASE * 2 ** (repairs - 1), remaining))
            if time.monotonic() >= deadline:
                raise Failure('desktop-session-not-running')
            start_desktop()
            continue
        if time.monotonic() >= deadline:
            raise Failure('desktop-session-not-running')
        time.sleep(2)


def doctor(wait, repair=False):
    """True when `lcu doctor` reports ready inside the desktop session."""
    wait_for_session(wait, repair)
    # The launcher must run as the desktop account itself.
    result = run([lcu_command('lcu-session'), '--user', USER, '--', lcu_command('lcu'),
                  'doctor', '--non-interactive', '--require-ready'], user=True, timeout=300,
                 check=False)
    return result.returncode == 0


def digest(report):
    """The fields of `lcu status --json` Silo shows."""
    report = report or {}
    compat = report.get('compatibility') if isinstance(report.get('compatibility'), dict) else {}
    app = report.get('app') if isinstance(report.get('app'), dict) else {}
    return {
        'compatibility': compat.get('status') if compat.get('status') in COMPATIBILITY else 'unknown',
        'warning': clean_text(compat.get('warning')),
        'appVersion': clean(app.get('version')),
        'runtimeVersion': clean_text(app.get('runtime'), 64),
    }


def write_receipt(pinned, approval, state, reason=None, **fields):
    receipt = {'schemaVersion': SCHEMA, 'state': state, 'reason': reason,
               'appDir': pinned['app']['dir'], 'lcuVersion': pinned['lcu']['version'],
               'archiveSha256': pinned['lcu']['sha256'], 'approval': approval,
               'updatedAt': now()}
    receipt.update(fields)
    write_json(RECEIPT, receipt)
    return receipt


def report(approval, outcome, reason=None):
    """This run's approval outcome, as the host records it."""
    return {'approval': approval, 'outcome': outcome, 'reason': reason}


def apply(approval, force=False, boot=False):
    """Brings computer use up to date for the pinned pair with `approval` configured in
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
        mount = mount_state()
        if mount != 'ok':
            outcome = report(approval, 'failed', 'mount-' + mount)
        elif not app_present(pinned):
            outcome = report(approval, 'failed', 'app-missing')
        else:
            outcome = update(pinned, approval, force, boot)
    # The lock is released: `status` reports `installing` only while another run holds it.
    return dict(status(pinned), apply=outcome)


def installed_for(report, pinned):
    """Whether `lcu status --json` shows the pinned LCU installed against the pinned app folder."""
    if not report or report.get('lcu_version') != pinned['lcu']['version']:
        return False
    app = report.get('app') if isinstance(report.get('app'), dict) else {}
    # LCU links the app in place: its release folder's `app` resolves to the mounted folder.
    try:
        return os.path.realpath(app['path']) == os.path.realpath(app_folder(pinned))
    except (KeyError, TypeError, OSError):
        return False


def update(pinned, mode, force, boot):
    """One run for `mode`; returns its approval report (`report`)."""
    existing = read_json(RECEIPT)
    configured = (not force and existing and existing.get('state') == 'ready'
                  and matches(existing, pinned, mode) and Path(lcu_command('lcu')).exists())
    if configured and not boot:
        # The receipt shows `lcu setup` applied this mode completely. An agent installed
        # since then is still registered.
        agents, pending = list(existing.get('agents') or []), list(existing.get('pending') or [])
        registered, remaining = reconcile_agents(agents, pending, always=False)
        if (registered, remaining) != (agents, pending):
            write_json(RECEIPT, dict(existing, agents=registered, pending=remaining, updatedAt=now()))
        ensure_triggers(remaining)
        return report(mode, 'applied')
    write_receipt(pinned, mode, 'installing')
    # Set once approval is confirmed: later failures (the readiness check) do not change it.
    result = None
    try:
        STAGE.mkdir(mode=0o700, parents=True, exist_ok=True)
        installed = lcu_status() if Path(lcu_command('lcu')).exists() else None
        if not installed_for(installed, pinned):
            install(pinned, STAGE)
            configured = False
        if configured:
            outcome, agents, reason, stored = 'applied', existing.get('agents', []), None, True
            pending = list(existing.get('pending') or [])
        else:
            outcome, agents, reason, stored = setup(mode)
            pending = None
        if outcome == 'failed':
            raise Failure(reason)
        result = report(mode, outcome, reason)
        if not stored:
            # The approval outcome is kept; readiness fails and the next boot retries.
            raise Failure('cross-turn-failed', 'LCU did not turn on Computer Use across turns')
        if pending is None:
            pending = pending_agents(lcu_status()) or []
        # A setup that just ran has registered what is installed; a verified boot asks LCU.
        agents, pending = reconcile_agents(agents, pending, always=configured)
        ensure_triggers(pending)
        digested = digest(lcu_status())
        if not doctor(SESSION_WAIT_BOOT if boot else SESSION_WAIT, repair=boot):
            raise Failure('doctor-failed')
        write_receipt(pinned, mode, 'ready', readiness='ready', agents=agents, pending=pending,
                      approvalOutcome=outcome, verifiedAt=now(), **digested)
    except Failure as failure:
        log(f'computer use setup failed: {failure.reason}: {failure}')
        result = result or report(mode, 'failed', failure.reason)
        write_receipt(pinned, mode, 'failed', failure.reason, readiness='failed',
                      approvalOutcome=result['outcome'])
    except Exception as error:  # noqa: BLE001 - the receipt must always record the end state
        log(f'computer use setup failed: {type(error).__name__}: {error}')
        result = result or report(mode, 'failed', 'setup-failed')
        write_receipt(pinned, mode, 'failed', 'setup-failed', readiness='failed',
                      approvalOutcome=result['outcome'])
    finally:
        subprocess.run(['rm', '-rf', str(STAGE)], check=False)
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('status')
    commands.add_parser('reconcile')
    commands.add_parser('watch')
    apply_parser = commands.add_parser('apply')
    apply_parser.add_argument('--approval', choices=APPROVALS, required=True)
    apply_parser.add_argument('--force', action='store_true')
    apply_parser.add_argument('--boot', action='store_true')
    args = parser.parse_args(argv)
    if os.geteuid() != 0:
        print('Run as root', file=sys.stderr)
        return 1
    if args.command == 'watch':
        watch()
        return 0
    try:
        if args.command == 'status':
            result = status()
        elif args.command == 'reconcile':
            result = reconcile()
        else:
            result = apply(args.approval, args.force, args.boot)
    except Failure as failure:
        # The run could not even start (another run held the lock): still a report.
        result = {'schemaVersion': SCHEMA, 'state': 'failed', 'reason': failure.reason}
        if args.command == 'apply':
            result['apply'] = report(args.approval, 'failed', failure.reason)
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
