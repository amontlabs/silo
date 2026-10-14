"""Build desktop apps, finalizing the VM signature on local macOS bundles.

Two channels exist and never share state: production (`org.silo.preview`, "Silo")
and development (`org.silo.dev`, "Silo Dev"). A build is production unless it is
a debug build (`--debug`), which always gets the development identity.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys

from macos_release_signing import sign_runtime, verify_bundle
from channel_names import channel_names
from embed_engine import embed_engine

APP = Path(__file__).resolve().parent.parent
DEVELOPMENT_CONFIG = 'src-tauri/tauri.dev.conf.json'


def build(arguments, *, root=APP, platform=sys.platform, run=subprocess.run):
    root = Path(root).resolve()
    tauri = ['node', str(root / 'node_modules/@tauri-apps/cli/tauri.js'), 'build']
    cargo_arguments = []
    if '--' in arguments:
        separator = arguments.index('--')
        arguments, cargo_arguments = arguments[:separator], arguments[separator:]
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument('--debug', '-d', action='store_true')
    parser.add_argument('--no-bundle', action='store_true')
    parser.add_argument('--help', '-h', action='store_true')
    parser.add_argument('--version', '-V', action='store_true')
    parser.add_argument('--target', '-t')
    parser.add_argument('--bundles', '-b', nargs='+')
    options, _ = parser.parse_known_args(arguments)
    if options.debug:
        arguments = arguments + ['--config', DEVELOPMENT_CONFIG]
    if (platform == 'linux' and not options.no_bundle
            and not options.help and not options.version
            and (not options.target or options.target.endswith('unknown-linux-gnu'))):
        run(tauri + arguments + [
            '--config', 'src-tauri/tauri.linux.package.conf.json',
            '--config', json.dumps({'bundle': {'createUpdaterArtifacts': False}}),
        ] + cargo_arguments, cwd=root, check=True)
        return None
    if (platform != 'darwin' or options.debug or options.no_bundle or options.help or options.version
            or (options.target and not options.target.endswith('apple-darwin'))):
        run(tauri + arguments + cargo_arguments, cwd=root, check=True)
        return None
    if options.target and options.target != 'aarch64-apple-darwin':
        raise ValueError('Local macOS bundles require the supported aarch64-apple-darwin target.')
    if options.bundles is not None and options.bundles != ['app']:
        raise ValueError('Local macOS builds support --bundles app. Use the release workflow for DMG and updater packages.')
    if cargo_arguments:
        raise ValueError('Use CARGO_TARGET_DIR for a separate local bundle output; raw Cargo arguments require --no-bundle.')

    metadata = run(['cargo', 'metadata', '--format-version', '1', '--no-deps', '--locked'],
                   cwd=root / 'src-tauri', check=True, capture_output=True, text=True)
    target = Path(json.loads(metadata.stdout)['target_directory'])
    if options.target:
        target /= options.target
    production = channel_names()['production']
    bundle = target / 'release/bundle/macos' / f"{production['productName']}.app"
    # Generate only the app here. Installers and updater archives must be made
    # after runtime finalization by package-macos-release.py.
    config = json.dumps({'identifier': production['identifier'], 'productName': production['productName'],
                         'bundle': {'active': True, 'createUpdaterArtifacts': False,
                                    'macOS': {'hardenedRuntime': True}}})
    args = arguments + ([] if options.bundles else ['--bundles', 'app'])
    run(tauri + args + ['--config', config], cwd=root, check=True)
    # The app is a client of the engine and carries its own copy for Service Management.
    run(['cargo', 'build', '--locked', '--release', '-p', 'silod'] + (['--target', options.target] if options.target else []),
        cwd=root, check=True)
    commit = (run(['git', 'rev-parse', 'HEAD'], cwd=root, check=False, capture_output=True, text=True).stdout or '').strip() or 'unknown'
    silod = target / 'release/silod'
    embed_engine(bundle, silod, identifier=production['identifier'], build=f'silo-{commit}')
    sign_runtime(bundle)
    verify_bundle(bundle)
    return bundle


if __name__ == '__main__':
    try:
        bundle = build(sys.argv[1:])
        if bundle:
            print(f'Verified local macOS app: {bundle}')
    except (ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f'Desktop build failed: {error}', file=sys.stderr)
        sys.exit(1)
