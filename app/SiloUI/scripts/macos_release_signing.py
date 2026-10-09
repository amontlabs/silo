"""The optimized macOS exact-engine signing policy. Never edits installed apps."""
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile

TOOLS = ('silo-ui', 'msb', 'git', 'git-lfs', 'git-remote-http', 'git-remote-https')
# macOS computers run Virtualization.framework inside the app process itself.
APP_REQUIRED_ENTITLEMENTS = ('com.apple.security.virtualization',)
RUNTIME_ENTITLEMENTS = {
    'com.apple.security.hypervisor': True,
    'com.apple.security.cs.disable-library-validation': True,
}


def command(*args):
    return subprocess.run(list(map(str, args)), capture_output=True, check=True)


def code_hash(path):
    display = command('codesign', '--display', '--verbose=4', path).stderr.decode()
    match = re.search(r'^CDHash=([0-9a-f]{40})$', display, re.M)
    if not match:
        raise RuntimeError(f'No supported code-directory hash for {path.name}.')
    return bytes.fromhex(match[1])


def policy_files(app, directory):
    library = app / 'Contents/Frameworks/libkrunfw.5.dylib'
    command('codesign', '--verify', '--strict', library)
    for binary in (library, app / 'Contents/MacOS/msb'):
        if command('lipo', '-archs', binary).stdout.strip() != b'arm64':
            raise RuntimeError('This signing policy requires thin Apple Silicon binaries.')
    entitlements = directory / 'msb.plist'
    constraint = directory / 'libraries.coderequirement'
    entitlements.write_bytes(plistlib.dumps(RUNTIME_ENTITLEMENTS))
    constraint.write_bytes(plistlib.dumps({'cdhash': code_hash(library)}))
    return entitlements, constraint


def sign_helper(helper, entitlements, constraint):
    command('codesign', '--force', '--sign', '-', '--options', 'runtime',
            '--preserve-metadata=identifier,runtime', '--entitlements', entitlements,
            '--enforce-constraint-validity', '--library-constraint', constraint, helper)


def sign_runtime(app):
    with tempfile.TemporaryDirectory(prefix='silo-sign-') as temporary:
        entitlements, constraint = policy_files(app, Path(temporary))
        sign_helper(app / 'Contents/MacOS/msb', entitlements, constraint)
    # Re-seal the outer bundle without re-signing its nested executables.
    command('codesign', '--force', '--sign', '-', '--options', 'runtime',
            '--entitlements', Path(__file__).resolve().parent.parent / 'src-tauri/Entitlements.plist', app)


def verify_bundle(app):
    command('codesign', '--verify', '--deep', '--strict', app)
    for name in TOOLS:
        executable = app / 'Contents/MacOS' / name
        display = command('codesign', '--display', '--verbose=4', executable).stderr
        if not re.search(rb'flags=0x[0-9a-f]+\([^\n)]*\bruntime\b', display):
            raise RuntimeError(f'{name} must retain hardened runtime.')
        raw = command('codesign', '--display', '--entitlements', ':-', executable).stdout
        entitlements = plistlib.loads(raw) if raw.strip() else {}
        if name == 'msb':
            if entitlements != RUNTIME_ENTITLEMENTS:
                raise RuntimeError('The VM helper has unexpected runtime entitlements.')
        elif entitlements.get('com.apple.security.cs.disable-library-validation'):
            raise RuntimeError('Only the constrained VM helper may have the library-loading exception.')
        if name == 'silo-ui':
            for key in APP_REQUIRED_ENTITLEMENTS:
                if entitlements.get(key) is not True:
                    raise RuntimeError(f'Silo must be signed with {key}.')
    with tempfile.TemporaryDirectory(prefix='silo-verify-') as temporary:
        directory = Path(temporary)
        entitlements, constraint = policy_files(app, directory)
        helper = app / 'Contents/MacOS/msb'
        expected = directory / 'msb'
        shutil.copy2(helper, expected)
        # Apple's signer reconstructs the exact expected signature on a copy.
        # Equal CodeDirectory hashes bind the full constraint and entitlements;
        # finding an allowed hash inside a possibly broader rule is insufficient.
        sign_helper(expected, entitlements, constraint)
        if code_hash(expected) != code_hash(helper):
            raise RuntimeError('The embedded VM library constraint is missing or differs from the exact bundled engine policy.')
