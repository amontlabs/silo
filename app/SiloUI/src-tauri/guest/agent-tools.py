#!/usr/bin/python3
"""Pinned agent tools for a Silo Linux computer: Node.js and the Claude Code CLI.

Usage: agent-tools.py LOCK status|setup
Silo runs it as root inside the guest. `status` prints one JSON line and changes nothing;
`setup` installs what is missing or outdated and prints the same status. It never signs in
to Claude Code: authentication comes from the client that runs it.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import pwd
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

STATE = Path('/var/lib/silo')
RECEIPT = STATE / 'agent-tools.json'
NODE_ROOT = Path('/opt/silo/node')
BIN = Path('/usr/local/bin')
ACCOUNT = 'silo'
NODE_LINKS = ('node', 'npm', 'npx')
ARCHITECTURES = {'aarch64': 'arm64', 'arm64': 'arm64', 'x86_64': 'amd64', 'amd64': 'amd64'}


def read_lock(path):
    lock = json.loads(Path(path).read_text())
    node, claude = lock.get('node', {}), lock.get('claudeCode', {})
    installer = claude.get('installer', {})
    ok = (lock.get('schemaVersion') == 1 and isinstance(lock.get('recipeVersion'), int)
          and re.fullmatch(r'\d+\.\d+\.\d+', node.get('version', ''))
          and re.fullmatch(r'\d+\.\d+\.\d+', claude.get('version', ''))
          and set(node.get('assets', {})) == {'arm64', 'amd64'}
          and installer.get('url') == 'https://downloads.claude.ai/claude-code-releases/bootstrap.sh'
          and re.fullmatch('[0-9a-f]{64}', installer.get('sha256', '')))
    for arch, asset in node.get('assets', {}).items():
        name = 'x64' if arch == 'amd64' else arch
        ok = ok and asset.get('url') == (
            f"https://nodejs.org/dist/v{node.get('version')}/node-v{node.get('version')}-linux-{name}.tar.xz"
        ) and re.fullmatch('[0-9a-f]{64}', asset.get('sha256', ''))
    if not ok:
        raise RuntimeError('Invalid bundled agent tools lock')
    return lock


def run(command, timeout=60, **options):
    return subprocess.run(command, capture_output=True, text=True, timeout=timeout, **options)


def first_version(output):
    match = re.search(r'\d+\.\d+\.\d+', output or '')
    return match.group(0) if match else None


def account_home():
    return Path(pwd.getpwnam(ACCOUNT).pw_dir)


def node_version():
    try:
        result = run([str(BIN / 'node'), '--version'], timeout=10)
    except (OSError, subprocess.SubprocessError):
        return None
    return first_version(result.stdout) if result.returncode == 0 else None


def claude_command():
    return ['runuser', '-u', ACCOUNT, '--', 'env', f'HOME={account_home()}', str(BIN / 'claude'), '--version']


def claude_version():
    try:
        result = run(claude_command(), timeout=20)
    except (OSError, KeyError, subprocess.SubprocessError):
        return None
    return first_version(result.stdout) if result.returncode == 0 else None


def recipe_version():
    try:
        value = json.loads(RECEIPT.read_text()).get('recipeVersion')
    except (OSError, ValueError, AttributeError):
        return None
    return value if isinstance(value, int) else None


def status():
    return {'nodeVersion': node_version(), 'claudeVersion': claude_version(), 'recipeVersion': recipe_version()}


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()


def download(url, sha256, destination):
    """Downloads over HTTPS and keeps the file only when it matches the pinned SHA-256."""
    partial = Path(str(destination) + '.partial')
    result = run(['curl', '--silent', '--show-error', '--fail', '--location', '--retry', '2',
                  '--connect-timeout', '30', '--max-time', '900', '--proto', '=https', '--tlsv1.2',
                  url, '-o', str(partial)], timeout=1000)
    if result.returncode != 0:
        partial.unlink(missing_ok=True)
        raise RuntimeError(f'Could not download {url.rsplit("/", 1)[-1]}: {result.stderr.strip()}')
    if sha256_file(partial) != sha256:
        partial.unlink(missing_ok=True)
        raise RuntimeError(f'{url.rsplit("/", 1)[-1]} does not match the version Silo expects')
    partial.replace(destination)


def safe_members(archive):
    members = archive.getmembers()
    for member in members:
        name = member.name
        if name.startswith('/') or '..' in Path(name).parts or member.isdev():
            raise RuntimeError('The Node.js archive has an unsafe path')
        if member.issym() or member.islnk():
            target = Path(name).parent / member.linkname
            if member.linkname.startswith('/') or '..' in Path(os.path.normpath(target)).parts:
                raise RuntimeError('The Node.js archive has an unsafe link')
    return members


def install_node(lock, work):
    version = lock['node']['version']
    if node_version() == version and (NODE_ROOT / version / 'bin/node').exists():
        return
    arch = ARCHITECTURES.get(platform.machine())
    if arch is None:
        raise RuntimeError(f'Unsupported architecture {platform.machine()}')
    asset = lock['node']['assets'][arch]
    archive_path = Path(work) / asset['url'].rsplit('/', 1)[-1]
    download(asset['url'], asset['sha256'], archive_path)
    NODE_ROOT.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix='.node-', dir=NODE_ROOT))
    try:
        with tarfile.open(archive_path, 'r:xz') as archive:
            archive.extractall(staging, members=safe_members(archive), filter='data')
        (root,) = list(staging.iterdir())
        target = NODE_ROOT / version
        if target.exists():
            shutil.rmtree(target)
        root.rename(target)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    for name in NODE_LINKS:
        link = BIN / name
        link.parent.mkdir(parents=True, exist_ok=True)
        temporary = BIN / f'.{name}.silo'
        temporary.unlink(missing_ok=True)
        temporary.symlink_to(NODE_ROOT / version / 'bin' / name)
        temporary.replace(link)
    if node_version() != version:
        raise RuntimeError('Node.js did not report the pinned version after installation')


def install_claude(lock, work):
    version = lock['claudeCode']['version']
    if claude_version() == version:
        return
    installer = lock['claudeCode']['installer']
    script = Path(work) / 'claude-install.sh'
    download(installer['url'], installer['sha256'], script)
    script.chmod(0o755)
    # The working account runs the script, and the work directory is a private temporary one.
    Path(work).chmod(0o755)
    home = account_home()
    # The documented installer, as the working account. It checks the binary against
    # Anthropic's release manifest. Auto-update stays off so the pinned version holds.
    result = run(['runuser', '-u', ACCOUNT, '--', 'env', f'HOME={home}', 'DISABLE_AUTOUPDATER=1',
                  'PATH=/usr/local/bin:/usr/bin:/bin', 'bash', str(script), version],
                 timeout=900, cwd=str(home))
    if result.returncode != 0:
        tail = (result.stderr or result.stdout).strip().splitlines()[-1:] or ['the installer failed']
        raise RuntimeError(f'Claude Code could not be installed: {tail[0]}')
    binary = home / '.local/bin/claude'
    if not binary.exists():
        raise RuntimeError('The Claude Code installer did not create ~/.local/bin/claude')
    link = BIN / 'claude'
    temporary = BIN / '.claude.silo'
    temporary.unlink(missing_ok=True)
    temporary.symlink_to(binary)
    temporary.replace(link)
    if claude_version() != version:
        raise RuntimeError('Claude Code did not report the pinned version after installation')


def write_receipt(lock):
    STATE.mkdir(parents=True, exist_ok=True)
    receipt = {'schemaVersion': 1, 'recipeVersion': lock['recipeVersion'],
               'nodeVersion': lock['node']['version'], 'claudeVersion': lock['claudeCode']['version']}
    temporary = RECEIPT.with_suffix('.tmp')
    temporary.write_text(json.dumps(receipt, separators=(',', ':')) + '\n')
    temporary.replace(RECEIPT)


def setup(lock):
    if os.geteuid() != 0:
        raise RuntimeError('Agent tools setup requires guest root')
    try:
        pwd.getpwnam(ACCOUNT)
    except KeyError:
        raise RuntimeError('The working account does not exist yet; start the computer once and try again')
    with tempfile.TemporaryDirectory(prefix='silo-agent-tools-') as work:
        install_node(lock, work)
        install_claude(lock, work)
    write_receipt(lock)


def main(argv):
    if len(argv) != 3 or argv[2] not in ('status', 'setup'):
        print('Usage: agent-tools.py LOCK status|setup', file=sys.stderr)
        return 2
    try:
        lock = read_lock(argv[1])
        if argv[2] == 'setup':
            setup(lock)
    except (RuntimeError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        print(f'error: {error}', file=sys.stderr)
        return 1
    print(json.dumps(status(), separators=(',', ':')))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
