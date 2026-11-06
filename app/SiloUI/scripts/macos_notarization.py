"""Developer ID signing identity, notarization, stapling and Gatekeeper checks for macOS releases.

`SILO_RELEASE_SIGNING` selects the mode:

* `adhoc` (default): ad hoc signatures, no notarization. Local and artifact-only builds.
* `dry-run`: ad hoc signatures; every notarization, stapling and Gatekeeper command is printed
  instead of run, so the whole pipeline can be exercised without Apple credentials.
* `developer-id`: signs with the Developer ID Application identity, then notarizes, staples and
  assesses. Credentials come only from the environment (see
  docs/SiloUI-RELEASES.md#apple-signing-and-notarization).

`resolve-mode` turns a requested `developer-id` into `adhoc` when none of the Apple credentials
is configured (an unsigned release); a partial set is an error.

Printed commands show placeholders for credential values, never the values.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile

MODE_ENV = 'SILO_RELEASE_SIGNING'
MODES = ('adhoc', 'dry-run', 'developer-id')
DEVELOPER_ID_PREFIX = 'Developer ID Application:'
IDENTITY_ENV = 'APPLE_DEVELOPER_ID_IDENTITY'
NOTARY_ENV = ('APPLE_TEAM_ID', 'APPLE_API_KEY_P8', 'APPLE_API_KEY_ID', 'APPLE_API_ISSUER_ID')
CERTIFICATE_ENV = ('APPLE_CERTIFICATE_P12_BASE64', 'APPLE_CERTIFICATE_PASSWORD')
KEY_PLACEHOLDER = '<APPLE_API_KEY_P8 file>'


def release_mode(environ=os.environ):
    mode = environ.get(MODE_ENV, '').strip() or 'adhoc'
    if mode not in MODES:
        raise RuntimeError(f'{MODE_ENV} must be one of {", ".join(MODES)}, not {mode!r}.')
    return mode


APPLE_CREDENTIALS = (IDENTITY_ENV, *NOTARY_ENV, *CERTIFICATE_ENV)


def resolve_mode(environ=os.environ, warn=None):
    """The mode a release build uses. A requested `developer-id` with no Apple credentials at all
    becomes `adhoc`: signed ad hoc and not notarized. Any credential present requires all of them."""
    mode = release_mode(environ)
    if mode != 'developer-id':
        return mode
    present = [name for name in APPLE_CREDENTIALS if environ.get(name, '').strip()]
    if not present:
        if warn:
            warn('::warning::No Apple Developer ID credentials are configured: this release is signed '
                 'ad hoc and is not notarized. See docs/SiloUI-RELEASES.md#releases-without-developer-id-signing.')
        return 'adhoc'
    require_environment(APPLE_CREDENTIALS, environ)
    return mode


def require_environment(names, environ=os.environ):
    missing = [name for name in names if not environ.get(name, '').strip()]
    if missing:
        raise RuntimeError('Missing release signing configuration: ' + ', '.join(missing)
                           + '. See docs/SiloUI-RELEASES.md#apple-signing-and-notarization.')


def developer_id_identity(environ):
    identity = environ[IDENTITY_ENV].strip()
    if not identity.startswith(DEVELOPER_ID_PREFIX):
        raise RuntimeError(f'{IDENTITY_ENV} must name a "{DEVELOPER_ID_PREFIX}" certificate.')
    return identity


def release_identity(environ=os.environ):
    """The codesign identity of the selected mode: the Developer ID name, else ad hoc (`-`)."""
    if release_mode(environ) != 'developer-id':
        return '-'
    require_environment((IDENTITY_ENV,), environ)
    return developer_id_identity(environ)


class Release:
    """The signing identity and the notarization steps for one packaging run."""

    def __init__(self, mode='adhoc', identity='-', environ=os.environ, run=subprocess.run, out=print):
        self.mode, self.identity, self.environ, self._run, self._out = mode, identity, environ, run, out
        self.steps = []
        self._key = None

    @classmethod
    def from_environ(cls, environ=os.environ, run=subprocess.run, out=print):
        mode = release_mode(environ)
        if mode != 'developer-id':
            return cls(mode, '-', environ, run, out)
        require_environment((IDENTITY_ENV,) + NOTARY_ENV, environ)
        return cls(mode, developer_id_identity(environ), environ, run, out)

    @property
    def notarizes(self):
        return self.mode != 'adhoc'

    @property
    def dry(self):
        return self.mode == 'dry-run'

    def step(self, args, shown=None):
        """Runs one command, or only prints it in a dry run. `shown` replaces arguments that hold secrets."""
        shown = [str(a) for a in (shown or args)]
        self.steps.append(shown)
        if self.dry:
            self._out('[dry-run] ' + ' '.join(shown))
            return None
        return self._run([str(a) for a in args], check=True, capture_output=True, text=True)

    def _key_file(self):
        if self.dry:
            return KEY_PLACEHOLDER
        if self._key is None:
            handle, name = tempfile.mkstemp(prefix='silo-notary-', suffix='.p8')
            with os.fdopen(handle, 'w') as key:
                key.write(self.environ['APPLE_API_KEY_P8'])
            self._key = name
        return self._key

    def cleanup(self):
        if self._key is not None:
            Path(self._key).unlink(missing_ok=True)
            self._key = None

    def notarize(self, path):
        """Submits `path` (a zip or dmg) and waits; fails unless Apple accepts it."""
        if not self.notarizes:
            return
        env = self.environ
        args = ['xcrun', 'notarytool', 'submit', path, '--wait', '--output-format', 'json',
                '--key', self._key_file(), '--key-id', env.get('APPLE_API_KEY_ID', ''),
                '--issuer', env.get('APPLE_API_ISSUER_ID', ''), '--team-id', env.get('APPLE_TEAM_ID', '')]
        shown = args[:7] + ['--key', KEY_PLACEHOLDER, '--key-id', '<APPLE_API_KEY_ID>',
                            '--issuer', '<APPLE_API_ISSUER_ID>', '--team-id', '<APPLE_TEAM_ID>']
        result = self.step(args, shown)
        if result is None:
            return
        try:
            outcome = json.loads(result.stdout)
        except ValueError:
            raise RuntimeError('notarytool returned no readable result.') from None
        if outcome.get('status') != 'Accepted':
            raise RuntimeError(f'Notarization of {Path(path).name} ended as {outcome.get("status")!r} '
                               f'(submission {outcome.get("id")}); read it with `xcrun notarytool log`.')

    def staple(self, path):
        self.step(['xcrun', 'stapler', 'staple', path])
        self.step(['xcrun', 'stapler', 'validate', path])

    def notarize_app(self, app, workdir):
        """Notarizes, staples and assesses the whole app. The nested engine bundle is part of this
        submission and covered by its ticket; it is not stapled on its own, which would change the
        sealed app."""
        if not self.notarizes:
            return
        archive = Path(workdir) / f'{Path(app).stem}-notarize.zip'
        self.step(['ditto', '-c', '-k', '--keepParent', app, archive])
        self.notarize(str(archive))
        self.staple(app)
        self.step(['codesign', '--verify', '--deep', '--strict', '--verbose=2', app])
        self.step(['spctl', '--assess', '--type', 'execute', '--verbose=4', app])

    def notarize_dmg(self, dmg):
        """Signs, notarizes, staples and assesses the disk image."""
        if not self.notarizes:
            return
        self.step(['codesign', '--force', '--sign', self.identity, '--timestamp', dmg])
        self.step(['codesign', '--verify', '--strict', dmg])
        self.notarize(str(dmg))
        self.staple(dmg)
        self.step(['spctl', '--assess', '--type', 'open', '--context', 'context:primary-signature',
                   '--verbose=4', dmg])


def import_certificate(environ=os.environ, run=subprocess.run):
    """Imports the Developer ID p12 into a new temporary keychain, puts it on the search list and
    returns its path. A failed command is reported by name only: its arguments hold passwords. The
    keychain is deleted again on failure."""
    require_environment(CERTIFICATE_ENV, environ)
    base = Path(environ.get('RUNNER_TEMP') or tempfile.gettempdir())
    keychain = base / f'silo-signing-{secrets.token_hex(4)}.keychain-db'
    password = secrets.token_hex(24)
    p12 = keychain.with_suffix('.p12')
    p12.write_bytes(base64.b64decode(environ['APPLE_CERTIFICATE_P12_BASE64']))
    p12.chmod(0o600)
    try:
        for args in (['security', 'create-keychain', '-p', password, keychain],
                     ['security', 'set-keychain-settings', '-lut', '21600', keychain],
                     ['security', 'unlock-keychain', '-p', password, keychain],
                     ['security', 'import', p12, '-k', keychain, '-P', environ['APPLE_CERTIFICATE_PASSWORD'],
                      '-T', '/usr/bin/codesign', '-T', '/usr/bin/security'],
                     ['security', 'set-key-partition-list', '-S', 'apple-tool:,apple:,codesign:',
                      '-s', '-k', password, keychain]):
            run([str(a) for a in args], check=True, capture_output=True)
        listing = run(['security', 'list-keychains', '-d', 'user'], check=True, capture_output=True, text=True).stdout
        existing = [line.strip().strip('"') for line in listing.splitlines() if line.strip()]
        run(['security', 'list-keychains', '-d', 'user', '-s', str(keychain), *existing],
            check=True, capture_output=True)
    except subprocess.CalledProcessError as error:
        delete_keychain(str(keychain), run)
        raise RuntimeError(f'`security {error.cmd[1]}` failed with exit code {error.returncode} '
                           'while importing the signing certificate.') from None
    except BaseException:
        delete_keychain(str(keychain), run)
        raise
    finally:
        p12.unlink(missing_ok=True)
    return str(keychain)


def delete_keychain(keychain, run=subprocess.run):
    run(['security', 'delete-keychain', keychain], check=False, capture_output=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('import-certificate', 'delete-keychain', 'mode', 'resolve-mode'))
    parser.add_argument('keychain', nargs='?')
    args = parser.parse_args(argv)
    if args.command == 'import-certificate':
        print(import_certificate())
    elif args.command == 'delete-keychain':
        delete_keychain(args.keychain)
    elif args.command == 'resolve-mode':
        print(resolve_mode(warn=lambda line: print(line, file=sys.stderr)))
    else:
        print(release_mode())
    return 0


if __name__ == '__main__':
    sys.exit(main())
