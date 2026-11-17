// Helpers for the tests of the GitHub side: a fake Keychain, a checkout with an origin to fetch
// from, the command run against the fake GitHub with every output checked for secrets.
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { appKeys, startFake } from './fake-github.js';
import { cleanup, git, run, temp, writePlan } from './helpers.js';
import { canonicalFeature, canonicalText } from '../model/canonical.js';

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

/**
 * A fake GitHub and a checkout: `origin` (bare) and `root` (a clone with the plan pushed to main).
 * @param {import('node:test').TestContext} t @param {Record<string, unknown>} files
 */
export async function world(t, files) {
  const fake = await startFake();
  const base = temp('plan-gh-');
  t.after(async () => {
    await fake.stop();
    cleanup(base);
  });
  const origin = join(base, 'origin.git');
  const root = join(base, 'work');
  const home = join(base, 'home');
  mkdirSync(home);
  git(base, 'init', '-q', '--bare', '-b', 'main', origin);
  writePlan(files, root);
  git(root, 'init', '-q', '-b', 'main');
  git(root, 'remote', 'add', 'origin', origin);
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'Plan');
  git(root, 'push', '-q', 'origin', 'main');
  const keychain = fakeKeychain({ WORKSHOP_APP_ID: fake.appId, WORKSHOP_PRIVATE_KEY: appKeys().privateKey });
  const deps = { env: {}, home, keychain, fetch: globalThis.fetch, api: fake.url, web: 'https://github.com', repo: fake.repo, site: 'http://127.0.0.1:9', writeDelayMs: 0 };

  const w = {
    fake, base, root, origin, home, keychain, deps,
    /** Reads a feature file of the checkout. */
    feature: (id) => JSON.parse(readFileSync(join(root, 'plan', 'features', `${id}.json`), 'utf8')),
    /** Changes features in the checkout (a value of undefined removes a key; `notes` writes the .md), commits and pushes. */
    change(edits, message = 'Change') {
      for (const [id, fields] of Object.entries(edits)) {
        const path = join(root, 'plan', 'features', `${id}.json`);
        const data = JSON.parse(readFileSync(path, 'utf8'));
        for (const [key, value] of Object.entries(fields)) {
          if (key === 'notes') writeFileSync(join(root, 'plan', 'features', `${id}.md`), value);
          else if (value === undefined) delete data[key];
          else data[key] = value;
        }
        writeFileSync(path, canonicalText(canonicalFeature(data)));
      }
      git(root, 'add', '-A');
      git(root, 'commit', '-q', '-m', message);
      git(root, 'push', '-q', '-f', 'origin', 'HEAD:main');
      return git(root, 'rev-parse', 'HEAD').trim();
    },
    head: () => git(root, 'rev-parse', 'HEAD').trim(),
    /** Runs the command in the checkout against the fake; asserts no secret in its output. */
    async run(argv, context = {}) {
      const result = await run(root, argv, { ...context, deps: { ...deps, ...context.deps } });
      assertNoSecrets(result.out + result.err, secretsOf(fake));
      return result;
    },
  };
  return w;
}
