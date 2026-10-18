"""The optimized macOS exact-engine signing policy. Never edits installed apps."""
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile

from embed_engine import REQUIREMENT_KEY

TOOLS = (
    'silo-ui', 'msb', 'silo-macos-gateway', 'silo-network-bootstrap',
    'git', 'git-lfs', 'git-remote-http', 'git-remote-https',
)
# macOS computers run Virtualization.framework inside the app process itself.
APP_REQUIRED_ENTITLEMENTS = ('com.apple.security.virtualization',)
RUNTIME_ENTITLEMENTS = {
    'com.apple.security.hypervisor': True,
    'com.apple.security.cs.disable-library-validation': True,
}
GATEWAY_ENTITLEMENTS = {
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
    for binary in (library, app / 'Contents/MacOS/msb', app / 'Contents/MacOS/silo-macos-gateway'):
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
        gateway_entitlements = Path(temporary) / 'gateway.plist'
        gateway_entitlements.write_bytes(plistlib.dumps(GATEWAY_ENTITLEMENTS))
        sign_helper(app / 'Contents/MacOS/msb', entitlements, constraint)
        sign_helper(
            app / 'Contents/MacOS/silo-macos-gateway',
            gateway_entitlements,
            constraint,
        )
        command('codesign', '--force', '--sign', '-', '--options', 'runtime',
                '--preserve-metadata=identifier,runtime',
                app / 'Contents/MacOS/silo-network-bootstrap')
    # Re-seal the outer bundle without re-signing its nested executables.
    command('codesign', '--force', '--sign', '-', '--options', 'runtime',
            '--entitlements', Path(__file__).resolve().parent.parent / 'src-tauri/Entitlements.plist', app)


ENGINE_DISTRIBUTION = 'Contents/Resources/SiloEngineDistribution/Silo Engine.app'


def verify_engine_requirement(engine):
    """The engine names its signer in its sealed Info.plist; a production silod refuses to start without it."""
    info = plistlib.loads((engine / 'Contents/Info.plist').read_bytes())
    requirement = info.get(REQUIREMENT_KEY)
    if not isinstance(requirement, str) or not requirement.strip():
        raise RuntimeError(f'The engine Info.plist must set {REQUIREMENT_KEY}; silod refuses to start without it.')


def verify_engine(app, identifier):
    """The embedded engine is sealed with the virtualization entitlement, and the app's LaunchAgent
    plist points at it."""
    engine = app / ENGINE_DISTRIBUTION
    executable = engine / 'Contents/MacOS/silod'
    command('codesign', '--verify', '--strict', engine)
    verify_engine_requirement(engine)
    display = command('codesign', '--display', '--verbose=4', executable).stderr
    if not re.search(rb'flags=0x[0-9a-f]+\([^\n)]*\bruntime\b', display):
        raise RuntimeError('silod must retain hardened runtime.')
    raw = command('codesign', '--display', '--entitlements', ':-', executable).stdout
    entitlements = plistlib.loads(raw) if raw.strip() else {}
    if entitlements.get('com.apple.security.virtualization') is not True:
        raise RuntimeError('silod must be signed with com.apple.security.virtualization.')
    if entitlements.get('com.apple.security.cs.disable-library-validation'):
        raise RuntimeError('Only the constrained VM helper may have the library-loading exception.')
    agent = app / 'Contents/Library/LaunchAgents' / f'{identifier}.engine.plist'
    plist = plistlib.loads(agent.read_bytes())
    if plist.get('BundleProgram') != f'{ENGINE_DISTRIBUTION}/Contents/MacOS/silod':
        raise RuntimeError('The LaunchAgent plist must point at the embedded engine.')


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
        elif name == 'silo-macos-gateway':
            if entitlements != GATEWAY_ENTITLEMENTS:
                raise RuntimeError('The network gateway has unexpected runtime entitlements.')
        elif name == 'silo-network-bootstrap':
            if entitlements:
                raise RuntimeError('The guest network bootstrap must not carry host entitlements.')
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
        gateway_entitlements = directory / 'gateway.plist'
        gateway_entitlements.write_bytes(plistlib.dumps(GATEWAY_ENTITLEMENTS))
        # Apple's signer reconstructs each constrained helper's signature on a copy.
        # Equal CodeDirectory hashes bind its complete entitlement and library policy.
        for name, allowed in (
            ('msb', entitlements),
            ('silo-macos-gateway', gateway_entitlements),
        ):
            helper = app / 'Contents/MacOS' / name
            expected = directory / name
            shutil.copy2(helper, expected)
            sign_helper(expected, allowed, constraint)
            if code_hash(expected) != code_hash(helper):
                raise RuntimeError(f'{name} is missing the exact bundled-engine library constraint.')
