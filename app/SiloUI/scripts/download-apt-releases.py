"""Fetch the latest published stable release and its predecessor, checking all bytes.

The release's own SHA256SUMS is editable with the release, so each package must
also match GitHub's release attestation, which GitHub signs when an immutable
release is published and which later release edits cannot change.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)


def attested(tag, target, repository, required):
    """Fails unless the bytes match the digest GitHub attested at publication."""
    for name in ('Silo-linux-x64.deb', 'Silo-linux-arm64.deb'):
        args = ['gh', 'release', 'verify-asset', tag, str(target / name), '--repo', repository]
        result = subprocess.run(args, check=False, capture_output=True, text=True)
        if result.returncode == 0:
            continue
        if not required and 'no attestations' in result.stdout + result.stderr:
            return False
        raise subprocess.CalledProcessError(result.returncode, args, result.stdout, result.stderr)
    return True


def download(output, repository):
    latest = json.loads(gh('api', f'repos/{repository}/releases/latest'))
    pages = json.loads(gh('api', '--paginate', '--slurp', f'repos/{repository}/releases'))
    stable = [r for page in pages for r in page if not r['draft'] and not r['prerelease'] and re.fullmatch(r'v\d+\.\d+\.\d+', r['tag_name'])]
    stable.sort(key=lambda r: tuple(map(int, r['tag_name'][1:].split('.'))), reverse=True)
    if not stable or stable[0]['tag_name'] != latest['tag_name']:
        raise ValueError('Latest release must be the highest published stable version')
    output.mkdir(parents=True, exist_ok=False)
    for index, release in enumerate(stable[:2]):
        if release.get('immutable') is not True:
            raise ValueError(f"{release['tag_name']} is not an immutable release; enable release immutability before publishing")
        version = release['tag_name'][1:]
        target = output / version
        target.mkdir()
        subprocess.run(['gh', 'release', 'download', release['tag_name'], '--repo', repository, '--dir', str(target), '--pattern', 'Silo-linux-*.deb', '--pattern', 'SHA256SUMS'], check=True)
        hashes = {}
        for line in (target / 'SHA256SUMS').read_text().splitlines():
            digest, name = line.split('  ', 1)
            if name in hashes or not re.fullmatch(r'[0-9a-f]{64}', digest):
                raise ValueError('Invalid or duplicate release checksums')
            hashes[name] = digest
        for name in ('Silo-linux-x64.deb', 'Silo-linux-arm64.deb'):
            package = target / name
            if package.is_symlink() or hashlib.sha256(package.read_bytes()).hexdigest() != hashes.get(name):
                raise ValueError('Release package checksum mismatch')
        if not attested(release['tag_name'], target, repository, required=index == 0):
            # Releases published before the repository moved carry no attestation for it;
            # the predecessor is left out rather than published unverified.
            shutil.rmtree(target)
            print(f"Leaving out {release['tag_name']}: GitHub has no release attestation for it in {repository}")
    (output / 'latest-version').write_text(latest['tag_name'])


if __name__ == '__main__':
    # The workflow passes its own repository, so forks publish their own releases.
    download(Path(sys.argv[1]), os.environ['GH_REPO'])
