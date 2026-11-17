// The setup, store and doctor tests' harness: a temporary HOME, the command run in-process against
// the fake GitHub, its output checked for secrets.
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { main } from '../cli/main.js';
import { appKeys } from './fake-github.js';
import { assertNoSecrets, fakeKeychain, secretsOf } from './github-helpers.js';

export { assertNoSecrets, fakeKeychain, secretsOf };

/** A temporary directory holding `home/`. */
export function sandbox(t) {
  const base = mkdtempSync(join(process.env.ATELIER_TMP || tmpdir(), 'plan-setup-'));
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
    cwd: box.base,
    out: (s) => (stdout += `${s}\n`),
    err: (s) => (stderr += `${s}\n`),
    deps: {
      env,
      home: box.home,
      keychain: keychain ?? fakeKeychain({ WORKSHOP_APP_ID: fake.appId, WORKSHOP_PRIVATE_KEY: appKeys().privateKey }),
      api: fake.url,
      web: fake.url,
      repo: fake.repo,
      site: 'http://127.0.0.1:9',
      fetch: globalThis.fetch,
      writeDelayMs: 0,
      openBrowser: () => {},
      ...overrides,
    },
  });
  assertNoSecrets(stdout + stderr, secretsOf(fake));
  return { code, stdout, stderr };
}
