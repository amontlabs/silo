"""The optimized macOS exact-engine signing policy. Never edits installed apps."""
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile

from embed_engine import ENGINE_FRAMEWORKS, ENGINE_RESOURCES, ENGINE_TOOLS, REQUIREMENT_KEY

TOOLS = (
    'silo-ui', 'msb', 'silo-macos-gateway', 'silo-network-bootstrap',
    'git', 'git-lfs', 'git-remote-http', 'git-remote-https',
    'silo',
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


def timestamp(identity):
    """A Developer ID signature needs Apple's trusted timestamp; an ad hoc one cannot have it."""
    return [] if identity == '-' else ['--timestamp']


def sign_helper(helper, entitlements, constraint, identity='-'):
    command('codesign', '--force', '--sign', identity, *timestamp(identity), '--options', 'runtime',
            '--preserve-metadata=identifier,runtime', '--entitlements', entitlements,
            '--enforce-constraint-validity', '--library-constraint', constraint, helper)


MACH_O_MAGIC = {bytes.fromhex(h) for h in ('feedfacf', 'cffaedfe', 'cafebabe', 'bebafeca')}
# Signed on their own below (helpers with constraints) or by `seal_app` (the main executable).
SIGNED_SEPARATELY = ('msb', 'silo-macos-gateway', 'silo-network-bootstrap', 'silo', 'silo-ui')


def is_mach_o(path):
    if path.is_symlink() or not path.is_file():
        return False
    with path.open('rb') as handle:
        return handle.read(4) in MACH_O_MAGIC


def sign_other_code(app, identity):
    """Signs every other Mach-O file (the Git tools, libraries) with the release identity. Tauri
    signed them ad hoc, which Apple's notary service rejects. Entitlements and flags carry over."""
    for path in sorted((Path(app) / 'Contents').rglob('*')):
        if path.parent.name == 'MacOS' and path.name in SIGNED_SEPARATELY or not is_mach_o(path):
            continue
        command('codesign', '--force', '--sign', identity, *timestamp(identity), '--options', 'runtime',
                '--preserve-metadata=identifier,entitlements', path)


def sign_helpers(app, identity='-'):
    """Gives the app's VM helper, gateway and bootstrap their final signatures. Run it before
    `embed_engine`, which copies them into the engine and seals it. A release identity first
    signs every other binary, so the library's final hash is the one the helper's constraint names."""
    if identity != '-':
        sign_other_code(app, identity)
    with tempfile.TemporaryDirectory(prefix='silo-sign-') as temporary:
        entitlements, constraint = policy_files(app, Path(temporary))
        gateway_entitlements = Path(temporary) / 'gateway.plist'
        gateway_entitlements.write_bytes(plistlib.dumps(GATEWAY_ENTITLEMENTS))
        sign_helper(app / 'Contents/MacOS/msb', entitlements, constraint, identity)
        sign_helper(
            app / 'Contents/MacOS/silo-macos-gateway',
            gateway_entitlements,
            constraint,
            identity,
        )
        command('codesign', '--force', '--sign', identity, *timestamp(identity), '--options', 'runtime',
                '--preserve-metadata=identifier,runtime',
                app / 'Contents/MacOS/silo-network-bootstrap')
        # The command line is a client of the engine: hardened runtime, no entitlements.
        command('codesign', '--force', '--sign', identity, *timestamp(identity), '--options', 'runtime',
                '--identifier', 'org.silo.cli', app / 'Contents/MacOS/silo')


def seal_app(app, *, hardened=True, identity='-'):
    """Re-seals the outer bundle without re-signing its nested code (the engine and the tools).
    `hardened=False` keeps a development bundle as Tauri signed it, without hardened runtime."""
    options = ['--options', 'runtime'] if hardened else []
    command('codesign', '--force', '--sign', identity, *timestamp(identity), *options,
            '--entitlements', Path(__file__).resolve().parent.parent / 'src-tauri/Entitlements.plist', app)


ENGINE_DISTRIBUTION = 'Contents/Resources/SiloEngineDistribution/Silo Engine.app'


def verify_engine_requirement(engine):
    """The engine names its signer in its sealed Info.plist; a production silod refuses to start without it."""
    info = plistlib.loads((engine / 'Contents/Info.plist').read_bytes())
    requirement = info.get(REQUIREMENT_KEY)
    if not isinstance(requirement, str) or not requirement.strip():
        raise RuntimeError(f'The engine Info.plist must set {REQUIREMENT_KEY}; silod refuses to start without it.')


def verify_engine_contents(app):
    """The engine carries every tool, library and resource silod resolves in its own bundle, and
    each signed copy is the app's own (equal code hashes keep the VM helper's library constraint)."""
    engine = app / ENGINE_DISTRIBUTION / 'Contents'
    copies = [f'MacOS/{name}' for name in ENGINE_TOOLS] + [f'Frameworks/{name}' for name in ENGINE_FRAMEWORKS]
    for relative in copies + [f'Resources/{name}' for name in ENGINE_RESOURCES]:
        if not (engine / relative).exists():
            raise RuntimeError(f'The embedded engine is missing {relative}; silod cannot run computers without it.')
    for relative in copies:
        if code_hash(engine / relative) != code_hash(app / 'Contents' / relative):
            raise RuntimeError(f'The embedded engine\'s {relative} is not the app\'s signed copy.')


def verify_engine(app, identifier):
    """The embedded engine is sealed with the virtualization entitlement, carries its tools, and the
    app's LaunchAgent plist points at it."""
    engine = app / ENGINE_DISTRIBUTION
    executable = engine / 'Contents/MacOS/silod'
    command('codesign', '--verify', '--strict', engine)
    verify_engine_requirement(engine)
    verify_engine_contents(app)
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


def verify_bundle(app, identity='-'):
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
        elif name == 'silo':
            if entitlements:
                raise RuntimeError('The silo command must not carry host entitlements.')
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
            sign_helper(expected, allowed, constraint, identity)
            if code_hash(expected) != code_hash(helper):
                raise RuntimeError(f'{name} is missing the exact bundled-engine library constraint.')
