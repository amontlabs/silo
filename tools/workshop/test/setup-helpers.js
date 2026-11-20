// The setup, store and doctor tests' harness: a temporary HOME, the command run in-process against
// the fake GitHub, its output checked for secrets.
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { main } from '../cli/main.js';
import { appKeys } from './fake-github.js';

export function fakeKeychain(initial = {}) {
  const store = new Map(Object.entries(initial));
  return { store, available: () => true, get: (name) => store.get(name) ?? null, set: (name, value) => store.set(name, value) };
}

/** Every value that must never appear in output. */
export function secretsOf(fake, extra = []) {
  const pemLines = appKeys().privateKey.split('\n').filter((line) => line && !line.startsWith('-----'));
  return [...pemLines, ...fake.secrets, ...fake.tokens, ...fake.jwts, ...extra].filter(Boolean);
}

export function assertNoSecrets(output, secrets) {
  for (const secret of secrets) assert.ok(!output.includes(secret), 'a secret appeared in the output');
}

/** A temporary directory holding `home/`. */
export function sandbox(t) {
  const base = mkdtempSync(join(process.env.ATELIER_TMP || tmpdir(), 'workshop-'));
  t.after(() => rmSync(base, { recursive: true, force: true }));
  const home = join(base, 'home');
  mkdirSync(home, { recursive: true });
  return { base, home };
}

/** Runs the command in-process against the fake; returns { code, stdout, stderr }. */
export async function run(argv, { fake, box, keychain, env = {}, overrides = {} }) {
  let stdout = '';
  let stderr = '';
  const code = await main(argv, {
    out: (s) => (stdout += `${s}\n`),
    err: (s) => (stderr += `${s}\n`),
    deps: {
      env,
      home: box.home,
      keychain: keychain ?? fakeKeychain({ WORKSHOP_APP_ID: fake.appId, WORKSHOP_PRIVATE_KEY: appKeys().privateKey }),
      api: fake.url,
      web: fake.url,
      repo: fake.repo,
      fetch: globalThis.fetch,
      writeDelayMs: 0,
      openBrowser: () => {},
      ...overrides,
    },
  });
  assertNoSecrets(stdout + stderr, secretsOf(fake));
  return { code, stdout, stderr };
}
