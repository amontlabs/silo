import assert from 'node:assert/strict';
import { readFileSync, statSync } from 'node:fs';
import { dirname } from 'node:path';
import { test } from 'node:test';
import { vercelEnvPath } from '../src/setup.js';
import { startFake } from './fake-github.js';
import { assertNoSecrets, fakeKeychain, run, sandbox, secretsOf } from './helpers.js';

const unescape = (s) => s.replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');

/** Starts `workshop setup`, plays the browser and GitHub's two clicks, and returns the command's result. */
async function driveSetup({ fake, box, keychain, args = [], tamper }) {
  let opened;
  const opening = new Promise((resolve) => (opened = resolve));
  const running = run(['setup', ...args], { fake, box, keychain, overrides: { openBrowser: (url) => opened(url) } });
  const local = await Promise.race([opening, running.then((r) => ({ finished: r }))]);
  if (local.finished) return { result: local.finished };
  const html = await (await fetch(local)).text();
  const action = unescape(html.match(/action="([^"]+)"/)[1]);
  const manifestValue = unescape(html.match(/name="manifest" value="([^"]+)"/)[1]);
  const posted = await fetch(action, {
    method: 'POST',
    redirect: 'manual',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ manifest: manifestValue }),
  });
  assert.equal(posted.status, 302);
  let back = posted.headers.get('location');
  if (tamper) back = await tamper(back);
  const callback = await fetch(back);
  const page = await callback.text();
  return { result: await running, local, action, manifest: JSON.parse(manifestValue), page, callbackStatus: callback.status };
}

test('setup registers the App from a manifest, stores the credentials and writes vercel.env 0600', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t, { features: '', decisions: '' });
  const keychain = fakeKeychain();
  const { result, local, action, manifest, page } = await driveSetup({ fake, box, keychain, args: ['--org', 'amontlabs'] });
  assert.equal(result.code, 0, result.stderr);
  assert.match(local, /^http:\/\/127\.0\.0\.1:\d+\/$/);
  assert.match(action, new RegExp(`^${fake.url}/organizations/amontlabs/settings/apps/new\\?state=[0-9a-f]{32}$`));

  assert.deepEqual(manifest, {
    name: 'Silo Workshop',
    url: 'https://silo.amontlabs.com/workshop',
    description: manifest.description,
    redirect_url: `${new URL(local).origin}/callback`,
    callback_urls: ['https://silo.amontlabs.com/api/workshop/callback'],
    public: false,
    default_permissions: { issues: 'write', metadata: 'read' },
    default_events: [],
    request_oauth_on_install: false,
  });
  assert.ok(!('hook_attributes' in manifest), 'no webhook');
  assert.equal(fake.manifests[0].org, 'amontlabs');

  assert.deepEqual([...keychain.store.keys()].sort(), ['WORKSHOP_APP_ID', 'WORKSHOP_CLIENT_ID', 'WORKSHOP_CLIENT_SECRET', 'WORKSHOP_PRIVATE_KEY']);
  assert.equal(keychain.store.get('WORKSHOP_APP_ID'), fake.appId);
  assert.equal(keychain.store.get('WORKSHOP_CLIENT_SECRET'), fake.createdApp.client_secret);
  assert.equal(keychain.store.get('WORKSHOP_PRIVATE_KEY'), fake.createdApp.pem);

  const envPath = vercelEnvPath(box.home);
  assert.equal(statSync(envPath).mode & 0o777, 0o600);
  assert.equal(statSync(dirname(envPath)).mode & 0o777, 0o700);
  const env = Object.fromEntries(readFileSync(envPath, 'utf8').trim().split('\n').map((line) => {
    const [, name, value] = line.match(/^([A-Z_]+)=(.*)$/);
    return [name, JSON.parse(value)];
  }));
  assert.deepEqual(Object.keys(env).sort(), [
    'WORKSHOP_APP_ID', 'WORKSHOP_CLIENT_ID', 'WORKSHOP_CLIENT_SECRET', 'WORKSHOP_PRIVATE_KEY', 'WORKSHOP_REPO', 'WORKSHOP_SESSION_SECRET', 'WORKSHOP_SITE_ORIGIN',
  ]);
  assert.equal(env.WORKSHOP_PRIVATE_KEY, fake.createdApp.pem);
  assert.equal(Buffer.from(env.WORKSHOP_SESSION_SECRET, 'base64').length, 32);
  assert.equal(env.WORKSHOP_REPO, 'amontlabs/silo');
  assert.equal(env.WORKSHOP_SITE_ORIGIN, 'https://silo.amontlabs.com');

  assert.match(result.stdout, new RegExp(`${fake.url}/apps/silo-workshop/installations/new`));
  assert.match(page, /installations\/new/);
  const secrets = [...secretsOf(fake), env.WORKSHOP_SESSION_SECRET, env.WORKSHOP_CLIENT_SECRET];
  assertNoSecrets(result.stdout + result.stderr + page, secrets);
});

test('setup refuses to overwrite existing credentials without --replace, and replaces with it', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t, { features: '', decisions: '' });
  const keychain = fakeKeychain({ WORKSHOP_CLIENT_SECRET: 'old-secret' });
  const refused = await driveSetup({ fake, box, keychain });
  assert.equal(refused.result.code, 1);
  assert.match(refused.result.stderr, /already exist.*--replace/);
  assert.equal(keychain.store.get('WORKSHOP_CLIENT_SECRET'), 'old-secret');
  assert.equal(fake.manifests.length, 0);
  assert.ok(!refused.result.stderr.includes('old-secret'));

  const replaced = await driveSetup({ fake, box, keychain, args: ['--replace', '--json'] });
  assert.equal(replaced.result.code, 0, replaced.result.stderr);
  assert.notEqual(keychain.store.get('WORKSHOP_CLIENT_SECRET'), 'old-secret');
  assert.equal(JSON.parse(replaced.result.stdout).app, 'silo-workshop');

  const again = await driveSetup({ fake, box, keychain: fakeKeychain() });
  assert.equal(again.result.code, 1, 'an existing vercel.env also counts');
});

test('a callback with the wrong state is refused and the code is not exchanged', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t, { features: '', decisions: '' });
  const keychain = fakeKeychain();
  let wrongStatus;
  const { result, callbackStatus } = await driveSetup({
    fake,
    box,
    keychain,
    tamper: async (location) => {
      const wrong = new URL(location);
      wrong.searchParams.set('state', 'f'.repeat(32));
      wrongStatus = (await fetch(wrong)).status;
      return location;
    },
  });
  assert.equal(wrongStatus, 400);
  assert.equal(callbackStatus, 200);
  assert.equal(result.code, 0);
  assert.equal(fake.log.filter((r) => r.path.includes('/conversions')).length, 1);
});

test('setup rejects a site that is not https', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t, { features: '', decisions: '' });
  const result = await run(['setup', '--site', 'http://example.com'], { fake, box, keychain: fakeKeychain() });
  assert.equal(result.code, 2);
});
