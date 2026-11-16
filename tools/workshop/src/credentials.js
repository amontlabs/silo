// Credential lookup: environment, then the Keychain, then
// ~/.config/silo-workshop/credentials.json (mode 0600 or refused). Values are
// returned to callers and never printed; `describe` reports only sources.
import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { Failure } from './names.js';

export function credentialsFile(home) {
  return join(home, '.config', 'silo-workshop', 'credentials.json');
}

export function stateDir(home) {
  return join(home, '.local', 'state', 'silo-workshop');
}

function readFile(home) {
  const path = credentialsFile(home);
  let stat;
  try {
    stat = statSync(path);
  } catch {
    return null;
  }
  if ((stat.mode & 0o077) !== 0) {
    throw new Failure(`${path} must be readable only by you (chmod 600); refusing to read it`);
  }
  let data;
  try {
    data = JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    throw new Failure(`${path} is not valid JSON`);
  }
  if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Failure(`${path} must hold a JSON object`);
  return data;
}

/** Looks up each name; returns { values: {name: value}, sources: {name: 'environment'|'keychain'|'file'|null} }. */
export function lookup(names, { env, home, keychain }) {
  const values = {};
  const sources = {};
  let file;
  for (const name of names) {
    if (env[name]) {
      values[name] = env[name];
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

/** The App id and private key, or a Failure naming what is missing. */
export function appCredentials(deps) {
  const { values, sources } = lookup(['WORKSHOP_APP_ID', 'WORKSHOP_PRIVATE_KEY'], deps);
  const missing = Object.keys(sources).filter((name) => !sources[name]);
  if (missing.length) {
    throw new Failure(`missing ${missing.join(' and ')}: run \`workshop setup\`, or see tools/workshop/README.md`);
  }
  return { appId: values.WORKSHOP_APP_ID, privateKey: values.WORKSHOP_PRIVATE_KEY, sources };
}
