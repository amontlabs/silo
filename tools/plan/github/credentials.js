// Credential lookup: environment, then the Keychain, then
// ~/.config/silo-workshop/credentials.json (mode 0600 or refused). Values are
// returned to callers and never printed; only their sources are reported.
import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { Failure, NotConfigured } from './errors.js';

/** @param {string} home */
export function credentialsFile(home) {
  return join(home, '.config', 'silo-workshop', 'credentials.json');
}

/** @param {string} home */
export function stateDir(home) {
  return join(home, '.local', 'state', 'silo-workshop');
}

/** @param {string} home */
function readFile(home) {
  const path = credentialsFile(home);
  let stat;
  try {
    stat = statSync(path);
  } catch {
    return null;
  }
  if ((stat.mode & 0o077) !== 0) throw new Failure(`${path} must be readable only by you (chmod 600); refusing to read it`);
  let data;
  try {
    data = JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    throw new Failure(`${path} is not valid JSON`);
  }
  if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Failure(`${path} must hold a JSON object`);
  return data;
}

/**
 * Looks up each name. A locked Keychain throws; it never reads as "absent".
 * @param {readonly string[]} names @param {{ env: Record<string, string | undefined>, home: string, keychain: { get(name: string): string | null } }} deps
 * @returns {{ values: Record<string, string>, sources: Record<string, 'environment' | 'keychain' | 'file' | null> }}
 */
export function lookup(names, { env, home, keychain }) {
  /** @type {Record<string, string>} */
  const values = {};
  /** @type {Record<string, any>} */
  const sources = {};
  let file;
  for (const name of names) {
    if (env[name]) {
      values[name] = /** @type {string} */ (env[name]);
      sources[name] = 'environment';
      continue;
    }
    const fromKeychain = keychain.get(name);
    if (fromKeychain) {
      values[name] = fromKeychain;
      sources[name] = 'keychain';
      continue;
    }
    if (file === undefined) file = readFile(home);
    if (file && typeof file[name] === 'string' && file[name]) {
      values[name] = file[name];
      sources[name] = 'file';
      continue;
    }
    sources[name] = null;
  }
  return { values, sources };
}

/** The App id and private key; NotConfigured (exit 3) naming what is missing, never a value. */
export function appCredentials(deps) {
  const { values, sources } = lookup(['WORKSHOP_APP_ID', 'WORKSHOP_PRIVATE_KEY'], deps);
  const missing = Object.keys(sources).filter((name) => !sources[name]);
  if (missing.length) throw new NotConfigured(`missing ${missing.join(' and ')}: run \`plan setup\`, or see tools/plan/README.md`);
  return { appId: values.WORKSHOP_APP_ID, privateKey: values.WORKSHOP_PRIVATE_KEY, sources };
}
