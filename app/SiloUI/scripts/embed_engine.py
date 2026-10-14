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


def codesign_engine(engine, entitlements, run=subprocess.run, environ=os.environ):
    run(['codesign', '--force', '--sign', signing_identity(environ), '--options', 'runtime',
         '--entitlements', str(entitlements), str(engine)], check=True, capture_output=True)
    run(['codesign', '--verify', '--strict', str(engine)], check=True, capture_output=True)


def embed_engine(bundle, silod_binary, *, identifier='org.silo.preview', build,
                 source=SILOD, sign=codesign_engine):
    """Copies the engine into `bundle` and signs it. `build` names the engine build."""
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
    shutil.copy2(inputs['info'], contents / 'Info.plist')
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
