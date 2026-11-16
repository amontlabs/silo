// The macOS Keychain through /usr/bin/security: generic passwords of one
// service, one item per name. Values are stored base64-encoded (with a
// "base64:" prefix) and passed on stdin to `security -i`, so a secret never
// appears on a command line and multi-line values (the PEM key) survive.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { KEYCHAIN_SERVICE } from './names.js';

const SECURITY = '/usr/bin/security';
const PREFIX = 'base64:';

export const macKeychain = {
  available() {
    return process.platform === 'darwin' && existsSync(SECURITY);
  },
  get(name) {
    if (!this.available()) return null;
    const r = spawnSync(SECURITY, ['find-generic-password', '-s', KEYCHAIN_SERVICE, '-a', name, '-w'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    if (r.status !== 0) return null;
    const raw = r.stdout.replace(/\n$/, '');
    return raw.startsWith(PREFIX) ? Buffer.from(raw.slice(PREFIX.length), 'base64').toString('utf8') : raw;
  },
  set(name, value) {
    if (!this.available()) throw new Error('the macOS Keychain is not available');
    const hex = Buffer.from(PREFIX + Buffer.from(value, 'utf8').toString('base64'), 'utf8').toString('hex');
    const command = `add-generic-password -U -s ${KEYCHAIN_SERVICE} -a ${name} -l "Silo Workshop ${name}" -X ${hex}\n`;
    const r = spawnSync(SECURITY, ['-i'], { input: command, stdio: ['pipe', 'ignore', 'ignore'] });
    if (r.status !== 0) throw new Error(`could not store ${name} in the Keychain`);
  },
};
