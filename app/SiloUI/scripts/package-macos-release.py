"""Produce matching DMG and signed update archive with constrained VM loading."""
import argparse
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
from embed_engine import embed_engine
from macos_release_signing import sign_runtime, verify_bundle, verify_engine
from channel_names import channel_names


def engine_build():
    commit = os.environ.get('GITHUB_SHA', '').strip()
    if not commit:
        result = subprocess.run(['git', 'rev-parse', 'HEAD'], capture_output=True, text=True,
                                cwd=Path(__file__).resolve().parent)
        commit = result.stdout.strip() if result.returncode == 0 else ''
    return f'silo-{commit or "unknown"}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--engine', type=Path, required=True,
                        help='the release-built silod binary to embed and sign')
    parser.add_argument('--engine-build', default=None,
                        help='the engine build name (default: silo-<GITHUB_SHA or git HEAD>)')
    args = parser.parse_args()
    app, output = args.bundle.resolve(), args.output.resolve()
    product_name = channel_names()["production"]["productName"]
    bundle_name = f"{product_name}.app"
    if app.name != bundle_name or not (app / 'Contents/MacOS/msb').is_file():
        raise RuntimeError(f'Expected a built {bundle_name} with its VM runtime.')
    key = os.environ.get('TAURI_SIGNING_PRIVATE_KEY', '')
    if not key:
        raise RuntimeError('An updater signing key is required.')
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f'{product_name}-macos-arm64.app.tar.gz'
    dmg = output / f'{product_name}-macos-arm64.dmg'
    if any(path.exists() for path in (archive, Path(str(archive) + '.sig'), dmg)):
        raise RuntimeError('Release assets already exist; use a fresh output directory.')
    identifier = channel_names()["production"]["identifier"]
    # The nested engine is signed first with the virtualization entitlement; sign_runtime then
    # re-seals the outer bundle without --deep, which keeps the nested signature.
    embed_engine(app, args.engine.resolve(), identifier=identifier, build=args.engine_build or engine_build())
    sign_runtime(app)
    verify_bundle(app)
    verify_engine(app, identifier)
    # Python tarfile does not insert Apple's resource-fork sidecar entries.
    with tarfile.open(archive, 'w:gz') as tar:
        tar.add(app, arcname=bundle_name)
    with tarfile.open(archive, 'r:gz') as tar:
        if any(member.name.split('/')[0] != bundle_name for member in tar):
            raise RuntimeError(f'The update archive must contain only {bundle_name}.')
    signer = ['npx', 'tauri', 'signer', 'sign']
    try:
        is_file = Path(key).is_file()
    except OSError:
        is_file = False
    signer_env = dict(os.environ)
    signer_env.pop('TAURI_SIGNING_PRIVATE_KEY_PATH', None)
    if is_file:
        signer += ['-f', key]
        # Tauri treats this environment variable as --private-key (contents),
        # which conflicts with --private-key-path even when both name one file.
        signer_env.pop('TAURI_SIGNING_PRIVATE_KEY', None)
    # Tauri reads key contents and the password from its environment.
    signer_env.setdefault('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', '')
    try:
        subprocess.run(signer + [str(archive)], env=signer_env, check=True, stdout=subprocess.DEVNULL)
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f'Updater signing failed with exit code {error.returncode}.') from None
    with tempfile.TemporaryDirectory(prefix='silo-dmg-') as temporary:
        stage = Path(temporary) / 'image'
        stage.mkdir()
        subprocess.run(['ditto', str(app), str(stage / bundle_name)], check=True)
        (stage / 'Applications').symlink_to('/Applications')
        subprocess.run(['hdiutil', 'create', '-volname', product_name, '-srcfolder', str(stage),
                        '-format', 'UDZO', str(dmg)], check=True)
    print(f'Created verified release assets in {output}')


if __name__ == '__main__':
    main()
