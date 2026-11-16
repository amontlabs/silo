// The macOS Keychain through /usr/bin/security: generic passwords of one
// service, one item per name. A value is stored as "base64:<base64>" through
// `security -i` on stdin, so no secret appears on a command line and the
// multi-line PEM survives. `security -i` reads at most about 4,095 bytes per
// line and runs the rest as another command, so a line of 4,000 bytes or more
// is refused before anything runs.
import { spawnSync as nodeSpawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { Failure, KEYCHAIN_SERVICE } from './names.js';

const SECURITY = '/usr/bin/security';
const PREFIX = 'base64:';
// errSecItemNotFound, as `security find-generic-password` reports it.
const NOT_FOUND = 44;
export const MAX_LINE = 4000;

const UNAVAILABLE = 'the Keychain is locked or unavailable';

/** The one `security -i` line that stores a value; throws when it would be too long to be read as one line. */
export function addCommand(name, value) {
  if (!/^WORKSHOP_[A-Z_]+$/.test(name)) throw new Error(`not a Workshop credential name: ${name}`);
  const encoded = PREFIX + Buffer.from(value, 'utf8').toString('base64');
  const line = `add-generic-password -U -s ${KEYCHAIN_SERVICE} -a ${name} -l "Silo Workshop ${name}" -w ${encoded}\n`;
  if (Buffer.byteLength(line) >= MAX_LINE) throw new Failure(`${name} is too long to store in the Keychain in one step`);
  return line;
}

export function decodeValue(raw) {
  return raw.startsWith(PREFIX) ? Buffer.from(raw.slice(PREFIX.length), 'base64').toString('utf8') : raw;
}

export function createMacKeychain({ spawnSync = nodeSpawnSync, platform = process.platform, exists = existsSync } = {}) {
  return {
    available() {
      return platform === 'darwin' && exists(SECURITY);
    },
    /** The value, or null when the item does not exist; any other failure throws. */
    get(name) {
      if (!this.available()) return null;
      const r = spawnSync(SECURITY, ['find-generic-password', '-s', KEYCHAIN_SERVICE, '-a', name, '-w'], {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      });
      if (r.status === NOT_FOUND) return null;
      if (r.status !== 0 || r.error) throw new Failure(UNAVAILABLE);
      return decodeValue(r.stdout.replace(/\n$/, ''));
    },
    /** Stores the value and reads it back; throws when it did not land. */
    set(name, value) {
      if (!this.available()) throw new Failure(UNAVAILABLE);
      const r = spawnSync(SECURITY, ['-i'], { input: addCommand(name, value), stdio: ['pipe', 'ignore', 'ignore'] });
      if (r.status !== 0 || r.error) throw new Failure(`${UNAVAILABLE}: could not store ${name}`);
      let stored;
      try {
        stored = this.get(name);
      } catch {
        stored = null;
      }
      if (stored !== value) throw new Failure(`${UNAVAILABLE}: ${name} did not store correctly`);
    },
  };
}

export const macKeychain = createMacKeychain();
