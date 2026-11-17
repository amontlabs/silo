import assert from 'node:assert/strict';
import { chmodSync, readFileSync, statSync } from 'node:fs';
import { request } from 'node:http';
import { dirname } from 'node:path';
import { test } from 'node:test';
import { Failure } from '../github/errors.js';
import { vercelEnvPath } from '../github/setup.js';
import { startFake } from './fake-github.js';
import { assertNoSecrets, fakeKeychain, run, sandbox, secretsOf } from './setup-helpers.js';

const unescape = (s) => s.replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');

/** Starts `plan setup`, plays the browser and GitHub's two clicks, and returns the command's result. */
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
  const box = sandbox(t);
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
    public: true,
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
  const box = sandbox(t);
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
  const box = sandbox(t);
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
  const box = sandbox(t);
  const result = await run(['setup', '--site', 'http://example.com'], { fake, box, keychain: fakeKeychain() });
  assert.equal(result.code, 2);
});

function lockedKeychain() {
  return {
    available: () => true,
    get: () => {
      throw new Failure('the Keychain is locked or unavailable');
    },
    set: () => {
      throw new Failure('the Keychain is locked or unavailable');
    },
  };
}

test('a locked Keychain stops setup and the other commands; it never reads as "no credentials"', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  for (const argv of [['setup'], ['setup', '--replace'], ['doctor'], ['store']]) {
    const result = await run(argv, { fake, box, keychain: lockedKeychain() });
    assert.equal(result.code, 1, argv.join(' '));
    assert.match(result.stderr, /the Keychain is locked or unavailable/);
    assert.doesNotMatch(result.stderr, /missing/);
  }
  assert.equal(fake.manifests.length, 0);
});

/** A Keychain that fails every store after the first `okStores`, until `heal()`. */
function flakyKeychain(okStores) {
  const keychain = fakeKeychain();
  let left = okStores;
  return {
    keychain,
    heal: () => (left = Infinity),
    available: () => true,
    get: (name) => keychain.get(name),
    set: (name, value) => {
      if (left <= 0) throw new Failure(`the Keychain is locked or unavailable: could not store ${name}`);
      left -= 1;
      keychain.set(name, value);
    },
  };
}

for (const okStores of [0, 2]) {
  test(`a Keychain failure after ${okStores} of the four values keeps everything in vercel.env; store finishes and no second App is made`, async (t) => {
    const fake = await startFake();
    t.after(fake.stop);
    const box = sandbox(t);
    const flaky = flakyKeychain(okStores);
    const first = await driveSetup({ fake, box, keychain: flaky });
    assert.equal(first.result.code, 1);
    assert.match(first.result.stderr, /the App silo-workshop exists on GitHub and its credentials are in .*vercel\.env.*run `plan store`/);
    assert.equal(flaky.keychain.store.size, okStores);
    const envPath = vercelEnvPath(box.home);
    assert.equal(statSync(envPath).mode & 0o777, 0o600);
    assert.match(readFileSync(envPath, 'utf8'), /WORKSHOP_PRIVATE_KEY=/);
    assertNoSecrets(first.result.stderr + first.page, [...secretsOf(fake)]);

    flaky.heal();
    for (const args of [[], ['--replace']]) {
      const retry = await driveSetup({ fake, box, keychain: flaky, args });
      assert.equal(retry.result.code, 1);
      assert.match(retry.result.stderr, /run `plan store` to finish/);
    }
    assert.equal(fake.manifests.length, 1, 'never a second App');

    const stored = await run(['store', '--json'], { fake, box, keychain: flaky });
    assert.equal(stored.code, 0, stored.stderr);
    assert.equal(JSON.parse(stored.stdout).stored.length, 4 - okStores);
    assert.equal(flaky.keychain.store.get('WORKSHOP_PRIVATE_KEY'), fake.createdApp.pem);
    assert.equal(flaky.keychain.store.get('WORKSHOP_CLIENT_SECRET'), fake.createdApp.client_secret);
    assert.ok(!flaky.keychain.store.has('WORKSHOP_SESSION_SECRET'));
    const again = await run(['store'], { fake, box, keychain: flaky });
    assert.equal(again.code, 0);
    assert.match(again.stdout, /already holds/);

    // Now complete and matching: setup refuses without --replace and registers a new App with it.
    const plain = await driveSetup({ fake, box, keychain: flaky });
    assert.match(plain.result.stderr, /already exist in the Keychain; pass --replace/);
    const replaced = await driveSetup({ fake, box, keychain: flaky, args: ['--replace'] });
    assert.equal(replaced.result.code, 0, replaced.result.stderr);
    assert.equal(fake.manifests.length, 2);
  });
}

test('a Keychain holding another App than vercel.env: setup --replace refuses and says what to do; store needs --replace', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const keychain = fakeKeychain();
  assert.equal((await driveSetup({ fake, box, keychain })).result.code, 0);
  const other = { WORKSHOP_APP_ID: '999', WORKSHOP_CLIENT_ID: 'c', WORKSHOP_CLIENT_SECRET: 's', WORKSHOP_PRIVATE_KEY: 'k' };
  for (const [name, value] of Object.entries(other)) keychain.set(name, value);
  const refused = await driveSetup({ fake, box, keychain, args: ['--replace'] });
  assert.equal(refused.result.code, 1);
  assert.match(refused.result.stderr, /holds App 4242 but the Keychain holds App 999: run `plan store --replace` .* or delete .*vercel\.env/);
  assert.equal(fake.manifests.length, 1);
  const storeRefused = await run(['store'], { fake, box, keychain });
  assert.equal(storeRefused.code, 1);
  assert.match(storeRefused.stderr, /App 999.*pass --replace/);
  assert.equal((await run(['store', '--replace'], { fake, box, keychain })).code, 0);
  assert.equal(keychain.get('WORKSHOP_APP_ID'), fake.appId);

  // A partial set of another App (its id among them) is overwritten without --replace.
  keychain.store.clear();
  keychain.set('WORKSHOP_APP_ID', '999');
  const partial = await driveSetup({ fake, box, keychain, args: ['--replace'] });
  assert.match(partial.result.stderr, /run `plan store` to finish/);
  assert.equal((await run(['store'], { fake, box, keychain })).code, 0);
  assert.equal(keychain.get('WORKSHOP_APP_ID'), fake.appId);
  assert.equal(keychain.store.size, 4);
});

test('store refuses a vercel.env readable by others', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const keychain = fakeKeychain();
  assert.equal((await driveSetup({ fake, box, keychain })).result.code, 0);
  chmodSync(vercelEnvPath(box.home), 0o644);
  const loose = await run(['store', '--replace'], { fake, box, keychain });
  assert.equal(loose.code, 1);
  assert.match(loose.stderr, /chmod 600/);
});

test('the setup server refuses any Host but 127.0.0.1 or localhost on its port', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  let opened;
  const opening = new Promise((resolve) => (opened = resolve));
  const running = run(['setup'], { fake, box, keychain: fakeKeychain(), overrides: { openBrowser: (url) => opened(url), setupTimeoutMs: 2000 } });
  const local = new URL(await opening);
  const ask = (host, path = '/') =>
    new Promise((resolve, reject) => {
      const req = request({ host: '127.0.0.1', port: local.port, path, headers: { host } }, (res) => {
        res.resume();
        resolve(res.statusCode);
      });
      req.on('error', reject);
      req.end();
    });
  assert.equal(await ask(`evil.example:${local.port}`), 421);
  assert.equal(await ask(`127.0.0.1:${Number(local.port) + 1}`), 421);
  assert.equal(await ask('127.0.0.1'), 421);
  assert.equal(await ask(`evil.example:${local.port}`, '/callback?code=x&state=y'), 421);
  assert.equal(await ask(`localhost:${local.port}`), 200);
  assert.equal(await ask(`127.0.0.1:${local.port}`), 200);
  const result = await running;
  assert.equal(result.code, 1);
  assert.match(result.stderr, /timed out/);
});

test('doctor: sources only, the App public and installed, its permissions, the six labels', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const missing = await run(['doctor', '--json'], { fake, box });
  assert.equal(missing.code, 1);
  const report = JSON.parse(missing.stdout);
  assert.deepEqual(report.credentials, { WORKSHOP_APP_ID: 'keychain', WORKSHOP_CLIENT_ID: 'missing', WORKSHOP_CLIENT_SECRET: 'missing', WORKSHOP_PRIVATE_KEY: 'keychain' });
  assert.equal(report.public, true);
  assert.equal(report.installed, true);
  assert.deepEqual(Object.keys(report.labels), ['workshop:entry', 'workshop:suggestion', 'verdict:take', 'verdict:later', 'verdict:drop', 'workshop:building']);
  assert.match(report.problems.join('\n'), /labels missing \(plan sync creates them\): workshop:entry, .*workshop:building/);
  for (const name of Object.keys(report.labels)) fake.labels.push({ name });
  const ok = await run(['doctor'], { fake, box });
  assert.equal(ok.code, 0, ok.stdout);
  assert.match(ok.stdout, /Label workshop:building: yes/);
  fake.public = false;
  assert.match((await run(['doctor'], { fake, box })).stdout, /Public: no/);
});
