// Test helpers: a temp HOME and repository copy, a fake Keychain, and the
// command run in-process with its output captured and checked for secrets.
import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { main } from '../src/cli.js';
import { appKeys } from './fake-github.js';

export const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), 'fixtures');
// The fixture each plan file is copied from. DECISIONS.md is kept as a .txt so the repository's
// Markdown link check does not read its relative links, which point into a docs/product folder.
export const FIXTURE_FILES = { 'features.jsonl': 'features.jsonl', 'DECISIONS.md': 'decisions.txt' };

export function fakeKeychain(initial = {}) {
  const store = new Map(Object.entries(initial));
  return {
    store,
    available: () => true,
    get: (name) => store.get(name) ?? null,
    set: (name, value) => store.set(name, value),
  };
}

/** A temp directory holding `home/` and `repo/docs/product/` (copies of the files in `fixtures/`, or the given texts). */
export function sandbox(t, { features, decisions } = {}) {
  const base = mkdtempSync(join(process.env.ATELIER_TMP || tmpdir(), 'workshop-test-'));
  t.after(() => rmSync(base, { recursive: true, force: true }));
  const home = join(base, 'home');
  const root = join(base, 'repo');
  const product = join(root, 'docs', 'product');
  mkdirSync(home, { recursive: true });
  mkdirSync(product, { recursive: true });
  const write = (name, text) => {
    if (text === undefined) copyFileSync(join(FIXTURES, FIXTURE_FILES[name]), join(product, name));
    else writeFileSync(join(product, name), text);
  };
  write('features.jsonl', features);
  write('DECISIONS.md', decisions);
  return {
    base,
    home,
    root,
    featuresPath: join(product, 'features.jsonl'),
    decisionsPath: join(product, 'DECISIONS.md'),
    features: () => readFileSync(join(product, 'features.jsonl'), 'utf8'),
    decisions: () => readFileSync(join(product, 'DECISIONS.md'), 'utf8'),
  };
}

/** Every value that must never appear in output. */
export function secretsOf(fake, extra = []) {
  const pemLines = appKeys().privateKey.split('\n').filter((line) => line && !line.startsWith('-----'));
  return [...pemLines, ...fake.secrets, ...fake.tokens, ...fake.jwts, ...extra].filter(Boolean);
}

export function assertNoSecrets(output, secrets) {
  for (const secret of secrets) assert.ok(!output.includes(secret), 'a secret appeared in the output');
}

/** Runs the command in-process against the fake; returns { code, stdout, stderr }. */
export async function run(argv, { fake, box, keychain, env = {}, stdin = '', overrides = {} }) {
  let stdout = '';
  let stderr = '';
  const code = await main(argv, {
    env,
    home: box.home,
    root: box.root,
    keychain: keychain ?? fakeKeychain({ WORKSHOP_APP_ID: fake.appId, WORKSHOP_PRIVATE_KEY: appKeys().privateKey }),
    api: fake.url,
    web: fake.url,
    repo: fake.repo,
    fetch: globalThis.fetch,
    writeDelayMs: 0,
    stdout: { write: (s) => (stdout += s) },
    stderr: { write: (s) => (stderr += s) },
    readStdin: () => stdin,
    openBrowser: () => {},
    now: () => new Date(),
    ...overrides,
  });
  assertNoSecrets(stdout + stderr, secretsOf(fake));
  return { code, stdout, stderr };
}
