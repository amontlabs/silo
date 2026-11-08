#!/usr/bin/python3
"""Set up this Silo computer's silo working account; Silo runs it as root (see working-account.sh).

A new computer gets a fresh account. A computer from an older Silo kept agent files under /root,
and sometimes had a separate silo-desktop account: those homes are copied into
/home/silo and the originals stay in place. The account record is written last;
completed copies can be retried, and conflicting entries stop setup for resolution.
"""
import filecmp
import grp
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import stat
import subprocess
import sys
import tempfile

POLICY = {'schemaVersion': 1, 'user': 'silo', 'home': '/home/silo'}
RECORD = Path('/var/lib/silo/working-account.json')
HOME = Path('/home/silo')
# Checked before any change, so a missing tool never leaves a partial account.
TOOLS = ('findmnt', 'groupadd', 'groupmod', 'useradd', 'usermod', 'runuser', 'sudo', 'visudo')
SHELL_SETUP = ('.bashrc', '.profile', '.bash_profile', '.zshrc', '.npmrc')
PATH_SETUP = '\n# Silo migrated user tools\nif [ -d "$HOME/.local/bin" ]; then\n    export PATH="$HOME/.local/bin:$PATH"\nfi\n'


def run(*args):
    return subprocess.run(args, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout.strip()


def account(name):
    try:
        return pwd.getpwnam(name)
    except KeyError:
        return None


def group(name):
    try:
        return grp.getgrnam(name)
    except KeyError:
        return None


def carried(path, defaults=Path('/usr/share/base-files')):
    """False for a dotfile identical to the distribution's default for root, such as an
    unchanged /root/.bashrc: it carries nothing and would replace the account's own."""
    default = defaults / ('dot' + path.name)
    return not (path.name.startswith('.') and path.is_file() and not path.is_symlink()
                and default.is_file() and path.read_bytes() == default.read_bytes())


def path_mode(path):
    try:
        return path.lstat().st_mode
    except FileNotFoundError:
        return None


def conflict(path):
    raise RuntimeError(f'Home migration conflict at {path}. Preserve or move the conflicting entry and retry; legacy homes are unchanged.')


def check_destination(destination):
    # Check ancestors from the filesystem root before inspecting any child.
    for path in reversed((destination.absolute(), *destination.absolute().parents)):
        mode = path_mode(path)
        if mode is not None and not stat.S_ISDIR(mode):
            conflict(path)


def relocate(value):
    for legacy in ('/home/silo-desktop', '/root'):
        if value == legacy or value.startswith(legacy + '/'):
            return '/home/silo' + value[len(legacy):]
    return value


def relocate_contents(data):
    return re.sub(rb"(?<![\w./-])(?:/home/silo-desktop|/root)(?=/|[\s'\";:]|$)", b'/home/silo', data)


def launcher_contents(path, relative):
    if not (relative.parent == Path('.') and path.name in SHELL_SETUP or 'bin' in relative.parts):
        return None
    try:
        data = path.read_bytes()
    except OSError:
        return None
    if b'\x00' in data:
        return None
    if not data.startswith(b'#!'):
        try:
            data.decode('utf-8')
        except UnicodeError:
            return None
    updated = relocate_contents(data)
    return updated if updated != data else None


def copy_file(path, target, contents=None, replace=False):
    with tempfile.NamedTemporaryFile(dir=target.parent, prefix='.silo-copy-', delete=False) as staged:
        temporary = Path(staged.name)
    try:
        shutil.copy2(path, temporary)
        if contents is not None:
            if isinstance(contents, bytes):
                temporary.write_bytes(contents)
            else:
                temporary.write_text(contents)
        if replace:
            os.replace(temporary, target)
        else:
            # Publish complete bytes without replacing an entry created during copying.
            os.link(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)


def copy_home(source, destination):
    """Preflight the whole merge; preserve conflicting entries and both legacy homes."""
    check_destination(destination)
    copies = []

    def preflight(path, target):
        mode = path.lstat().st_mode
        relative = path.relative_to(source)
        if stat.S_ISSOCK(mode) or stat.S_ISFIFO(mode) or path.parent == source and not carried(path):
            return
        existing = path_mode(target)
        if stat.S_ISDIR(mode):
            if existing is not None and not stat.S_ISDIR(existing):
                conflict(target)
            if existing is None:
                copies.append((path, target, mode))
            for child in sorted(path.iterdir()):
                preflight(child, target / child.name)
        elif stat.S_ISLNK(mode):
            if existing is not None:
                if stat.S_ISLNK(existing) and os.readlink(target) == relocate(os.readlink(path)):
                    return
                conflict(target)
            copies.append((path, target, mode))
        elif stat.S_ISREG(mode):
            if existing is not None:
                if not stat.S_ISREG(existing):
                    conflict(target)
                # Shell setup is resolved separately from root's authoritative copy.
                if path.parent == source and path.name in SHELL_SETUP:
                    return
                updated = launcher_contents(path, relative)
                same = target.read_bytes() == updated if updated is not None else filecmp.cmp(path, target, shallow=False)
                if same:
                    return
                conflict(target)
            copies.append((path, target, mode))
        else:
            conflict(path)

    preflight(source, destination)
    for path, target, mode in copies:
        if stat.S_ISDIR(mode):
            target.mkdir()
        elif stat.S_ISLNK(mode):
            target.symlink_to(relocate(os.readlink(path)))
            shutil.copystat(path, target, follow_symlinks=False)
        else:
            copy_file(path, target, launcher_contents(path, path.relative_to(source)))
    for path, target, mode in reversed(copies):
        if stat.S_ISDIR(mode):
            shutil.copystat(path, target)


def copy_shell_setup(source, destination):
    # The root account owns agent shell setup; desktop defaults must not replace it.
    check_destination(destination)
    originals = []
    for name in SHELL_SETUP:
        original, target = source / name, destination / name
        if original.is_file() and not original.is_symlink() and carried(original):
            existing = path_mode(target)
            if existing is not None and not stat.S_ISREG(existing):
                conflict(target)
            originals.append((original, target))
    for original, target in originals:
        contents = relocate_contents(original.read_bytes())
        # Root's profile does not put the user's own tools on PATH.
        if original.name == '.profile' and PATH_SETUP.encode('utf-8') not in contents:
            contents += PATH_SETUP.encode('utf-8')
        copy_file(original, target, contents, replace=True)


def validate_account(entry):
    if entry.pw_uid != 1001 or entry.pw_gid != 1001 or entry.pw_dir != '/home/silo':
        raise RuntimeError('The silo account does not match the layout Silo needs.')


def write_text_atomic(path, contents, mode):
    fd, temporary = tempfile.mkstemp(prefix=f'.{path.name}-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(contents)
            output.flush()
            os.fchmod(output.fileno(), mode)
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


def set_up(desktop_service):
    if os.geteuid() != 0 or run('sh', '-c', '. /etc/os-release; printf "%s %s" "$ID" "$VERSION_ID"') != 'ubuntu 24.04':
        raise RuntimeError('The silo account needs root inside an Ubuntu 24.04 Silo computer.')
    missing = [tool for tool in TOOLS if not shutil.which(tool)]
    if missing:
        raise RuntimeError(f'The sandbox is missing {", ".join(missing)}.')
    # A container computer has no workspace disk of its own: /workspace is a folder of its root.
    container = os.environ.get('SILO_RUNTIME') == 'container'
    if container:
        if not Path('/workspace').is_dir():
            raise RuntimeError('The silo account needs the workspace folder.')
    elif run('findmnt', '-rn', '-T', '/workspace', '-o', 'TARGET,FSTYPE') != '/workspace ext4':
        raise RuntimeError('The silo account needs the standard workspace disk.')
    if RECORD.exists():
        if json.loads(RECORD.read_text()) != POLICY:
            raise RuntimeError('This sandbox records an account layout this Silo does not know. Update Silo.')
        return
    silo, desktop = account('silo'), account('silo-desktop')
    if silo and desktop:
        raise RuntimeError('Both the silo and silo-desktop accounts exist.')
    for lookup in (pwd.getpwuid, grp.getgrgid):
        try:
            entry = lookup(1001)
        except KeyError:
            continue
        if entry[0] not in ('silo', 'silo-desktop'):
            raise RuntimeError('The reserved account ID 1001 belongs to another account.')
    if HOME.is_symlink() or HOME.exists() and not HOME.is_dir():
        raise RuntimeError('The silo home folder is not a folder.')
    if HOME.exists() and not silo and not desktop:
        raise RuntimeError('The silo home folder exists without its account.')
    # Each step checks what an interrupted earlier run already did.
    if desktop:
        if desktop.pw_uid != 1001 or desktop.pw_gid != 1001 or desktop.pw_dir != '/home/silo-desktop':
            raise RuntimeError('The old silo-desktop account has an unexpected layout.')
        # Stop the managed desktop, which may have started at boot, before renaming its account.
        if Path('/usr/local/bin/silo-desktop').exists():
            run('/usr/local/bin/silo-desktop', 'stop')
        run('usermod', '-l', 'silo', '-d', '/home/silo', '-s', '/bin/bash', 'silo-desktop')
    if not group('silo'):
        if group('silo-desktop'):
            run('groupmod', '-n', 'silo', 'silo-desktop')
        else:
            run('groupadd', '--gid', '1001', 'silo')
    if not account('silo'):
        run('useradd', '--uid', '1001', '--gid', 'silo', '--create-home', '--shell', '/bin/bash', '--comment', 'Silo working account', 'silo')
    validate_account(account('silo'))
    Path('/etc/sudoers.d/silo-desktop').unlink(missing_ok=True)
    HOME.mkdir(exist_ok=True)
    # Root holds agent state; merge desktop preferences and files around it.
    copy_home(Path('/root'), HOME)
    if Path('/home/silo-desktop').is_dir():
        copy_home(Path('/home/silo-desktop'), HOME)
    copy_shell_setup(Path('/root'), HOME)
    run('chown', '-hR', 'silo:silo', str(HOME))
    # Do not traverse other mounts or follow symlinks into system or host files.
    run('find', '/workspace', '-xdev', '-exec', 'chown', '-h', 'silo:silo', '{}', '+')
    # sudo cannot gain privileges in a container (no-new-privileges), so none is granted there.
    if not container:
        sudoers = Path('/etc/sudoers.d/silo')
        write_text_atomic(sudoers, 'silo ALL=(ALL:ALL) NOPASSWD: ALL\n', 0o440)
        run('visudo', '-cf', str(sudoers))
    state = Path('/var/lib/silo-desktop')
    if (state / 'installed.json').exists():
        service = Path('/usr/local/bin/silo-desktop')
        write_text_atomic(service, desktop_service, 0o755)
        write_text_atomic(state / 'configuration-managed.json', '{"home":"/home/silo"}\n', 0o600)
    run('runuser', '-u', 'silo', '--', 'env', 'HOME=/home/silo', 'USER=silo', 'LOGNAME=silo', 'sh', '-ec', 'test -w "$HOME"; test -w /workspace; ' + ('' if container else 'sudo -n true; ') + 'test -x /usr/lib/openssh/sftp-server')
    RECORD.parent.mkdir(parents=True, exist_ok=True)
    write_text_atomic(RECORD, json.dumps(POLICY, separators=(',', ':')) + '\n', 0o644)


if __name__ == '__main__':
    set_up(sys.argv[1])
