#!/usr/bin/env python3
"""Transfer only the existing public runtime cache between jobs in one workflow.

Consumers must still run normal runtime preparation, which validates pinned
inputs, the compiled runtime's capabilities, and regenerates release metadata.
"""
import argparse
import fnmatch
import hashlib
from pathlib import Path, PurePosixPath
import re
import tarfile

# Match prepare-release-runtime's cache paths. Regression tests prevent drift.
FILES = (
    'app/SiloUI/src-tauri/target/runtime-cache/v*/msb-*',
    'app/SiloUI/src-tauri/target/runtime-cache/v*/microsandbox-*.tar.gz',
    'app/SiloUI/src-tauri/target/runtime-cache/v*/agentd-*',
    'app/SiloUI/src-tauri/target/runtime-cache/v*/libkrunfw-*',
    'app/SiloUI/src-tauri/target/runtime-cache/v*/patched-builds/*/msb',
    'app/SiloUI/src-tauri/target/runtime-cache/v*/patched-builds/*/msb.sha256',
    'app/SiloUI/src-tauri/target/runtime-cache/git-lfs-transfer/*/source.tar.gz',
)
DIRECTORIES = (
    'app/SiloUI/src-tauri/target/runtime-cache/v*/licenses',
    'app/SiloUI/src-tauri/target/runtime-cache/dugite',
    'app/SiloUI/src-tauri/target/runtime-cache/git-lfs-transfer/*/builds',
)


def approved(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or str(path) != name:
        return False
    for pattern in FILES + DIRECTORIES:
        parts = PurePosixPath(pattern).parts
        if len(path.parts) < len(parts):
            continue
        if all(fnmatch.fnmatchcase(value, wanted) for value, wanted in zip(path.parts, parts)):
            if pattern in DIRECTORIES or len(path.parts) == len(parts):
                return True
    return False


def contained(root, path):
    relative = path.relative_to(root)
    for length in range(1, len(relative.parts) + 1):
        if root.joinpath(*relative.parts[:length]).is_symlink():
            raise ValueError('Runtime cache transfer refuses symlinks')


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def pack(root, archive):
    root = root.resolve()
    selected = set()
    for pattern in FILES + DIRECTORIES:
        for match in root.glob(pattern):
            contained(root, match)
            candidates = match.rglob('*') if match.is_dir() else [match]
            for path in candidates:
                contained(root, path)
                if path.is_file():
                    name = path.relative_to(root).as_posix()
                    if not approved(name):
                        raise ValueError('Runtime cache contains an unapproved path')
                    selected.add(path)
    if not selected:
        raise ValueError('No public runtime cache files were prepared')
    archive.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, 'w:gz', dereference=True) as bundle:
        for path in sorted(selected):
            bundle.add(path, arcname=path.relative_to(root).as_posix(), recursive=False)
    return digest(archive)


def unpack(root, archive, expected_sha256):
    if not re.fullmatch(r'[a-f0-9]{64}', expected_sha256) or digest(archive) != expected_sha256:
        raise ValueError('Runtime cache archive checksum mismatch')
    root = root.resolve()
    with tarfile.open(archive, 'r:gz') as bundle:
        members = bundle.getmembers()
        names = set()
        # Validate every destination before writing the first file.
        for member in members:
            if not member.isfile() or not approved(member.name) or member.name in names:
                raise ValueError('Runtime cache archive contains an unapproved entry')
            names.add(member.name)
            contained(root, root / member.name)
        if not members:
            raise ValueError('Runtime cache archive is empty')
        for member in members:
            destination = root / member.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            with bundle.extractfile(member) as source, destination.open('wb') as target:
                while chunk := source.read(1024 * 1024):
                    target.write(chunk)
            destination.chmod(member.mode & 0o777)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['pack', 'unpack'])
    parser.add_argument('--root', type=Path, default=Path.cwd())
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--sha256', default='')
    args = parser.parse_args()
    if args.operation == 'pack':
        print(f'sha256={pack(args.root, args.archive)}')
    else:
        unpack(args.root, args.archive, args.sha256)
        print('Imported public runtime cache; normal preparation must verify and re-stage it.')


if __name__ == '__main__':
    main()
