#!/usr/bin/python3
"""Pinned agent tools for a Silo Linux computer: Node.js, the Claude Code CLI and the Codex CLI.

Usage: agent-tools.py LOCK status|setup [claude-code|codex]
Silo runs it as root inside the guest. `status` prints one JSON line and changes nothing;
`setup` installs what is missing or outdated for one agent (Claude Code, with the Node.js
runtime it needs, unless Codex is named) and prints the same status. It never signs in to
an agent: authentication comes from the client that runs it.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import pwd
import re
import resource
import shutil
import subprocess
import sys
import tarfile
import tempfile

STATE = Path('/var/lib/silo')
RECEIPT = STATE / 'agent-tools.json'
NODE_ROOT = Path('/opt/silo/node')
CODEX_ROOT = Path('/opt/silo/codex')
BIN = Path('/usr/local/bin')
ACCOUNT = 'silo'
NODE_LINKS = ('node', 'npm', 'npx')
AGENTS = ('claude-code', 'codex')
CODEX_ASSETS = {
    'linux-arm64': 'aarch64-unknown-linux-musl',
    'linux-amd64': 'x86_64-unknown-linux-musl',
    'macos-arm64': 'aarch64-apple-darwin',
}
NODE_MAX_BYTES = 256 * 1024 * 1024
INSTALLER_MAX_BYTES = 8 * 1024 * 1024
ARCHITECTURES = {'aarch64': 'arm64', 'arm64': 'arm64', 'x86_64': 'amd64', 'amd64': 'amd64'}


def read_lock(path):
    lock = json.loads(Path(path).read_text())
    node, claude = lock.get('node', {}), lock.get('claudeCode', {})
    codex = lock.get('codex', {})
    installer = claude.get('installer', {})
    ok = (lock.get('schemaVersion') == 1 and isinstance(lock.get('recipeVersion'), int)
          and re.fullmatch(r'\d+\.\d+\.\d+', node.get('version', ''))
          and re.fullmatch(r'\d+\.\d+\.\d+', claude.get('version', ''))
          and set(node.get('assets', {})) == {'arm64', 'amd64'}
          and installer.get('url') == 'https://downloads.claude.ai/claude-code-releases/bootstrap.sh'
          and re.fullmatch('[0-9a-f]{64}', installer.get('sha256', ''))
          and re.fullmatch(r'\d+\.\d+\.\d+', codex.get('version', ''))
          and set(codex.get('assets', {})) == set(CODEX_ASSETS))
    for key, triple in CODEX_ASSETS.items():
        asset = codex.get('assets', {}).get(key, {})
        ok = ok and asset.get('url') == (
            f"https://github.com/openai/codex/releases/download/rust-v{codex.get('version')}/codex-{triple}.tar.gz"
        ) and asset.get('member') == f'codex-{triple}' and re.fullmatch('[0-9a-f]{64}', asset.get('sha256', '')) \
            and isinstance(asset.get('bytes'), int) and asset['bytes'] > 0
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
    """The first whole word of the output that is a x.y.z version, with a leading v allowed."""
    for word in (output or '').split():
        match = re.fullmatch(r'v?([0-9]+\.[0-9]+\.[0-9]+)', word)
        if match:
            return match.group(1)
    return None


def version_parts(version):
    parts = (version or '').split('.')
    return [int(part) for part in parts] if all(re.fullmatch('[0-9]+', part) for part in parts) else None


def not_older(installed, pinned):
    """Whether the installed version is the pinned one or a newer one. Dotted numbers compare
    numerically; anything else counts only when it is equal."""
    have, want = version_parts(installed), version_parts(pinned)
    if have is None or want is None:
        return installed == pinned
    size = max(len(have), len(want))
    return have + [0] * (size - len(have)) >= want + [0] * (size - len(want))


def account_home():
    return Path(pwd.getpwnam(ACCOUNT).pw_dir)


def node_version():
    try:
        result = run([str(BIN / 'node'), '--version'], timeout=10)
    except (OSError, subprocess.SubprocessError):
        return None
    return first_version(result.stdout) if result.returncode == 0 else None


def codex_version():
    try:
        result = run(['runuser', '-u', ACCOUNT, '--', 'env', f'HOME={account_home()}', str(BIN / 'codex'), '--version'],
                     timeout=20)
    except (OSError, KeyError, subprocess.SubprocessError):
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
    return {'nodeVersion': node_version(), 'claudeVersion': claude_version(), 'codexVersion': codex_version(),
            'recipeVersion': recipe_version()}


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()


def limit_file_size(limit):
    """For a child process: a file it writes cannot grow past the limit."""
    def apply():
        resource.setrlimit(resource.RLIMIT_FSIZE, (limit, limit))
    return apply


def download(url, sha256, destination, max_bytes, size=None):
    """Downloads over HTTPS (redirects too), refuses more than `max_bytes`, and keeps the file
    only when it has the pinned size, if there is one, and the pinned SHA-256."""
    partial = Path(str(destination) + '.partial')
    name = url.rsplit('/', 1)[-1]
    result = run(['curl', '--silent', '--show-error', '--fail', '--location', '--retry', '2',
                  '--connect-timeout', '30', '--max-time', '900', '--proto', '=https', '--proto-redir', '=https',
                  '--tlsv1.2', '--max-filesize', str(max_bytes), url, '-o', str(partial)],
                 timeout=1000, preexec_fn=limit_file_size(max_bytes + 1))
    if result.returncode != 0:
        partial.unlink(missing_ok=True)
        raise RuntimeError(f'Could not download {name}: {result.stderr.strip()}')
    if partial.stat().st_size > max_bytes or (size is not None and partial.stat().st_size != size) \
            or sha256_file(partial) != sha256:
        partial.unlink(missing_ok=True)
        raise RuntimeError(f'{name} does not match the version Silo expects')
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
    download(asset['url'], asset['sha256'], archive_path, NODE_MAX_BYTES)
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
    installed = claude_version()
    if installed is not None and not_older(installed, version):
        return
    installer = lock['claudeCode']['installer']
    script = Path(work) / 'claude-install.sh'
    download(installer['url'], installer['sha256'], script, INSTALLER_MAX_BYTES)
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


def install_codex(lock, work):
    """Installs the pinned Codex, or keeps a newer one. Returns the version that is installed."""
    version = lock['codex']['version']
    installed = codex_version()
    if installed is not None and installed != version and not_older(installed, version):
        return installed
    if installed == version and (CODEX_ROOT / version / 'codex').exists():
        return version
    arch = ARCHITECTURES.get(platform.machine())
    if arch is None:
        raise RuntimeError(f'Unsupported architecture {platform.machine()}')
    asset = lock['codex']['assets'][f'linux-{arch}']
    archive_path = Path(work) / asset['url'].rsplit('/', 1)[-1]
    download(asset['url'], asset['sha256'], archive_path, asset['bytes'], asset['bytes'])
    CODEX_ROOT.mkdir(parents=True, exist_ok=True)
    CODEX_ROOT.chmod(0o755)
    staging = Path(tempfile.mkdtemp(prefix='.codex-', dir=CODEX_ROOT))
    try:
        with tarfile.open(archive_path, 'r:gz') as archive:
            members = archive.getmembers()
            if len(members) != 1 or members[0].name != asset['member'] or not members[0].isreg():
                raise RuntimeError('The Codex archive does not contain the expected file')
            source = archive.extractfile(members[0])
            binary = staging / 'codex'
            with open(binary, 'wb') as target:
                shutil.copyfileobj(source, target, 1 << 20)
            binary.chmod(0o755)
        # The temporary folder is private; the working account must be able to run the program.
        staging.chmod(0o755)
        target = CODEX_ROOT / version
        if target.exists():
            shutil.rmtree(target)
        staging.rename(target)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    link = BIN / 'codex'
    link.parent.mkdir(parents=True, exist_ok=True)
    temporary = BIN / '.codex.silo'
    temporary.unlink(missing_ok=True)
    temporary.symlink_to(CODEX_ROOT / version / 'codex')
    temporary.replace(link)
    if codex_version() != version:
        raise RuntimeError('Codex did not report the pinned version after installation')
    for other in CODEX_ROOT.iterdir():
        if other.name != version:
            shutil.rmtree(other, ignore_errors=True)
    return version


def write_receipt(lock, agent, codex_installed=None):
    """Records what was installed. One file holds every agent; setting up one keeps the others."""
    STATE.mkdir(parents=True, exist_ok=True)
    try:
        receipt = json.loads(RECEIPT.read_text())
    except (OSError, ValueError):
        receipt = {}
    if not isinstance(receipt, dict):
        receipt = {}
    receipt['schemaVersion'] = 1
    if agent == 'codex':
        receipt['codexVersion'] = codex_installed or lock['codex']['version']
    else:
        receipt.update(recipeVersion=lock['recipeVersion'], nodeVersion=lock['node']['version'],
                       claudeVersion=lock['claudeCode']['version'])
    temporary = RECEIPT.with_suffix('.tmp')
    temporary.write_text(json.dumps(receipt, separators=(',', ':')) + '\n')
    temporary.replace(RECEIPT)


def setup(lock, agent='claude-code'):
    if os.geteuid() != 0:
        raise RuntimeError('Agent tools setup requires guest root')
    try:
        pwd.getpwnam(ACCOUNT)
    except KeyError:
        raise RuntimeError('The working account does not exist yet; start the computer once and try again')
    STATE.mkdir(parents=True, exist_ok=True)
    # Codex's archive is over 100 MB, so its work folder is on the disk, not in /tmp.
    with tempfile.TemporaryDirectory(prefix='silo-agent-tools-', dir=STATE if agent == 'codex' else None) as work:
        installed = None
        if agent == 'codex':
            installed = install_codex(lock, work)
        else:
            install_node(lock, work)
            install_claude(lock, work)
    write_receipt(lock, agent, installed)


def main(argv):
    agent = argv[3] if len(argv) == 4 else 'claude-code'
    if len(argv) not in (3, 4) or argv[2] not in ('status', 'setup') or agent not in AGENTS:
        print('Usage: agent-tools.py LOCK status|setup [claude-code|codex]', file=sys.stderr)
        return 2
    try:
        lock = read_lock(argv[1])
        if argv[2] == 'setup':
            setup(lock, agent)
    except (RuntimeError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        print(f'error: {error}', file=sys.stderr)
        return 1
    print(json.dumps(status(), separators=(',', ':')))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
