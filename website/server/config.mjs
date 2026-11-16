// Reads the Workshop's settings from the environment. Nothing here is logged.

export const DEFAULT_REPO = 'amontlabs/silo';
export const DEFAULT_ORIGIN = 'https://silo.amontlabs.com';

const REPO_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;

export function readConfig(env = process.env, overrides = {}) {
  const repo = env.WORKSHOP_REPO || DEFAULT_REPO;
  if (!REPO_PATTERN.test(repo)) throw new Error('WORKSHOP_REPO must be owner/name');
  const origin = normalizeOrigin(env.WORKSHOP_SITE_ORIGIN || DEFAULT_ORIGIN);
  const secret = env.WORKSHOP_SESSION_SECRET ? Buffer.from(env.WORKSHOP_SESSION_SECRET, 'base64') : null;
  const privateKey = env.WORKSHOP_PRIVATE_KEY ? env.WORKSHOP_PRIVATE_KEY.replace(/\\n/g, '\n') : null;
  const config = {
    appId: env.WORKSHOP_APP_ID || null,
    clientId: env.WORKSHOP_CLIENT_ID || null,
    clientSecret: env.WORKSHOP_CLIENT_SECRET || null,
    privateKey,
    sessionSecret: secret && secret.length >= 32 ? secret : null,
    repo,
    origin,
    apiBase: 'https://api.github.com',
    webBase: 'https://github.com',
    secureCookies: origin.startsWith('https://'),
    ...overrides,
  };
  config.configured = Boolean(config.appId && config.clientId && config.clientSecret && config.privateKey && config.sessionSecret);
  return config;
}

export function normalizeOrigin(value) {
  const url = new URL(value);
  if (url.protocol !== 'https:' && url.protocol !== 'http:') throw new Error('WORKSHOP_SITE_ORIGIN must be an http(s) origin');
  return url.origin;
}
