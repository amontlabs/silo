// Reads the Workshop's settings from the environment. An invalid value leaves
// the server unconfigured; `problem` names the variable, never its value.
import { createPrivateKey } from 'node:crypto';

export const DEFAULT_REPO = 'amontlabs/silo';
export const DEFAULT_ORIGIN = 'https://silo.amontlabs.com';

const REPO_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const SECRET_PATTERN = /^[A-Za-z0-9+/]{43}=$/;
const REQUIRED = ['WORKSHOP_APP_ID', 'WORKSHOP_CLIENT_ID', 'WORKSHOP_CLIENT_SECRET', 'WORKSHOP_PRIVATE_KEY', 'WORKSHOP_SESSION_SECRET'];

export function readConfig(env = process.env, overrides = {}) {
  let problem = null;
  const fail = (name) => { problem ??= name; return null; };

  const repo = env.WORKSHOP_REPO || DEFAULT_REPO;
  if (!REPO_PATTERN.test(repo)) fail('WORKSHOP_REPO');
  const origin = parseOrigin(env.WORKSHOP_SITE_ORIGIN || DEFAULT_ORIGIN) ?? fail('WORKSHOP_SITE_ORIGIN');

  const missing = REQUIRED.filter((name) => !env[name]);
  let sessionSecret = null;
  let privateKey = null;
  if (env.WORKSHOP_APP_ID && !/^\d{1,12}$/.test(env.WORKSHOP_APP_ID)) fail('WORKSHOP_APP_ID');
  // The owner: the numeric id of a GitHub account, which never changes (a login can). Without it nobody is the owner.
  const ownerId = env.WORKSHOP_OWNER_ID ? (/^[1-9]\d{0,15}$/.test(env.WORKSHOP_OWNER_ID) ? Number(env.WORKSHOP_OWNER_ID) : fail('WORKSHOP_OWNER_ID')) : null;
  if (env.WORKSHOP_SESSION_SECRET) {
    // Strict base64: exactly 32 bytes whose canonical encoding is the value itself.
    const raw = env.WORKSHOP_SESSION_SECRET;
    sessionSecret = SECRET_PATTERN.test(raw) ? Buffer.from(raw, 'base64') : null;
    if (sessionSecret?.length !== 32 || sessionSecret.toString('base64') !== raw) sessionSecret = fail('WORKSHOP_SESSION_SECRET');
  }
  if (env.WORKSHOP_PRIVATE_KEY) {
    try {
      privateKey = createPrivateKey(env.WORKSHOP_PRIVATE_KEY.replace(/\\n/g, '\n'));
      if (privateKey.asymmetricKeyType !== 'rsa') privateKey = fail('WORKSHOP_PRIVATE_KEY');
    } catch {
      privateKey = fail('WORKSHOP_PRIVATE_KEY');
    }
  }

  const config = {
    appId: env.WORKSHOP_APP_ID || null,
    clientId: env.WORKSHOP_CLIENT_ID || null,
    clientSecret: env.WORKSHOP_CLIENT_SECRET || null,
    privateKey,
    sessionSecret,
    ownerId,
    repo: REPO_PATTERN.test(repo) ? repo : DEFAULT_REPO,
    origin: origin || DEFAULT_ORIGIN,
    apiBase: 'https://api.github.com',
    webBase: 'https://github.com',
    ...overrides,
  };
  config.secureCookies ??= config.origin.startsWith('https://');
  config.problem = problem;
  config.missing = missing;
  config.configured = !problem && missing.length === 0;
  return config;
}

function parseOrigin(value) {
  try {
    const url = new URL(value);
    if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
    if (url.username || url.password || (url.pathname !== '/' && url.pathname !== '') || url.search || url.hash) return null;
    return url.origin;
  } catch {
    return null;
  }
}
