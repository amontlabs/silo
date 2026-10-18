"""Embed the engine (`silod`) in a Silo app bundle so Silo registers it at launch.

The app carries a sealed `Silo Engine.app` and a copy of its LaunchAgent plist whose
`BundleProgram` points at the nested helper. Service Management resolves agent plists from
the calling bundle (`Contents/Library/LaunchAgents`), while the executable stays inside its own
bundle so it sees its own identifier, manifest and resources. Fjord embeds the identical
distribution; whichever app registers first supplies the one engine per Mac.
"""
from pathlib import Path
import json
import os
import plistlib
import re
import shutil
import subprocess

APP = Path(__file__).resolve().parent.parent
SILOD = APP / 'src-tauri/silod'
DISTRIBUTION = 'Contents/Resources/SiloEngineDistribution/Silo Engine.app'


SIGNING_IDENTITY_ENV = 'SILO_ENGINE_SIGNING_IDENTITY'


def signing_identity(environ=os.environ):
    """The identity that seals silod: `SILO_ENGINE_SIGNING_IDENTITY`, else ad hoc (`-`).

    An ad hoc signature changes with every build, so Keychain access lists that name silod stop
    matching after a rebuild. A stable identity keeps them valid (docs/SiloUI-ENGINE.md)."""
    return environ.get(SIGNING_IDENTITY_ENV, '').strip() or '-'


# silod reads this Info.plist key at run time and accepts only a bundle whose signature satisfies it.
# The packager writes it before signing: the bundle seals the Info.plist.
REQUIREMENT_KEY = 'SiloEngineCodeRequirement'
DEVELOPER_ID_CERTIFICATE = 'Developer ID Application:'


def resolve_signing_identity(environ=os.environ, run=subprocess.run):
    """What `codesign --sign` receives: `-` (ad hoc) or the SHA-1 of one valid signing identity.

    A name is matched exactly against `security find-identity -v -p codesigning`, which lists only
    certificates with a private key, and must match exactly one. Signing with the hash keeps the
    signer and the requirement pinned to the same certificate."""
    identity = signing_identity(environ)
    if identity == '-' or re.fullmatch(r'[0-9A-Fa-f]{40}', identity):
        return identity.upper() if identity != '-' else identity
    listing = run(['security', 'find-identity', '-v', '-p', 'codesigning'],
                  check=True, capture_output=True, text=True).stdout
    found = {digest for digest, name in re.findall(r'^\s*\d+\)\s+([0-9A-Fa-f]{40})\s+"(.*)"', listing, re.M)
             if name == identity}
    if len(found) != 1:
        raise RuntimeError(f'Expected exactly one valid code-signing identity named {identity!r}, found {len(found)}.')
    return found.pop().upper()


def certificate_with_hash(digest, run=subprocess.run):
    """The PEM of the keychain certificate whose SHA-1 is `digest`."""
    listing = run(['security', 'find-certificate', '-a', '-Z', '-p'],
                  check=True, capture_output=True, text=True).stdout
    for found, pem in re.findall(r'SHA-1 hash: ([0-9A-Fa-f]{40})\n.*?(-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----)',
                                 listing, re.S):
        if found.upper() == digest.upper():
            return pem
    raise RuntimeError(f'No keychain certificate has SHA-1 {digest}.')


def engine_requirement(identifier, identity, run=subprocess.run):
    """The code requirement that the signer of the engine named `identifier` must satisfy.

    `identity` is from `resolve_signing_identity`. A Developer ID certificate with a team is trusted
    by its team (and Apple's Developer ID chain), any other certificate by its exact hash, and ad hoc
    by the bundle identifier."""
    if identity == '-':
        return f'identifier "{identifier}"'
    pinned = f'certificate leaf = H"{identity.lower()}"'
    subject = run(['openssl', 'x509', '-noout', '-subject', '-nameopt', 'RFC2253'],
                  input=certificate_with_hash(identity, run), check=True, capture_output=True, text=True).stdout.strip()
    team = re.search(r'(?:^|,)OU=([A-Z0-9]{10})(?:,|$)', subject.removeprefix('subject=').strip())
    if DEVELOPER_ID_CERTIFICATE in subject and team:
        return ('anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists'
                ' and certificate leaf[field.1.2.840.113635.100.6.1.13] exists'
                f' and certificate leaf[subject.OU] = "{team.group(1)}"')
    return pinned


def codesign_engine(engine, entitlements, run=subprocess.run, environ=os.environ, identity=None):
    run(['codesign', '--force', '--sign', identity or signing_identity(environ), '--options', 'runtime',
         '--entitlements', str(entitlements), str(engine)], check=True, capture_output=True)
    run(['codesign', '--verify', '--strict', str(engine)], check=True, capture_output=True)


def embed_engine(bundle, silod_binary, *, identifier='org.silo.preview', build,
                 source=SILOD, sign=None, requirement=None, environ=os.environ, run=subprocess.run):
    """Copies the engine into `bundle` and signs it. `build` names the engine build.

    `requirement` is written to the engine's Info.plist before signing; by default it is derived
    from the signing identity (`engine_requirement`)."""
    identity = resolve_signing_identity(environ, run)
    if sign is None:
        def sign(engine, entitlements):
            codesign_engine(engine, entitlements, run=run, environ=environ, identity=identity)
    bundle, silod_binary, source = Path(bundle), Path(silod_binary), Path(source)
    channel = 'production' if identifier == 'org.silo.preview' else 'development'
    agent = f'{identifier}.engine.plist'
    inputs = {
        'plist': source / 'macos/LaunchAgents' / agent,
        'info': source / f'macos/Info.{channel}.plist',
        'manifest': source / f'macos/resources/engine-manifest.{channel}.json',
        'entitlements': source / 'macos/Entitlements.plist',
        'binary': silod_binary,
    }
    for name, path in inputs.items():
        if not path.is_file():
            raise FileNotFoundError(f'Missing engine {name}: {path}')

    engine = bundle / DISTRIBUTION
    shutil.rmtree(engine, ignore_errors=True)
    contents = engine / 'Contents'
    for directory in ('MacOS', 'Library/LaunchAgents', 'Resources'):
        (contents / directory).mkdir(parents=True)
    executable = contents / 'MacOS/silod'
    shutil.copy2(inputs['binary'], executable)
    executable.chmod(0o755)
    info = plistlib.loads(inputs['info'].read_bytes())
    info[REQUIREMENT_KEY] = requirement or engine_requirement(f'{identifier}.engine', identity, run)
    (contents / 'Info.plist').write_bytes(plistlib.dumps(info))
    shutil.copy2(inputs['plist'], contents / 'Library/LaunchAgents' / agent)
    manifest = json.loads(inputs['manifest'].read_text(encoding='utf-8'))
    manifest['engineBuild'] = build
    (contents / 'Resources/engine-manifest.json').write_text(
        json.dumps(manifest, indent=2) + '\n', encoding='utf-8')

    # SMAppService resolves the plist from the app bundle; the program is the nested helper.
    outer = bundle / 'Contents/Library/LaunchAgents' / agent
    outer.parent.mkdir(parents=True, exist_ok=True)
    with inputs['plist'].open('rb') as handle:
        plist = plistlib.load(handle)
    plist['BundleProgram'] = f'{DISTRIBUTION}/Contents/MacOS/silod'
    with outer.open('wb') as handle:
        plistlib.dump(plist, handle)

    # Nested code is signed first and never with --deep, which would replace its signature.
    sign(engine, inputs['entitlements'])
    return engine, outer
