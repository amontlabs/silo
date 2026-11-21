import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createPublicKey, createPrivateKey, generateKeyPairSync, randomBytes, verify } from 'node:crypto';
import { cp, lstat, mkdir, mkdtemp, readFile, readlink, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { connect as netConnect, createServer } from 'node:net';
import { createRequire } from 'node:module';
import { get as httpGet } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createAppJwt } from '../server/app-auth.mjs';
import { readConfig } from '../server/config.mjs';
import { createGitHub, DEFAULT_TIMEOUT, GitHubError } from '../server/github.mjs';
import { createNodeHandler, MAX_BODY, readRequest, waitUntil } from '../server/node.mjs';
import { seal, unseal } from '../server/session.mjs';
import { BUDGET, createWorkshop, LABELS } from '../server/workshop.mjs';
import { CUTS, cut, readBody } from '../server/issues.mjs';
import { SNAPSHOT } from '../server/snapshot.mjs';
import { createFakeGitHub, FAKE_APP, seedDemo } from './fake-github.mjs';
import { DEFAULT_OUTPUT, ENDPOINTS, HANDLER, prepareVercel, RUNTIME } from './prepare-vercel.mjs';
import { loadEntries, SEEDED_ENTRIES, startDev } from './workshop-dev.mjs';
import { readEntryMarker, withoutMarkers } from '../../tools/workshop/model/index.js';
import { planEntries } from './plan-entries.mjs';

import { count, retiredNumber, entryNumber, FAILED, issueOf, ORIGIN, PLAN_ENTRIES, SESSION, setup, START, withBudget } from './workshop-test-support.mjs';

// The two tests that hold a call at the fake GitHub end it themselves. This limit decides nothing while they pass:
// it only turns a test left waiting on a call that never comes into a failure, where it would hang the whole run.
const HELD_LIMIT_MS = 20_000;

test('App JWT is RS256 with the App ID, and the installation token is cached until shortly before it expires', async () => {
  const t = await setup();
  try {
    const jwt = createAppJwt({ appId: FAKE_APP.appId, privateKey: t.fake.privateKeyPem, nowMs: START });
    const [header, payload, signature] = jwt.split('.');
    assert.deepEqual(JSON.parse(Buffer.from(header, 'base64url')), { alg: 'RS256', typ: 'JWT' });
    const claims = JSON.parse(Buffer.from(payload, 'base64url'));
    assert.equal(claims.iss, FAKE_APP.appId);
    assert.ok(claims.exp - claims.iat <= 600);
    const publicKey = createPublicKey(createPrivateKey(t.fake.privateKeyPem));
    assert.ok(verify('sha256', Buffer.from(`${header}.${payload}`), publicKey, Buffer.from(signature, 'base64url')));

    const c = t.client();
    const read = async () => { await c.call('GET', 'state'); await t.workshop.idle(); };
    await read();
    t.clock.t += 61_000;
    await read();
    t.clock.t += 50 * 60_000;
    await read();
    assert.equal(count(t.fake, 'POST', /access_tokens$/), 1);
    t.clock.t += 6 * 60_000;
    await read();
    assert.equal(count(t.fake, 'POST', /access_tokens$/), 2);
  } finally { await t.close(); }
});

test('secondary rate limits (retry-after, or only the message) are rate limits and block further calls', async () => {
  for (const kind of ['retry-after', 'message']) {
    const t = await setup();
    try {
      const c = t.client();
      t.fake.state.secondaryLimited = kind;
      assert.equal((await c.call('GET', 'state')).status, 502, `a cold snapshot read, limited by ${kind}`);
      const calls = t.fake.state.requests.length;
      t.fake.state.secondaryLimited = false;
      assert.equal((await c.call('GET', 'state')).status, 502);
      assert.equal(t.fake.state.requests.length, calls, `blocked after a ${kind} secondary limit`);
    } finally { await t.close(); }
  }
});

test('GitHub failing answers 502 when nothing is cached, and an unconfigured server answers 503', async () => {
  const t = await setup();
  try {
    t.fake.state.failNext = 10;
    const failed = await t.client().call('GET', 'state');
    assert.equal(failed.status, 502);
    assert.deepEqual(Object.keys(failed.json).sort(), ['error', 'message']);
  } finally { await t.close(); }
  const u = await setup({ configured: false });
  try {
    for (const path of ['state', 'me', 'issue?n=1', 'vote?n=1']) {
      const response = await u.client().call('GET', path);
      assert.equal(response.status, 503);
      assert.equal(response.json.error, 'not_configured');
    }
    const login = await u.client().call('GET', 'login');
    assert.equal(login.status, 302);
    assert.equal(login.headers.Location, FAILED);
  } finally { await u.close(); }
});

test('a hung or failing token refresh answers 502 and keeps the session; only an invalid refresh token signs out', { timeout: HELD_LIMIT_MS }, async () => {
  const t = await setup({ cuttable: (path, body) => path === '/login/oauth/access_token' && body.includes('refresh_token') });
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const cookie = c.jar.get(SESSION);
    t.clock.t += 7 * 3600_000 + 56 * 60_000;
    // The refresh reaches GitHub and is held there until the test cuts it: nothing but the test ends it.
    let reached;
    const arrived = new Promise((resolve) => { reached = resolve; });
    t.fake.state.hook = (r) => {
      if (r.path !== '/login/oauth/access_token') return null;
      reached();
      return { until: gate };
    };
    let settled = false;
    const pending = c.call('GET', 'vote?n=1').then((response) => { settled = true; return response; });
    // Whichever comes first: the refresh reaching GitHub, or the vote ending without one, which is a failure.
    const first = await Promise.race([arrived.then(() => 'refresh'), pending.then((response) => `the vote settled with ${response.status}`)]);
    assert.equal(first, 'refresh', `the refresh did not reach GitHub: ${first}`);
    const other = await t.client().call('GET', 'state');
    assert.equal(other.status, 200);
    assert.equal(settled, false, 'other requests are not held by the refresh');
    t.cut();
    const hung = await pending;
    assert.equal(hung.status, 502);
    assert.equal(c.jar.get(SESSION), cookie, 'the cookie is kept');
    assert.ok(c.jar.has('workshop-hint'));

    t.fake.state.hook = (r) => (r.path === '/login/oauth/access_token' ? { status: 503 } : null);
    t.clock.t += 31_000;
    assert.equal((await c.call('GET', 'vote?n=1')).status, 502);
    assert.equal(c.jar.get(SESSION), cookie);

    t.fake.state.hook = null;
    t.clock.t += 31_000;
    const refreshed = await c.call('GET', 'vote?n=1');
    assert.equal(refreshed.status, 200);
    assert.notEqual(c.jar.get(SESSION), cookie);
    assert.equal((await c.call('POST', 'comment', { body: { n: 1, text: 'After refresh.' } })).status, 200);

    t.fake.state.refresh.clear();
    t.clock.t += 8 * 3600_000;
    const invalid = await c.call('GET', 'vote?n=1');
    assert.equal(invalid.status, 401);
    assert.equal(c.jar.has(SESSION), false);
    assert.equal(c.jar.has('workshop-hint'), false);
  } finally { t.fake.state.hook = null; release(); await t.close(); }
});

test('a call GitHub does not answer within the limit fails as a timeout', { timeout: HELD_LIMIT_MS }, async () => {
  const t = await setup();
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const realTimeout = AbortSignal.timeout;
  try {
    await t.client().signIn('visitor-fake');
    const user = [...t.fake.state.tokens].find(([, entry]) => entry.kind === 'user')[0];
    // The limit is an `AbortSignal.timeout(ms)`; here the test holds that signal and fires it itself, so no clock decides.
    const limits = [];
    AbortSignal.timeout = (ms) => {
      const abort = new AbortController();
      limits.push({ ms, abort });
      return abort.signal;
    };
    let reached;
    const arrived = new Promise((resolve) => { reached = resolve; });
    t.fake.state.hook = (r) => {
      if (r.path !== '/user') return null;
      reached();
      return { until: gate };
    };
    const github = createGitHub({ apiBase: t.fake.url, timeoutMs: 100 });
    const call = github.request('GET', '/user', { token: user, as: 'user' }).then(() => 'answered', (error) => error);
    // The limit is fired only once GitHub holds the request, or the timeout could be reported for a call never made.
    const first = await Promise.race([arrived.then(() => 'held'), call.then((outcome) => `ended first: ${outcome instanceof Error ? outcome.message : outcome}`)]);
    assert.equal(first, 'held');
    assert.deepEqual(limits.map((limit) => limit.ms), [100], 'the call carries the limit it was given');
    limits[0].abort.abort(new DOMException('The operation timed out.', 'TimeoutError'));
    const error = await call;
    assert.ok(error instanceof GitHubError && error.timeout === true && error.network === true, String(error));
  } finally { AbortSignal.timeout = realTimeout; t.fake.state.hook = null; release(); await t.close(); }
});

test('a 401 on the installation token is a 502 and drops the token; a 401 on the person\'s token signs them out', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    t.fake.state.hook = (r) => (r.kind === 'installation' && r.path === '/repos/amontlabs/silo/issues' ? { status: 401 } : null);
    assert.equal((await t.client().call('GET', 'state')).status, 502, 'the installation token is refused: no state');
    const tokens = count(t.fake, 'POST', /access_tokens$/);
    assert.ok(c.jar.has(SESSION), 'still signed in');
    t.fake.state.hook = null;
    t.clock.t += 61_000;
    assert.equal((await t.client().call('GET', 'state')).status, 200);
    assert.equal(count(t.fake, 'POST', /access_tokens$/), tokens + 1, 'a new installation token');

    t.fake.state.hook = (r) => (r.kind === 'user' && r.method === 'POST' ? { status: 401 } : null);
    const comment = await c.call('POST', 'comment', { body: { n: 1, text: 'x' } });
    assert.equal(comment.status, 401);
    assert.equal(comment.json.error, 'signed_out');
    assert.equal(c.jar.has(SESSION), false);
    assert.equal(c.jar.has('workshop-hint'), false);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('me needs a session and answers the login and the avatar', async () => {
  const t = await setup();
  try {
    const anonymous = await t.client().call('GET', 'me');
    assert.equal(anonymous.status, 401);
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const me = await visitor.call('GET', 'me');
    assert.deepEqual(me.json, { login: 'visitor-fake', avatar: 'https://avatars.example.test/visitor-fake.png' });
    assert.equal(me.headers['Cache-Control'], 'private, no-store');
  } finally { await t.close(); }
});

test('GET vote reads the person\'s own reaction with their token, privately', async () => {
  const t = await setup();
  try {
    assert.equal((await t.client().call('GET', 'vote?n=1')).status, 401);
    const c = t.client();
    await c.signIn('visitor-fake');
    const on = await c.call('GET', 'vote?n=1');
    assert.deepEqual(on.json, { on: true, comments: [] });
    assert.equal(on.headers['Cache-Control'], 'private, no-store');
    assert.deepEqual((await c.call('GET', 'vote?n=2')).json, { on: false, comments: [] });
    assert.equal((await c.call('GET', 'vote?n=7')).status, 404);
    assert.equal(count(t.fake, 'GET', /reactions$/, 'installation'), 0);
  } finally { await t.close(); }
});

test('a signed-in person spends the installation token only on labelling a suggestion', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const before = t.fake.state.requests.length;
    await visitor.call('GET', 'vote?n=1');
    await visitor.call('POST', 'comment', { body: { n: 1, text: 'hi' } });
    await visitor.call('POST', 'vote', { body: { n: 1, on: false } });
    await visitor.call('POST', 'vote', { body: { n: 1, on: true } });
    await visitor.call('GET', 'me');
    await visitor.call('POST', 'suggest', { body: { title: 'T', text: 'x' } });
    await owner.call('GET', 'me');
    await t.workshop.idle();
    const refreshes = /^\/repos\/amontlabs\/silo\/issues(\/comments)?$/;
    const installation = t.fake.state.requests.slice(before).filter((r) => r.kind === 'installation' && !(r.method === 'GET' && refreshes.test(r.path)));
    assert.deepEqual(installation.map((r) => r.path.replace(/\/\d+\//, '/N/')).sort(), [
      '/repos/amontlabs/silo/issues/N/labels',
    ].sort());
  } finally { await t.close(); }
});

test('at most 30 writes a minute per person', async () => {
  const t = await setup();
  try {
    await withBudget({ writesPerMinute: 3 }, async () => {
      const c = t.client();
      await c.signIn('visitor-fake');
      for (let i = 0; i < 3; i += 1) assert.equal((await c.call('POST', 'comment', { body: { n: 1, text: `c${i}` } })).status, 200);
      const limited = await c.call('POST', 'comment', { body: { n: 1, text: 'c3' } });
      assert.equal(limited.status, 429);
      assert.equal(limited.json.error, 'rate_limited');
      const other = t.client();
      await other.signIn('owner-fake');
      assert.equal((await other.call('POST', 'comment', { body: { n: 1, text: 'o' } })).status, 200, 'per person');
      t.clock.t += 60_000;
      assert.equal((await c.call('POST', 'comment', { body: { n: 1, text: 'later' } })).status, 200);
    });
    assert.equal(BUDGET.writesPerMinute, 20);
  } finally { await t.close(); }
});

test('comment, vote and suggest write as the signed-in person', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const comment = await c.call('POST', 'comment', { body: { n: 2, text: '  Yes \u202Eplease.\u2066  ' } });
    assert.equal(comment.status, 200);
    const posted = issueOf(t.fake, 2).comments.at(-1);
    assert.equal(posted.user.login, 'visitor-fake');
    assert.equal(posted.body, 'Yes please.', 'direction controls removed');

    assert.equal((await c.call('POST', 'vote', { body: { n: 2, on: true } })).status, 200);
    assert.deepEqual(issueOf(t.fake, 2).reactions.map((r) => r.user.login).sort(), ['owner-fake', 'visitor-fake']);
    assert.deepEqual((await c.call('GET', 'vote?n=2')).json, { on: true, comments: [] });
    assert.equal((await c.call('POST', 'vote', { body: { n: 2, on: false } })).status, 200);
    assert.deepEqual(issueOf(t.fake, 2).reactions.map((r) => r.user.login), ['owner-fake'], 'only their own reaction removed');

    const suggest = await c.call('POST', 'suggest', { body: { title: '\u202ERun on FreeBSD', text: 'As a host.' } });
    assert.equal(suggest.status, 200);
    const created = issueOf(t.fake, suggest.json.number);
    assert.equal(created.title, 'Run on FreeBSD');
    assert.equal(created.user.login, 'visitor-fake');
    assert.deepEqual(created.labels, [LABELS.suggestion]);
    const labelCall = t.fake.state.requests.findLast((r) => r.path.endsWith(`/issues/${suggest.json.number}/labels`));
    assert.equal(labelCall.kind, 'installation');
    const state = await c.call('GET', 'state');
    assert.equal(state.json.issues[suggest.json.number].kind, 'suggestion', 'a write refreshes this instance\'s cache');

    issueOf(t.fake, 3).locked = true;
    const locked = await c.call('POST', 'comment', { body: { n: 3, text: 'x' } });
    assert.equal(locked.status, 403);
    assert.equal(locked.json.error, 'forbidden');
  } finally { await t.close(); }
});

test('a suggestion whose label fails twice answers 502 with the issue number and creates nothing else', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const issues = t.fake.state.issues.size;
    t.fake.state.hook = (r) => (r.kind === 'installation' && r.path.endsWith('/labels') ? { status: 500 } : null);
    const response = await c.call('POST', 'suggest', { body: { title: 'Stray', text: 'x' } });
    assert.equal(response.status, 502);
    assert.equal(response.json.error, 'github');
    assert.equal(typeof response.json.number, 'number');
    assert.equal(t.fake.state.issues.size, issues + 1);
    assert.deepEqual(issueOf(t.fake, response.json.number).labels, []);
    assert.equal(count(t.fake, 'POST', /\/labels$/, 'installation'), 2, 'one retry');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('an issue with only the retired decision label is not a Workshop issue', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('owner-fake');
    const state = (await c.call('GET', 'state')).json.issues;
    for (const n of [retiredNumber, retiredNumber + 1]) {
      assert.equal(Object.hasOwn(state, n), false, `#${n} is not in the state`);
      assert.equal((await c.call('GET', `issue?n=${n}`)).status, 404);
      assert.equal((await c.call('POST', 'comment', { body: { n, text: 'x' } })).status, 404);
      assert.equal((await c.call('GET', `vote?n=${n}`)).status, 404);
      assert.equal((await c.call('POST', 'vote', { body: { n, on: true } })).status, 404);
    }
    assert.equal(issueOf(t.fake, retiredNumber).comments.length, 1, 'nothing was written');
    // Beside one kind label it is ignored: the issue is an entry.
    const entry = t.fake.addIssue({ title: 'Entry with the old label', labels: ['workshop:entry', 'workshop:decision'] });
    t.clock.t += 61_000;
    await c.call('GET', 'state');
    await t.workshop.idle();
    assert.equal((await c.call('GET', 'state')).json.issues[entry.number].kind, 'entry');
  } finally { await t.close(); }
});

test('there is no decide or verdict endpoint: the owner\'s verdict lives in the plan\'s files only', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('owner-fake');
    assert.equal((await c.call('GET', 'me')).json.login, 'owner-fake');
    const before = JSON.stringify([...t.fake.state.issues.values()]);
    const requests = t.fake.state.requests.length;
    const post = await c.call('POST', 'decide', { body: { n: retiredNumber, text: 'Debian.' } });
    assert.deepEqual([post.status, post.json.error], [404, 'not_found']);
    assert.equal((await c.call('GET', 'decide')).status, 404);
    for (const verdict of ['take', 'later', 'drop', null]) {
      const response = await c.call('POST', 'verdict', { body: { n: entryNumber, verdict } });
      assert.deepEqual([response.status, response.json.error], [404, 'not_found']);
    }
    assert.equal((await c.call('GET', 'verdict')).status, 404);
    assert.equal(JSON.stringify([...t.fake.state.issues.values()]), before);
    assert.equal(t.fake.state.requests.slice(requests).filter((r) => r.method !== 'GET').length, 0, 'nothing was written on GitHub');
    assert.equal(ENDPOINTS.includes('decide'), false);
    assert.equal(ENDPOINTS.includes('verdict'), false);
  } finally { await t.close(); }
});

test('every refusal in the contract', async () => {
  const t = await setup();
  try {
    const anonymous = t.client();
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const owner = t.client();
    await owner.signIn('owner-fake');
    const before = JSON.stringify([...t.fake.state.issues.values()]);
    const cases = [
      [visitor, 'comment', { n: 1, text: 'x' }, { origin: 'https://evil.test' }, 403, 'bad_origin'],
      [visitor, 'comment', { n: 1, text: 'x' }, { origin: undefined }, 403, 'bad_origin'],
      [visitor, 'comment', { n: 1, text: 'x' }, { 'x-workshop': undefined }, 403, 'bad_request_header'],
      [visitor, 'logout', {}, { origin: 'https://evil.test' }, 403, 'bad_origin'],
      [anonymous, 'comment', { n: 1, text: 'x' }, {}, 401, 'signed_out'],
      [anonymous, 'vote', { n: 1, on: true }, {}, 401, 'signed_out'],
      [anonymous, 'suggest', { title: 'x', text: 'y' }, {}, 401, 'signed_out'],
      [visitor, 'comment', { n: 4, text: 'x' }, {}, 404, 'not_found'],
      [visitor, 'vote', { n: 5, on: true }, {}, 404, 'not_found'],
      [visitor, 'comment', { n: 7, text: 'x' }, {}, 404, 'not_found'],
      [visitor, 'vote', { n: 7, on: true }, {}, 404, 'not_found'],
      [visitor, 'comment', { n: 8, text: 'x' }, {}, 404, 'not_found'],
      [owner, 'decide', { n: 4, text: 'x' }, {}, 404, 'not_found'],
      [owner, 'verdict', { n: 1, verdict: 'take' }, {}, 404, 'not_found'],
      [visitor, 'comment', { n: 1, text: 'x'.repeat(5001) }, {}, 400, 'too_long'],
      [visitor, 'comment', { n: 1, text: '   ' }, {}, 400, 'empty'],
      [visitor, 'comment', { n: '1', text: 'x' }, {}, 400, 'bad_input'],
      [visitor, 'vote', { n: 1, on: 'yes' }, {}, 400, 'bad_input'],
      [visitor, 'suggest', { title: 'x'.repeat(121), text: 'y' }, {}, 400, 'too_long'],
      [visitor, 'suggest', { title: '', text: 'y' }, {}, 400, 'empty'],
      [visitor, 'suggest', { title: 'two\nlines', text: 'y' }, {}, 400, 'bad_input'],
      [visitor, 'suggest', { title: 'bell\u0007', text: 'y' }, {}, 400, 'bad_input'],
      [visitor, 'suggest', { title: 'separator\u2028', text: 'y' }, {}, 400, 'bad_input'],
    ];
    for (const [who, path, body, headers, status, code] of cases) {
      const response = await who.call('POST', path, { body, headers });
      assert.equal(response.status, status, `${path} ${JSON.stringify(body)} ${JSON.stringify(headers)} ${response.body}`);
      assert.equal(response.json.error, code, `${path} ${JSON.stringify(body)}`);
      assert.equal(typeof response.json.message, 'string');
    }
    assert.equal(JSON.stringify([...t.fake.state.issues.values()]), before, 'no refusal changed anything');
    assert.equal((await visitor.call('POST', 'comment', { body: '{not json' })).status, 400);
    assert.equal((await visitor.call('GET', 'comment')).status, 405);
    assert.equal((await visitor.call('GET', 'nothing')).status, 404);
    for (const back of ['//evil.test', 'https://evil.test/workshop', '/workshop/../api/workshop/me', '/workshop/%2e%2e/api', '/workshopx', '/workshop\\evil', '/\\evil.test', 'workshop', '/workshop//evil.test', '/api/workshop/me']) {
      const response = await anonymous.call('GET', `login?return=${encodeURIComponent(back)}`);
      assert.equal(response.status, 302, back);
      assert.equal(response.headers.Location, FAILED, back);
      assert.deepEqual(response.cookies, [], back);
    }
    const ok = await anonymous.call('GET', 'login?return=' + encodeURIComponent('/workshop/#entry-3'));
    assert.equal(ok.status, 302);
    assert.notEqual(ok.headers.Location, FAILED);
  } finally { await t.close(); }
});

test('session cookie: __Host- prefix, sealed round trip, tampering, purpose, expiry, and the readable hint', async () => {
  const secret = Buffer.alloc(32, 7);
  const sealed = seal(secret, 'session', { login: 'a', n: 1 });
  assert.deepEqual(unseal(secret, 'session', sealed), { login: 'a', n: 1 });
  assert.equal(unseal(secret, 'signin', sealed), null, 'a value sealed for one cookie does not open as another');
  assert.equal(unseal(Buffer.alloc(32, 8), 'session', sealed), null);
  const flipped = sealed.slice(0, 20) + (sealed[20] === 'A' ? 'B' : 'A') + sealed.slice(21);
  assert.equal(unseal(secret, 'session', flipped), null);
  assert.equal(unseal(secret, 'session', 'not a cookie!'), null);

  const t = await setup();
  try {
    const c = t.client();
    const done = await c.signIn('visitor-fake');
    const cookie = done.cookies.find((v) => v.startsWith(`${SESSION}=`));
    assert.match(cookie, /; Path=\/; Max-Age=2592000; HttpOnly; SameSite=Lax; Secure$/);
    assert.doesNotMatch(cookie, /Domain/);
    const hint = done.cookies.find((v) => v.startsWith('workshop-hint='));
    assert.equal(hint, 'workshop-hint=1; Path=/; Max-Age=2592000; SameSite=Lax; Secure');
    assert.ok(done.cookies.some((v) => v.startsWith('__Host-workshop-signin=;') && v.includes('Max-Age=0')));
    assert.equal(done.headers['Cache-Control'], 'private, no-store');
    const value = c.jar.get(SESSION);
    c.jar.set(SESSION, value.slice(0, -3) + (value.endsWith('AAA') ? 'BBB' : 'AAA'));
    const tampered = await c.call('GET', 'me');
    assert.equal(tampered.status, 401);
    assert.ok(tampered.cookies.some((v) => v.startsWith(`${SESSION}=;`) && v.includes('Max-Age=0')));
    assert.ok(tampered.cookies.some((v) => v.startsWith('workshop-hint=;') && v.includes('Max-Age=0')));

    const d = t.client();
    await d.signIn('visitor-fake');
    t.clock.t += 31 * 24 * 3600_000;
    assert.equal((await d.call('GET', 'me')).status, 401, 'expired after 30 days');
    assert.equal(d.jar.has('workshop-hint'), false);
  } finally { await t.close(); }
});

test('on a plain-http origin the cookies drop the prefix and Secure', async () => {
  const fake = createFakeGitHub();
  await fake.listen();
  seedDemo(fake);
  try {
    const workshop = createWorkshop({ config: readConfig(fake.workshopEnv('http://127.0.0.1:4174'), { apiBase: fake.url, webBase: fake.url }), log: () => {} });
    const response = await workshop.handle({ method: 'GET', url: '/api/workshop/login', headers: {}, body: '' });
    assert.match(response.cookies[0], /^workshop-signin=[^;]+; Path=\/; Max-Age=600; HttpOnly; SameSite=Lax$/);
  } finally { await fake.close(); }
});

test('sign-in failures redirect to /workshop?signin=failed and never show JSON', async () => {
  const t = await setup();
  try {
    const c = t.client();
    const start = await c.call('GET', 'login');
    assert.ok(start.cookies[0].startsWith('__Host-workshop-signin='));
    assert.match(start.cookies[0], /Path=\/; Max-Age=600; HttpOnly; SameSite=Lax; Secure/);
    const authorize = new URL(start.headers.Location);
    assert.equal(authorize.searchParams.get('code_challenge_method'), 'S256');
    assert.equal(authorize.searchParams.get('redirect_uri'), `${ORIGIN}/api/workshop/callback`);
    const wrong = await c.call('GET', 'callback?code=abc&state=nope');
    assert.equal(wrong.status, 302);
    assert.equal(wrong.headers.Location, FAILED);
    assert.equal(wrong.body, '');
    assert.equal(c.jar.size, 0, 'the state cookie is cleared after any callback');

    const missing = await t.client().call('GET', `callback?code=abc&state=${authorize.searchParams.get('state')}`);
    assert.equal(missing.headers.Location, FAILED);

    const d = t.client();
    const again = new URL((await d.call('GET', 'login')).headers.Location);
    t.clock.t += 11 * 60_000;
    assert.equal((await d.call('GET', `callback?code=abc&state=${again.searchParams.get('state')}`)).headers.Location, FAILED, 'expired');

    const e = t.client();
    const third = new URL((await e.call('GET', 'login')).headers.Location);
    const badCode = await e.call('GET', `callback?code=nope&state=${third.searchParams.get('state')}`);
    assert.equal(badCode.headers.Location, FAILED, 'a code GitHub refuses');

    const f = t.client();
    const fourth = new URL((await f.call('GET', 'login')).headers.Location);
    const denied = await f.call('GET', `callback?error=access_denied&state=${fourth.searchParams.get('state')}`);
    assert.equal(denied.status, 302);
    assert.equal(denied.headers.Location, '/workshop', 'cancelling is not a failure');
  } finally { await t.close(); }
});

test('logout revokes the whole grant (every session of that person) and clears the cookies', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const other = t.client();
    await other.signIn('visitor-fake');
    const out = await c.call('POST', 'logout', { body: {} });
    assert.equal(out.status, 200);
    assert.equal(c.jar.size, 0);
    assert.ok(out.cookies.some((v) => v.startsWith('workshop-hint=;')));
    assert.equal([...t.fake.state.tokens.values()].filter((e) => e.login === 'visitor-fake').length, 0);
    assert.equal((await c.call('GET', 'me')).status, 401);
    assert.equal((await other.call('GET', 'me')).status, 200, 'me reads only the cookie (a stated limit)');
    assert.equal((await other.call('POST', 'comment', { body: { n: 1, text: 'x' } })).status, 401, 'the other session is gone at GitHub');
    assert.equal(other.jar.has(SESSION), false);
  } finally { await t.close(); }
});

test('configuration: a weak secret or any invalid value answers 503 not_configured without naming it', async () => {
  const fake = createFakeGitHub();
  const base = fake.workshopEnv(ORIGIN);
  const cases = [
    ['WORKSHOP_SESSION_SECRET', 'c2hvcnQ='],
    ['WORKSHOP_SESSION_SECRET', randomBytes(32).toString('hex')],
    ['WORKSHOP_SESSION_SECRET', randomBytes(33).toString('base64')],
    ['WORKSHOP_SESSION_SECRET', Buffer.alloc(32, 0xfb).toString('base64url') + '='],
    ['WORKSHOP_REPO', 'not a repo'],
    ['WORKSHOP_REPO', 'a/b/c'],
    ['WORKSHOP_SITE_ORIGIN', 'not a url'],
    ['WORKSHOP_SITE_ORIGIN', 'https://silo.test/path'],
    ['WORKSHOP_SITE_ORIGIN', 'ftp://silo.test'],
    ['WORKSHOP_PRIVATE_KEY', '-----BEGIN RSA PRIVATE KEY-----\nnope\n-----END RSA PRIVATE KEY-----'],
    ['WORKSHOP_PRIVATE_KEY', generateKeyPairSync('ec', { namedCurve: 'P-256' }).privateKey.export({ type: 'pkcs8', format: 'pem' })],
    ['WORKSHOP_APP_ID', 'one'],
  ];
  assert.equal(readConfig(base).configured, true);
  for (const [name, value] of cases) {
    const config = readConfig({ ...base, [name]: value });
    assert.equal(config.configured, false, `${name}=${value}`);
    assert.equal(config.problem, name);
    const logs = [];
    const workshop = createWorkshop({ config, log: (entry) => logs.push(JSON.stringify(entry)) });
    const response = await workshop.handle({ method: 'GET', url: '/api/workshop/state', headers: {}, body: '' });
    assert.equal(response.status, 503);
    assert.equal(JSON.parse(response.body).error, 'not_configured');
    assert.equal(response.body.includes(name), false);
    assert.ok(logs.some((line) => line.includes(name)), 'the log names the variable');
    assert.equal(logs.join('').includes(value.slice(0, 12)), false, 'never its value');
  }
  const missing = readConfig({ WORKSHOP_SITE_ORIGIN: ORIGIN });
  assert.equal(missing.configured, false);
  assert.equal(missing.problem, null);
});

test('no response and no log line carries a token, a code, the client secret or the session secret', async () => {
  const t = await setup();
  try {
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const owner = t.client();
    await owner.signIn('owner-fake');
    await visitor.call('GET', 'me');
    await visitor.call('GET', 'issue?n=1');
    await visitor.call('GET', 'vote?n=1');
    await visitor.call('POST', 'comment', { body: { n: 1, text: 'hi' } });
    await visitor.call('POST', 'vote', { body: { n: 1, on: true } });
    await visitor.call('POST', 'suggest', { body: { title: 'T', text: 'x' } });
    await owner.call('POST', 'verdict', { body: { n: 2, verdict: 'later' } });
    t.fake.state.failNext = 3;
    await visitor.call('POST', 'comment', { body: { n: 1, text: 'fails' } });
    t.fake.state.rateLimited = true;
    t.clock.t += 61_000;
    await visitor.call('GET', 'state');
    await visitor.call('POST', 'comment', { body: { n: 1, text: 'limited' } });
    t.fake.state.rateLimited = false;
    t.clock.t += 9 * 3600_000;
    t.fake.state.refresh.clear();
    await visitor.call('GET', 'me');
    await owner.call('POST', 'logout', { body: {} });
    const everything = [...t.outputs, ...t.logs].join('\n');
    const secrets = t.secrets();
    assert.ok(secrets.length > 6);
    for (const secret of secrets) assert.equal(everything.includes(secret), false, 'a secret leaked');
    for (const fragment of ['ghu_', 'ghr_', 'ghs_', 'BEGIN RSA', 'Bearer']) assert.equal(everything.includes(fragment), false, fragment);
    assert.ok(t.logs.length > 10);
    for (const line of t.logs) assert.deepEqual(Object.keys(JSON.parse(line)).filter((k) => !['endpoint', 'status', 'github', 'failure', 'snapshot'].includes(k)), []);
  } finally { await t.close(); }
});

// Model: random operations by random people on random issues, a refused verdict
// among them. After every step, only Workshop issues have changed, and no entry's
// labels or state have: nothing the server does sets a verdict.
test('model: writes only reach Workshop issues, never change an entry, and an entry has one thread', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const built = t.fake.addIssue({ title: 'Built', body: 'x\n<!-- silo-entry: built-one -->', labels: ['workshop:entry'], state: 'closed', reason: 'completed' }).number;
    const retired = [4, 5];
    const outside = [7, 8].map((n) => JSON.stringify(issueOf(t.fake, n)));
    const retiredBefore = retired.map((n) => JSON.stringify(issueOf(t.fake, n)));
    const builtBefore = JSON.stringify(issueOf(t.fake, built));
    const entries = [1, 2, 3];
    const workshop = new Set([1, 2, 3, 6, built]);
    let seed = 12345;
    const rand = (k) => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed % k; };
    const numbers = [1, 2, 3, 4, 5, 6, 7, 8, built, 99];
    const verdicts = ['take', 'later', 'drop', null, 'refine'];
    // The reference model of each entry's thread: the issue its marker names, the suggestion it was
    // written from, or the one the first write on it opens; never a second one.
    const threads = new Map([['engine-service', 1], ['calls-and-events', 2], ['vm-runtime', 3], ['share-link', 6]]);
    const ids = [...PLAN_ENTRIES.map((entry) => entry.id), 'nope'];
    const markerIssues = (id) => [...t.fake.state.issues.values()].filter((i) => i.labels.includes('workshop:entry') && readEntryMarker(i.body) === id).length;
    for (let step = 0; step < 200; step += 1) {
      t.clock.t += 3_000;
      const who = rand(2) ? owner : visitor;
      const isOwner = who === owner;
      const n = numbers[rand(numbers.length)];
      const op = rand(5);
      let response;
      if (op === 4) {
        const id = ids[rand(ids.length)];
        const on = rand(2) === 0;
        response = rand(2) ? await who.call('POST', 'comment', { body: { entry: id, text: `e ${step}` } }) : await who.call('POST', 'vote', { body: { entry: id, on } });
        if (id === 'nope') {
          assert.equal(response.status, 404);
        } else {
          assert.equal(response.status, 200, response.body);
          if (!threads.has(id) && response.json.n !== undefined) threads.set(id, response.json.n);
          if (threads.has(id)) assert.equal(response.json.n, threads.get(id), `${id} at step ${step}`);
          else assert.deepEqual(response.json, { ok: true, up: 0 }, 'removing a 👍 where there is no thread');
          if (threads.has(id) && ![1, 2, 3, 6].includes(threads.get(id))) workshop.add(threads.get(id));
        }
        for (const entry of PLAN_ENTRIES) assert.equal(markerIssues(entry.id), entry.suggestedIn ? 0 : threads.has(entry.id) ? 1 : 0, `${entry.id} at step ${step}`);
      } else if (op === 0) {
        const verdict = verdicts[rand(verdicts.length)];
        response = await who.call('POST', 'verdict', { body: { n, verdict } });
        assert.deepEqual([response.status, response.json.error], [404, 'not_found'], `${isOwner ? 'owner' : 'visitor'} #${n}`);
      } else if (op === 1) {
        response = await who.call('POST', 'comment', { body: { n, text: `c ${step}` } });
        assert.equal(response.status === 200, workshop.has(n), response.body);
      } else if (op === 2) {
        response = await who.call('POST', 'vote', { body: { n, on: rand(2) === 0 } });
        assert.equal(response.status === 200, workshop.has(n));
      } else {
        response = await who.call('GET', `vote?n=${n}`);
        assert.equal(response.status === 200, workshop.has(n));
      }
      assert.deepEqual([7, 8].map((m) => JSON.stringify(issueOf(t.fake, m))), outside);
      assert.deepEqual(retired.map((m) => JSON.stringify(issueOf(t.fake, m))), retiredBefore);
      for (const m of entries) {
        const issue = issueOf(t.fake, m);
        assert.deepEqual(issue.labels, ['workshop:entry'], `entry ${m} at step ${step}`);
        assert.equal(issue.state, 'open');
      }
      const done = issueOf(t.fake, built);
      assert.deepEqual([done.labels, done.state, done.state_reason], [['workshop:entry'], 'closed', 'completed'], `built entry at step ${step}`);
    }
    assert.ok(threads.has('fork-computers') && threads.has('which-distro'), 'the entries without an issue got one');
    // Comments and votes were the only writes the built entry took.
    const after = issueOf(t.fake, built);
    assert.deepEqual(JSON.stringify({ ...after, comments: 0, reactions: 0 }), JSON.stringify({ ...JSON.parse(builtBefore), comments: 0, reactions: 0 }));
  } finally { await t.close(); }
});

test('the Node adapter accepts bodies up to 64 KB and refuses larger ones', async () => {
  assert.equal(MAX_BODY, 64 * 1024);
  const make = (size) => Object.assign(Readable.from([Buffer.alloc(size, 'a')]), { method: 'POST', url: '/api/workshop/comment', headers: {} });
  assert.equal((await readRequest(make(60 * 1024))).body.length, 60 * 1024);
  assert.equal((await readRequest(make(65 * 1024))).tooLarge, true);
  const t = await setup();
  try {
    const handler = createNodeHandler(t.workshop);
    const res = fakeResponse();
    await handler(Object.assign(make(65 * 1024), { headers: { origin: ORIGIN, 'x-workshop': '1' } }), res);
    assert.equal(res.statusCode, 413);
    const c = t.client();
    await c.signIn('visitor-fake');
    const wide = '😀'.repeat(5000);
    assert.ok(Buffer.byteLength(JSON.stringify({ n: 1, text: wide })) > 16 * 1024);
    assert.equal((await c.call('POST', 'comment', { body: { n: 1, text: wide } })).status, 200);
  } finally { await t.close(); }
});

async function listFiles(dir, prefix = '') {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const name = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) found.push(...await listFiles(join(dir, entry.name), name));
    else found.push(name);
  }
  return found;
}

function fakeResponse() {
  return {
    headers: {},
    setHeader(name, value) { this.headers[name.toLowerCase()] = value; },
    end(body) { this.body = body; this.ended = true; },
  };
}

// The plan a local server's fake GitHub is seeded from in these tests: a small valid plan directory.
const FIXTURE_FEATURES = [
  ['engine-service', 'The engine runs on its own', 'A background engine owns every computer.'],
  ['calls-and-events', 'Every call and event defined once', 'Each call is written down once.'],
  ['vm-runtime', 'The VM runtime, packaged', 'Silo ships the program that runs each computer.'],
  ['fixture-only', 'Only in the fixture', 'Proves the fixture is read.'],
];

const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

async function writePlan(dir) {
  await mkdir(join(dir, 'part-a', 'features'), { recursive: true });
  await writeFile(join(dir, 'README.md'), 'The model.\n');
  await writeFile(join(dir, 'PRODUCT.md'), '# Fixture\n\nA small plan.\n');
  await writeFile(join(dir, 'parts.json'), json([{ id: 'part-a', title: 'Part A', line: 'The one part of this plan.' }]));
  await writeFile(join(dir, 'part-a', 'README.md'), 'Part A.\n');
  await writeFile(join(dir, 'order.json'), json({ stages: [{ id: 'start', title: 'Start', line: 'The first things.', entries: FIXTURE_FEATURES.map(([id]) => id) }] }));
  await writeFile(join(dir, 'words.json'), json([]));
  await writeFile(join(dir, 'principles.json'), json([]));
  for (const [id, name, what] of FIXTURE_FEATURES) await writeFile(join(dir, 'part-a', 'features', `${id}.json`), json({ name, what }));
  // The plan tool is loaded only by the tests that write a plan: the server's own tests never need it.
  const { checkFiles } = await import('../../tools/plan/check/index.js');
  const { loadDir } = await import('../../tools/plan/read/index.js');
  const found = checkFiles(loadDir(dir));
  assert.deepEqual(found.errors, [], 'the fixture plan is valid');
}

async function fixtureSite(page = 'workshop/index.html', { plan = false } = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'workshop-site-'));
  await mkdir(join(dir, 'dist', 'workshop'), { recursive: true });
  await writeFile(join(dir, 'dist', 'index.html'), '<!doctype html><title>Silo</title>');
  if (page) await writeFile(join(dir, 'dist', page), '<!doctype html><title>Workshop</title>');
  if (plan) await writePlan(join(dir, 'plan'));
  return dir;
}

test('Build Output layout: static site, one Node.js function at every endpoint, and the Workshop routes', async () => {
  const site = await fixtureSite();
  const outputDir = join(site, '.vercel', 'output');
  try {
    const website = new URL('..', import.meta.url).pathname;
    assert.equal(DEFAULT_OUTPUT, join(website, '..', '.vercel', 'output'), 'the repository root, where a Git build with the repository as root directory reads it');
    await prepareVercel({ siteDir: website, distDir: join(site, 'dist'), outputDir });
    const config = JSON.parse(await readFile(join(outputDir, 'config.json'), 'utf8'));
    assert.deepEqual(config, { version: 3, routes: [
      { src: '^/(?!films/).*$', caseSensitive: true, headers: { 'Content-Security-Policy': "frame-ancestors 'none'", 'X-Frame-Options': 'DENY' }, continue: true },
      { src: '^/films/.*$', caseSensitive: true, headers: { 'Content-Security-Policy': "frame-ancestors 'self'", 'X-Frame-Options': 'SAMEORIGIN' }, continue: true },
      { src: '^/workshop/?$', dest: '/workshop/index.html' },
      { src: '^/films/([a-z0-9][a-z0-9-]*)/?$', caseSensitive: true, dest: '/films/$1/index.html' },
      { handle: 'filesystem' },
      { handle: 'hit' },
      { src: '^/(assets/.*|fonts/.*\\.woff2)$', headers: { 'Cache-Control': 'public, max-age=31536000, immutable' }, continue: true },
    ] });
    // The headers each path gets, as Vercel applies the routes before `filesystem` in order: a route's src
    // matches without regard to case unless caseSensitive is set, and a later route's header replaces an earlier one's.
    const headersOf = (path) => {
      const headers = {};
      for (const route of config.routes.slice(0, config.routes.findIndex((r) => r.handle))) {
        if (route.headers && new RegExp(route.src, route.caseSensitive ? '' : 'i').test(path)) Object.assign(headers, route.headers);
      }
      return headers;
    };
    const strict = { 'Content-Security-Policy': "frame-ancestors 'none'", 'X-Frame-Options': 'DENY' };
    const loose = { 'Content-Security-Policy': "frame-ancestors 'self'", 'X-Frame-Options': 'SAMEORIGIN' };
    for (const path of ['/', '/workshop/', '/films', '/FILMS/a/', '/Films/a/index.html', '/x/films/a/', '/api/workshop/state']) assert.deepEqual(headersOf(path), strict, path);
    for (const path of ['/films/a/', '/films/a', '/films/a/index.html', '/films/player/kit.js']) assert.deepEqual(headersOf(path), loose, path);
    assert.deepEqual(config.routes.map((route) => [route.src ?? route.handle, route.caseSensitive === true]), [
      ['^/(?!films/).*$', true], ['^/films/.*$', true], ['^/workshop/?$', false], ['^/films/([a-z0-9][a-z0-9-]*)/?$', true],
      ['filesystem', false], ['hit', false], ['^/(assets/.*|fonts/.*\\.woff2)$', false],
    ]);
    // No page may be framed but a film's, and that one only by a page of the site.
    for (const path of ['/', '/workshop', '/assets/x.js', '/api/workshop/state', '/films', '/x/films/a/']) assert.match(path, new RegExp(config.routes[0].src));
    for (const path of ['/films/a/', '/films/a/index.html', '/films/player/kit.js']) assert.doesNotMatch(path, new RegExp(config.routes[0].src));
    for (const path of ['/films/a/', '/films/player/kit.js']) assert.match(path, new RegExp(config.routes[1].src));
    for (const path of ['/', '/films', '/workshop']) assert.doesNotMatch(path, new RegExp(config.routes[1].src));
    for (const path of ['/workshop', '/workshop/']) assert.match(path, new RegExp(config.routes[2].src));
    for (const path of ['/workshops', '/workshop/x', '/api/workshop/state']) assert.doesNotMatch(path, new RegExp(config.routes[2].src));
    for (const path of ['/films/engine-one-call', '/films/engine-one-call/']) assert.equal(path.replace(new RegExp(config.routes[3].src), config.routes[3].dest), '/films/engine-one-call/index.html');
    for (const path of ['/films/', '/films/a/voice/b.mp3', '/films/a/explainer.js', '/films/A/']) assert.doesNotMatch(path, new RegExp(config.routes[3].src));
    // Only what never changes under its name is kept by the browser: hashed assets and font files, and
    // only when the file exists (the `hit` phase).
    for (const path of ['/assets/main-2DSE1Vge.js', '/assets/main-DpTBd47T.css', '/fonts/ibm-plex-sans-400-normal.woff2']) assert.match(path, new RegExp(config.routes[6].src));
    for (const path of ['/', '/workshop', '/workshop/index.html', '/theme.js', '/og.png', '/favicon.svg', '/fonts/fonts.css', '/fonts/OFL.txt', '/api/workshop/state', '/x/assets/y.js']) assert.doesNotMatch(path, new RegExp(config.routes[6].src));
    assert.equal(await readFile(join(outputDir, 'static', 'workshop', 'index.html'), 'utf8'), '<!doctype html><title>Workshop</title>');
    assert.ok((await lstat(join(outputDir, 'static', 'index.html'))).isFile());

    const functions = join(outputDir, 'functions', 'api', 'workshop');
    assert.deepEqual((await readdir(functions)).sort(), ENDPOINTS.map((e) => `${e}.func`).sort());
    const main = join(functions, 'state.func');
    assert.ok((await lstat(main)).isDirectory());
    for (const endpoint of ENDPOINTS.slice(1)) {
      const link = join(functions, `${endpoint}.func`);
      assert.ok((await lstat(link)).isSymbolicLink(), endpoint);
      assert.equal(await readlink(link), 'state.func');
    }
    const vc = JSON.parse(await readFile(join(main, '.vc-config.json'), 'utf8'));
    assert.deepEqual(vc, { runtime: RUNTIME, handler: 'website/api/workshop.cjs', launcherType: 'Nodejs', shouldAddHelpers: false, maxDuration: 15 });
    assert.equal(HANDLER, vc.handler);
    assert.match(vc.runtime, /^nodejs(22|24|26)\.x$/);
    assert.deepEqual((await readdir(main)).sort(), ['.vc-config.json', 'tools', 'website']);
    // No package ships: the server runs on Node's standard library.
    assert.deepEqual((await readdir(join(main, 'website'))).sort(), ['api', 'server']);
    // The Workshop's model ships beside the server, at its place in the repository, as ESM; nothing of the plan tool ships.
    const model = (await readdir(new URL('../../tools/workshop/model/', import.meta.url))).filter((name) => name.endsWith('.js')).sort();
    assert.ok(model.includes('index.js') && model.includes('markers.js'));
    assert.deepEqual((await listFiles(join(main, 'tools'))).sort(), ['workshop/package.json', ...model.map((name) => `workshop/model/${name}`)].sort());
    assert.equal(JSON.parse(await readFile(join(main, 'tools', 'workshop', 'package.json'), 'utf8')).type, 'module');

    // The packaged function runs on its own, from a clean folder with nothing above it to resolve from.
    const clean = await mkdtemp(join(tmpdir(), 'workshop-func-'));
    try {
      await cp(main, join(clean, 'state.func'), { recursive: true });
      const saved = Object.fromEntries(Object.keys(process.env).filter((k) => k.startsWith('WORKSHOP_')).map((k) => [k, process.env[k]]));
      for (const key of Object.keys(saved)) delete process.env[key];
      try {
        const entry = createRequire(import.meta.url)(join(clean, 'state.func', HANDLER));
        const req = Object.assign(Readable.from([]), { method: 'GET', url: '/api/workshop/state', headers: {} });
        const res = fakeResponse();
        await entry(req, res);
        assert.equal(res.statusCode, 503);
        assert.equal(JSON.parse(res.body).error, 'not_configured');
      } finally {
        Object.assign(process.env, saved);
      }
    } finally { await rm(clean, { recursive: true, force: true }); }
    const serverFiles = (await readdir(join(main, 'website', 'server'))).sort();
    assert.deepEqual(serverFiles, ['app-auth.mjs', 'config.mjs', 'entries.mjs', 'github.mjs', 'issues.mjs', 'node.mjs', 'plan-entries.json', 'session.mjs', 'snapshot.mjs', 'workshop.mjs']);
    // The plan's entries, made from the plan at build time, beside the server.
    const shippedEntries = JSON.parse(await readFile(join(main, 'website', 'server', 'plan-entries.json'), 'utf8'));
    assert.deepEqual(shippedEntries, planEntries());
    assert.ok(shippedEntries.length > 100 && shippedEntries.every((e) => ['decision', 'feature'].includes(e.kind) && e.title));

    // The packaged entry point loads and answers on its own.
    const saved = Object.fromEntries(Object.keys(process.env).filter((k) => k.startsWith('WORKSHOP_')).map((k) => [k, process.env[k]]));
    for (const key of Object.keys(saved)) delete process.env[key];
    try {
      const entry = createRequire(import.meta.url)(join(main, vc.handler));
      const req = Readable.from([]);
      Object.assign(req, { method: 'GET', url: '/api/workshop/state', headers: {} });
      const res = fakeResponse();
      await entry(req, res);
      assert.equal(res.statusCode, 503);
      assert.equal(JSON.parse(res.body).error, 'not_configured');
    } finally {
      Object.assign(process.env, saved);
    }

    const flat = await fixtureSite('workshop.html');
    try {
      const result = await prepareVercel({ siteDir: website, distDir: join(flat, 'dist'), outputDir: join(flat, '.vercel', 'output') });
      assert.equal(result.config.routes[2].dest, '/workshop.html');
    } finally { await rm(flat, { recursive: true, force: true }); }
    const none = await fixtureSite(null);
    try {
      await assert.rejects(prepareVercel({ siteDir: website, distDir: join(none, 'dist'), outputDir: join(none, '.vercel', 'output') }), /no Workshop page/);
    } finally { await rm(none, { recursive: true, force: true }); }
  } finally { await rm(site, { recursive: true, force: true }); }
});

async function freePort() {
  const server = createServer();
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

test('workshop-dev --fake serves the site and the API on one port, with sign-in through the fake GitHub', async () => {
  const site = await fixtureSite(undefined, { plan: true });
  await writeFile(join(site, 'dist', 'workshop', 'index.html'), '<!doctype html><head><title>Workshop</title></head>');
  const dev = await startDev({ fake: true, port: await freePort(), distDir: join(site, 'dist'), planDir: join(site, 'plan'), log: () => {} });
  try {
    for (const path of ['/workshop', '/workshop/']) {
      const page = await fetch(dev.origin + path);
      assert.equal(page.status, 200);
      assert.equal(await page.text(), '<!doctype html><head><title>Workshop</title></head>');
    }
    assert.equal(loadEntries(join(site, 'plan')).length, 4, 'seeded from the plan directory');
    assert.equal((await fetch(dev.origin + '/')).status, 200);
    assert.equal((await fetch(dev.origin + '/%2e%2e/package.json')).status, 404);
    const state = await (await fetch(dev.origin + '/api/workshop/state')).json();
    const entries = Object.values(state.issues).filter((i) => i.kind === 'entry');
    // Only the first few entries have an issue; the others have no thread yet, so rows without one can be tried.
    assert.equal(entries.length, SEEDED_ENTRIES);
    assert.deepEqual(entries.map((i) => i.entry), loadEntries(join(site, 'plan')).slice(0, SEEDED_ENTRIES).map((entry) => entry.id));
    assert.ok(Object.keys(state.threads).length === SEEDED_ENTRIES);
    assert.equal(state.threads['fixture-only'], undefined);
    const voted = Object.values((await (await fetch(dev.origin + '/api/workshop/issue?n=1')).json()).comments).some((comment) => comment.up > 0);
    assert.ok(voted, 'a seeded comment has votes');
    assert.equal(Object.values(state.issues).some((i) => i.kind === 'decision'), false, 'the retired kind is gone');

    const start = await fetch(dev.origin + '/api/workshop/login?return=/workshop', { redirect: 'manual' });
    const signin = start.headers.get('set-cookie').split(';')[0];
    assert.match(signin, /^workshop-signin=/);
    const authorize = await fetch(start.headers.get('location'));
    const html = await authorize.text();
    const link = html.match(/href="([^"]*login=owner-fake[^"]*)"/)[1].replaceAll('&#38;', '&');
    const approved = await fetch(new URL(link, dev.fakeGitHub.url), { redirect: 'manual' });
    const callback = await fetch(approved.headers.get('location'), { redirect: 'manual', headers: { cookie: signin } });
    assert.equal(callback.status, 302);
    assert.equal(callback.headers.get('location'), '/workshop');
    const cookies = callback.headers.getSetCookie();
    assert.ok(cookies.some((c) => c.startsWith('workshop-hint=1')));
    const session = cookies.find((c) => c.startsWith('workshop-session=')).split(';')[0];
    const me = await (await fetch(dev.origin + '/api/workshop/me', { headers: { cookie: session } })).json();
    assert.equal(me.login, 'owner-fake');
    const comment = await fetch(dev.origin + '/api/workshop/comment', {
      method: 'POST',
      headers: { cookie: session, origin: dev.origin, 'x-workshop': '1', 'content-type': 'application/json' },
      body: JSON.stringify({ n: 1, text: 'From the dev server.' }),
    });
    assert.equal(comment.status, 200);
    // The dev server knows the plan's entries: a write may name one.
    const onEntry = await fetch(dev.origin + '/api/workshop/comment', {
      method: 'POST',
      headers: { cookie: session, origin: dev.origin, 'x-workshop': '1', 'content-type': 'application/json' },
      body: JSON.stringify({ entry: 'fixture-only', text: 'On an entry.' }),
    });
    assert.equal(onEntry.status, 200);
    // An entry with no issue gets one at its first comment.
    const opened = (await onEntry.json()).n;
    assert.ok(Number.isInteger(opened) && !Object.keys(state.issues).includes(String(opened)));
    const after = await (await fetch(dev.origin + '/api/workshop/state')).json();
    assert.equal(after.threads['fixture-only'], opened);
  } finally {
    await dev.close();
    await rm(site, { recursive: true, force: true });
  }
});

test('me calls nothing on GitHub and never refreshes the token', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    t.clock.t += 7 * 3600_000 + 58 * 60_000;
    const before = t.fake.state.requests.length;
    assert.equal((await c.call('GET', 'me')).status, 200);
    assert.equal((await c.call('GET', 'me')).status, 200);
    assert.deepEqual(t.fake.state.requests.slice(before), []);
  } finally { await t.close(); }
});

test('while GitHub has told the installation to wait, nothing calls GitHub with it, and suggest creates nothing', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    t.fake.state.rateLimited = true;
    assert.equal((await t.client().call('GET', 'state')).status, 502, 'cold start with GitHub refusing');
    t.fake.state.rateLimited = false;
    const before = t.fake.state.requests.length;
    const issues = t.fake.state.issues.size;
    assert.equal((await c.call('GET', 'me')).status, 200);
    assert.equal((await t.client().call('GET', 'issue?n=1')).status, 502);
    const suggest = await c.call('POST', 'suggest', { body: { title: 'Blocked', text: 'x' } });
    assert.equal(suggest.status, 429);
    assert.equal(t.fake.state.issues.size, issues, 'no issue created');
    assert.equal(t.fake.state.requests.slice(before).filter((r) => r.kind === 'installation' || r.kind === 'app').length, 0);
  } finally { await t.close(); }
});

test('suggest refuses before creating anything when the budget is spent', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    await withBudget({ labelCallsPerMinute: 0 }, async () => {
      const issues = t.fake.state.issues.size;
      const response = await c.call('POST', 'suggest', { body: { title: 'Spent', text: 'x' } });
      assert.equal(response.status, 429);
      assert.equal(t.fake.state.issues.size, issues);
    });
  } finally { await t.close(); }
});

test('unlabelled suggestions are retried after a successful suggestion or a refresh, never on other requests', async () => {
  const t = await setup();
  const saved = BUDGET.labelCallsPerMinute;
  BUDGET.labelCallsPerMinute = 40;
  try {
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const owner = t.client();
    await owner.signIn('owner-fake');
    await t.client().call('GET', 'state');
    t.fake.state.hook = (r) => (r.kind === 'installation' && r.path.endsWith('/labels') ? { status: 500 } : null);
    const first = await visitor.call('POST', 'suggest', { body: { title: 'First', text: 'x' } });
    const second = await visitor.call('POST', 'suggest', { body: { title: 'Second', text: 'x' } });
    const third = await visitor.call('POST', 'suggest', { body: { title: 'Third', text: 'x' } });
    assert.deepEqual([first.status, second.status, third.status], [502, 502, 502]);
    await t.workshop.idle();
    t.fake.state.hook = null;
    const before = t.fake.state.requests.length;
    assert.equal((await owner.call('GET', 'vote?n=1')).status, 200);
    assert.equal((await owner.call('GET', 'me')).status, 200);
    await t.workshop.idle();
    assert.equal(t.fake.state.requests.slice(before).filter((r) => r.kind === 'installation').length, 0, 'other requests never retry');
    Object.assign(issueOf(t.fake, second.json.number), { state: 'closed', state_reason: 'not_planned' });
    issueOf(t.fake, third.json.number).labels.push('spam');
    t.fake.state.hook = (r) => (r.kind === 'installation' && r.method === 'GET' && r.path.endsWith(`/issues/${first.json.number}`) ? { status: 500 } : null);
    t.clock.t += 61_000;
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    assert.deepEqual(issueOf(t.fake, second.json.number).labels, [], 'a closed suggestion is not labelled');
    assert.deepEqual(issueOf(t.fake, third.json.number).labels, ['spam'], 'an issue with any label is left alone');
    assert.deepEqual(issueOf(t.fake, first.json.number).labels, [], 'failing this time');
    t.fake.state.hook = null;
    const fourth = await visitor.call('POST', 'suggest', { body: { title: 'Fourth', text: 'x' } });
    assert.equal(fourth.status, 200);
    await t.workshop.idle();
    assert.deepEqual(issueOf(t.fake, first.json.number).labels, [LABELS.suggestion], 'retried after a successful suggestion');
    const labels = count(t.fake, 'POST', /\/labels$/, 'installation');
    t.clock.t += 61_000;
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    assert.equal(count(t.fake, 'POST', /\/labels$/, 'installation'), labels, 'the list is empty now');
  } finally { BUDGET.labelCallsPerMinute = saved; t.fake.state.hook = null; await t.close(); }
});

async function settle(t, client) {
  await client.call('GET', 'state');
  await t.workshop.idle();
}

test('an issue with two Workshop kind labels is not a Workshop issue', async () => {
  const t = await setup();
  try {
    const both = t.fake.addIssue({ title: 'Both', labels: ['workshop:entry', 'workshop:suggestion'] });
    const c = t.client();
    await c.signIn('visitor-fake');
    assert.equal((await c.call('GET', 'state')).json.issues[both.number], undefined);
    assert.equal((await c.call('POST', 'comment', { body: { n: both.number, text: 'x' } })).status, 404);
    assert.equal((await c.call('GET', `vote?n=${both.number}`)).status, 404);
  } finally { await t.close(); }
});


test('the entry marker line is removed from a body', async () => {
  assert.deepEqual(readBody('Text.\n<!-- silo-entry: engine-service -->'), { entry: 'engine-service', text: 'Text.' });
  assert.deepEqual(readBody('Only text.'), { entry: null, text: 'Only text.' });
  assert.deepEqual(readBody('Text.\n<!-- silo-entry: Bad Id! -->').entry, null);
  assert.equal(readBody('<!-- silo-entry: UPPER -->').entry, null, 'an id must match the model pattern');
  for (const body of ['Text.\n<!-- silo-entry: engine-service -->', '<!-- silo-entry: a -->\nText.', 'Text.\n  <!--silo-entry:  a  -->', `${'x'.repeat(CUTS.body + 10)}\n<!-- silo-entry: engine-service -->`, 'Text.\r\n<!-- silo-entry: engine-service -->\r\n']) {
    assert.equal(readBody(body).entry, readEntryMarker(body), 'the server reads the entry marker as the model does');
    assert.equal(readBody(body).text, cut(withoutMarkers(body), CUTS.body), 'and removes the markers as the model does, then cuts');
  }
  const long = readBody(`${'x'.repeat(CUTS.body + 10)}\n<!-- silo-entry: engine-service -->`);
  assert.deepEqual([long.entry, long.text.length], ['engine-service', CUTS.body], 'a marker past the cut is still read');
  const t = await setup();
  try {
    const issue = issueOf(t.fake, 1);
    issue.body = 'A body.\n<!-- silo-entry: engine-service -->';
    t.clock.t += 61_000;
    const c = t.client();
    await settle(t, c);
    const answer = (await c.call('GET', 'issue?n=1')).json;
    assert.equal(answer.body, 'A body.');
    assert.equal((await c.call('GET', 'state')).json.issues[1].entry, 'engine-service');
  } finally { await t.close(); }
});

test('a comment a person writes through the Workshop is not the bot\'s, though GitHub records it as made via the App; the App\'s own is', async () => {
  const t = await setup();
  try {
    const c = t.client();
    t.fake.addComment(1, null, 'By the App.', START + 9000);
    await c.signIn('visitor-fake');
    await settle(t, c);
    const posted = await c.call('POST', 'comment', { body: { n: 1, text: 'Written by a person.' } });
    assert.equal(posted.status, 200);
    const viaApp = [...t.fake.state.issues.get(1).comments].find((x) => x.body === 'Written by a person.');
    assert.ok(viaApp?.viaApp, 'the fake records the comment as made via the App');
    await settle(t, c);
    const comments = (await c.call('GET', 'issue?n=1')).json.comments;
    assert.equal(comments.find((x) => x.text === 'Written by a person.').bot, false);
    assert.equal(comments.find((x) => x.text === 'By the App.').bot, true);
  } finally { await t.close(); }
});

test('a comment of the Workshop App is an ordinary comment: marked as the bot\'s, with no change and no count of history lines', async () => {
  const t = await setup();
  try {
    t.fake.addUser('silo-workshop[bot]', { type: 'Bot' });
    t.fake.addComment(1, null, 'Updated in bbbbbbb: what, verdict', START + 1000);
    t.fake.addComment(1, 'silo-workshop[bot]', 'Updated in ccccccc: notes', START + 2000);
    t.fake.addComment(1, 'visitor-fake', 'A person.', START + 3000);
    t.fake.addComment(6, null, 'On a suggestion, not listed.', START + 4000);
    const c = t.client();
    await settle(t, c);
    const comments = (await c.call('GET', 'issue?n=1')).json.comments;
    const mine = comments.filter((x) => x.createdAt > new Date(START).toISOString());
    assert.deepEqual(mine.map((x) => x.bot), [true, true, false]);
    assert.ok(mine.every((x) => !Object.hasOwn(x, 'change')), 'no comment carries a change');
    assert.deepEqual(mine.map((x) => x.text), ['Updated in bbbbbbb: what, verdict', 'Updated in ccccccc: notes', 'A person.']);
    const state = (await c.call('GET', 'state')).json;
    assert.ok(Object.values(state.issues).every((issue) => !Object.hasOwn(issue, 'updates')), 'an entry carries no count of history lines');
    assert.equal(state.issues[1].comments, comments.length, 'the App\'s comments count as comments');
  } finally { await t.close(); }
});

test('state: holds no list of the latest comments', async () => {
  const t = await setup();
  try {
    t.fake.addComment(1, 'visitor-fake', 'A comment.', START + 1000);
    const c = t.client();
    await settle(t, c);
    const state = (await c.call('GET', 'state')).json;
    assert.equal(Object.hasOwn(state, 'latest'), false);
  } finally { await t.close(); }
});

test('configuration: a non-canonical secret or a bad key is not_configured at the first request, login included', async () => {
  const fake = createFakeGitHub();
  const base = fake.workshopEnv(ORIGIN);
  const canonical = base.WORKSHOP_SESSION_SECRET;
  const last = canonical.at(-2);
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const loose = canonical.slice(0, -2) + alphabet[(alphabet.indexOf(last) & ~3) | 1] + '=';
  assert.equal(Buffer.from(loose, 'base64').length, 32, 'decodes, but is not the canonical encoding');
  for (const [name, value] of [['WORKSHOP_SESSION_SECRET', loose], ['WORKSHOP_PRIVATE_KEY', 'not a key']]) {
    const logs = [];
    const workshop = createWorkshop({ config: readConfig({ ...base, [name]: value }), log: (entry) => logs.push(JSON.stringify(entry)) });
    assert.equal((await workshop.handle({ method: 'GET', url: '/api/workshop/state', headers: {}, body: '' })).status, 503);
    const login = await workshop.handle({ method: 'GET', url: '/api/workshop/login', headers: {}, body: '' });
    assert.equal(login.status, 302);
    assert.equal(login.headers.Location, FAILED);
    assert.deepEqual(login.cookies, []);
    assert.ok(logs.some((line) => line.includes(name)));
  }
});

test('packaging ships server/ and api/ at any depth and the model of the Workshop, and refuses any other file the function loads', async () => {
  const root = await mkdtemp(join(tmpdir(), 'workshop-repo-'));
  try {
    const site = join(root, 'website');
    const here = new URL('..', import.meta.url).pathname;
    for (const part of ['server', 'api']) await cp(join(here, part), join(site, part), { recursive: true });
    await cp(new URL('../../tools/workshop/model', import.meta.url).pathname, join(root, 'tools', 'workshop', 'model'), { recursive: true });
    await cp(new URL('../../tools/workshop/package.json', import.meta.url).pathname, join(root, 'tools', 'workshop', 'package.json'));
    const dist = await fixtureSite();
    try {
      const fine = await prepareVercel({ siteDir: site, distDir: join(dist, 'dist'), outputDir: join(root, 'a', '.vercel', 'output') });
      assert.ok(fine.files.includes(join('tools', 'workshop', 'model', 'markers.js')));
      await mkdir(join(root, 'tools', 'plan', 'model'), { recursive: true });
      await writeFile(join(root, 'tools', 'plan', 'model', 'x.js'), 'export const x = 1;\n');
      const config = join(site, 'server', 'config.mjs');
      const original = await readFile(config, 'utf8');
      await writeFile(config, `import '../../tools/plan/model/x.js';\n${original}`);
      await assert.rejects(prepareVercel({ siteDir: site, distDir: join(dist, 'dist'), outputDir: join(root, 'b', '.vercel', 'output') }), /outside website\//);
      await assert.rejects(lstat(join(root, 'b', '.vercel', 'output')), 'nothing is written');
      // A file deeper in server/ or api/ ships; any other file in website/ is refused by name.
      await mkdir(join(site, 'server', 'deep'), { recursive: true });
      await writeFile(join(site, 'server', 'deep', 'y.mjs'), 'export const y = 1;\n');
      await writeFile(config, `import './deep/y.mjs';\n${original}`);
      const deep = await prepareVercel({ siteDir: site, distDir: join(dist, 'dist'), outputDir: join(root, 'c', '.vercel', 'output') });
      assert.ok(deep.files.includes(join('website', 'server', 'deep', 'y.mjs')));
      assert.ok((await lstat(join(root, 'c', '.vercel', 'output', 'functions', 'api', 'workshop', 'state.func', 'website', 'server', 'deep', 'y.mjs'))).isFile());
      await mkdir(join(site, 'lib'), { recursive: true });
      await writeFile(join(site, 'lib', 'z.mjs'), 'export const z = 1;\n');
      await writeFile(config, `import '../lib/z.mjs';\n${original}`);
      await assert.rejects(prepareVercel({ siteDir: site, distDir: join(dist, 'dist'), outputDir: join(root, 'd', '.vercel', 'output') }), /lib\/z\.mjs, in website\/ but outside website\/server\/ and website\/api\//);
      await assert.rejects(lstat(join(root, 'd', '.vercel', 'output')), 'nothing is written');
      // A package is refused like any other file outside server/ and api/.
      await mkdir(join(site, 'node_modules', 'pkg'), { recursive: true });
      await writeFile(join(site, 'node_modules', 'pkg', 'package.json'), '{ "name": "pkg", "type": "module", "main": "index.js" }\n');
      await writeFile(join(site, 'node_modules', 'pkg', 'index.js'), 'export const p = 1;\n');
      await writeFile(config, `import 'pkg';\n${original}`);
      await assert.rejects(prepareVercel({ siteDir: site, distDir: join(dist, 'dist'), outputDir: join(root, 'e', '.vercel', 'output') }), /node_modules\/pkg\/index\.js, in website\/ but outside website\/server\/ and website\/api\//);
      await assert.rejects(lstat(join(root, 'e', '.vercel', 'output')), 'nothing is written');
    } finally { await rm(dist, { recursive: true, force: true }); }
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('waitUntil hands the promise to the request context Vercel keeps, and asks nothing without one', () => {
  const key = Symbol.for('@vercel/request-context');
  const before = globalThis[key];
  try {
    delete globalThis[key];
    assert.equal(waitUntil(Promise.resolve()), undefined);
    const kept = [];
    globalThis[key] = { get: () => ({ waitUntil: (promise) => { kept.push(promise); } }) };
    const promise = Promise.resolve();
    waitUntil(promise);
    assert.deepEqual(kept, [promise]);
    // A context without the method asks nothing either.
    globalThis[key] = { get: () => ({}) };
    assert.equal(waitUntil(promise), undefined);
  } finally {
    if (before === undefined) delete globalThis[key];
    else globalThis[key] = before;
  }
});

test('packaging refuses an output directory that does not end in .vercel/output, and deletes nothing', async () => {
  const site = await fixtureSite();
  try {
    const website = new URL('..', import.meta.url).pathname;
    const precious = join(site, 'precious');
    await mkdir(precious);
    await writeFile(join(precious, 'keep.txt'), 'keep');
    for (const outputDir of [precious, join(site, 'output'), join(site, '.vercel'), join(site, 'x.vercel', 'output'), '/']) {
      await assert.rejects(prepareVercel({ siteDir: website, distDir: join(site, 'dist'), outputDir }), /must end in \.vercel\/output/);
    }
    assert.equal(await readFile(join(precious, 'keep.txt'), 'utf8'), 'keep');
  } finally { await rm(site, { recursive: true, force: true }); }
});

test('API answers forbid framing; route names are own properties only', async () => {
  const t = await setup();
  try {
    const state = await t.client().call('GET', 'state');
    assert.equal(state.headers['Content-Security-Policy'], "frame-ancestors 'none'");
    assert.equal(state.headers['X-Frame-Options'], 'DENY');
    for (const name of ['constructor', 'hasownproperty', 'valueof']) {
      const response = await t.client().call('GET', name);
      assert.equal(response.status, 404, name);
      assert.ok(Object.values(response.headers).every((value) => typeof value === 'string'), name);
    }
    const odd = await t.workshop.handle({ method: 'GET', url: '//', headers: {}, body: '' });
    assert.equal(odd.status, 404);
  } finally { await t.close(); }
});

test('the local server survives //, an aborted POST body and /api/workshop/constructor, and forbids framing', async () => {
  const site = await fixtureSite(undefined, { plan: true });
  const dev = await startDev({ fake: true, port: await freePort(), distDir: join(site, 'dist'), planDir: join(site, 'plan'), log: () => {} });
  const { port } = new URL(dev.origin);
  const raw = (text) => new Promise((resolve) => {
    const socket = netConnect(Number(port), '127.0.0.1', () => socket.write(text));
    let data = '';
    socket.on('data', (chunk) => { data += chunk; });
    socket.on('close', () => resolve(data));
    socket.on('error', () => resolve(data));
    setTimeout(() => socket.destroy(), 500);
  });
  try {
    assert.match(await raw('GET // HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n'), /^HTTP\/1\.1 (400|404)/);
    await raw(`POST /api/workshop/comment HTTP/1.1\r\nHost: x\r\nContent-Length: 5000\r\nOrigin: ${dev.origin}\r\nX-Workshop: 1\r\n\r\n{"n":1,`);
    const constructor = await fetch(dev.origin + '/api/workshop/constructor');
    assert.equal(constructor.status, 404);
    for (const [, value] of constructor.headers) assert.doesNotMatch(value, /function|native code/);
    const page = await fetch(dev.origin + '/workshop');
    assert.equal(page.status, 200);
    assert.equal(page.headers.get('content-security-policy'), "frame-ancestors 'none'");
    assert.equal((await fetch(dev.origin + '/api/workshop/state')).headers.get('x-frame-options'), 'DENY');
  } finally {
    await dev.close();
    await rm(site, { recursive: true, force: true });
  }
});

test('the local server lets only a file it really serves from dist/films/ be framed, whatever the address as typed', async () => {
  const site = await fixtureSite(undefined, { plan: true });
  await mkdir(join(site, 'dist', 'films', 'tiny', 'voice'), { recursive: true });
  await writeFile(join(site, 'dist', 'films', 'tiny', 'index.html'), '<!doctype html><title>Tiny</title>');
  await writeFile(join(site, 'dist', 'films', 'tiny', 'voice', 'a.mp3'), 'clip');
  const dev = await startDev({ fake: true, port: await freePort(), distDir: join(site, 'dist'), planDir: join(site, 'plan'), log: () => {} });
  const LOOSE = ["frame-ancestors 'self'", 'SAMEORIGIN'];
  const STRICT = ["frame-ancestors 'none'", 'DENY'];
  // fetch() would normalise some of these addresses itself: node:http sends the path as written.
  const get = (path) => new Promise((resolve, reject) => {
    httpGet({ host: '127.0.0.1', port: Number(new URL(dev.origin).port), path }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, frame: [response.headers['content-security-policy'], response.headers['x-frame-options']], body }));
    }).on('error', reject);
  });
  try {
    for (const [path, status, frame, body] of [
      ['/films/tiny/', 200, LOOSE, '<!doctype html><title>Tiny</title>'],
      ['/films/tiny', 200, LOOSE, '<!doctype html><title>Tiny</title>'],
      ['/films/tiny/voice/a.mp3', 200, LOOSE, 'clip'],
      // Encoded forms that lead to a film's file get the film's headers.
      ['/films/tiny%2Findex.html', 200, LOOSE, '<!doctype html><title>Tiny</title>'],
      ['/%66ilms/tiny/', 200, LOOSE, '<!doctype html><title>Tiny</title>'],
      ['/', 200, STRICT, '<!doctype html><title>Silo</title>'],
      ['/workshop/', 200, STRICT, '<!doctype html><title>Workshop</title>'],
      // The home page, reached through /films/: what is served decides, not the address.
      ['/films/..%2findex.html', 200, STRICT, '<!doctype html><title>Silo</title>'],
      ['/films/..%2F..%2Fpackage.json', 404, STRICT, 'Not found'],
      ['/films/missing/', 404, STRICT, 'Not found'],
      ['/films/%E0%A4%A/', 404, STRICT, 'Not found'],
    ]) {
      const response = await get(path);
      assert.deepEqual([response.status, response.frame, response.body], [status, frame, body], path);
    }
    // On a file system that ignores case /FILMS/ finds the film, and on one that does not it is a 404: strict either way.
    const upper = await get('/FILMS/tiny/');
    assert.ok([200, 404].includes(upper.status));
    assert.deepEqual(upper.frame, STRICT, '/FILMS/tiny/');
    const post = await fetch(`${dev.origin}/films/tiny/`, { method: 'POST' });
    assert.equal(post.status, 405);
    assert.equal(post.headers.get('x-frame-options'), 'DENY');
  } finally {
    await dev.close();
    await rm(site, { recursive: true, force: true });
  }
});

test('the local server gives no file the looser headers when dist/films is a link', async () => {
  const site = await fixtureSite(undefined, { plan: true });
  // dist/films leads to a folder outside dist/ that holds a film's page.
  await mkdir(join(site, 'elsewhere', 'tiny'), { recursive: true });
  await writeFile(join(site, 'elsewhere', 'tiny', 'index.html'), '<!doctype html><title>Tiny</title>');
  await symlink(join(site, 'elsewhere'), join(site, 'dist', 'films'));
  const dev = await startDev({ fake: true, port: await freePort(), distDir: join(site, 'dist'), planDir: join(site, 'plan'), log: () => {} });
  try {
    for (const path of ['/films/tiny/', '/films/tiny/index.html']) {
      const response = await fetch(dev.origin + path);
      assert.equal(response.status, 200, path);
      assert.equal(response.headers.get('content-security-policy'), "frame-ancestors 'none'", path);
      assert.equal(response.headers.get('x-frame-options'), 'DENY', path);
    }
  } finally {
    await dev.close();
    await rm(site, { recursive: true, force: true });
  }
});

const PLAN = [
  ['a1', 'take', '2026-11-01'], ['a2'], ['b1'], ['b2'], ['c1', 'take'], ['d2'],
].map(([id, verdict, built]) => ({
  id, area: 'part-a', needs: [], name: `Name of ${id}`, what: `What ${id} does.`,
  ...(verdict ? { verdict } : {}), ...(built ? { built } : {}),
}));

test('seedDemo closes only the entry its file says is built, and seeds no comment of the App', async () => {
  const t = await setup({ seed: false });
  try {
    seedDemo(t.fake, PLAN);
    const issues = Object.values((await t.client().call('GET', 'state')).json.issues);
    assert.deepEqual(issues.filter((i) => i.kind === 'entry' && i.state === 'closed').map((i) => i.entry), ['a1'], 'only the entry its file says is built');
    assert.equal(issues.some((i) => i.updates), false);
    const bots = [...t.fake.state.issues.values()].flatMap((issue) => issue.comments).filter((comment) => comment.user.type === 'Bot' && comment.body !== 'Posted by an agent.');
    assert.deepEqual(bots, []);
  } finally { await t.close(); }
});

test('vote and comment answer the new count, from the issue read before the write, with no extra call', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('owner-fake');
    const issue = (n) => t.fake.state.issues.get(n);
    const ups = (n) => issue(n).reactions.filter((r) => r.content === '+1').length;
    t.fake.react(3, 'visitor-fake');

    const reads = () => t.fake.state.requests.filter((r) => r.kind === 'user' && r.method === 'GET').length;
    let before = reads();
    const added = await c.call('POST', 'vote', { body: { n: 3, on: true } });
    assert.deepEqual(added.json, { ok: true, up: 2 }, 'a vote added counts');
    assert.equal(ups(3), 2);
    assert.equal(reads() - before, 1, 'only the issue read');

    const again = await c.call('POST', 'vote', { body: { n: 3, on: true } });
    assert.deepEqual(again.json, { ok: true, up: 2 }, 'a vote already there adds nothing');

    const removed = await c.call('POST', 'vote', { body: { n: 3, on: false } });
    assert.deepEqual(removed.json, { ok: true, up: 1 }, 'a vote removed');
    assert.equal(ups(3), 1);
    const none = await c.call('POST', 'vote', { body: { n: 3, on: false } });
    assert.deepEqual(none.json, { ok: true, up: 1 }, 'nothing to remove');

    const count = issue(3).comments.length;
    before = reads();
    const posted = await c.call('POST', 'comment', { body: { n: 3, text: 'Counted.' } });
    assert.equal(posted.json.ok, true);
    assert.equal(typeof posted.json.id, 'number');
    assert.equal(posted.json.comments, count + 1, 'a comment posted counts');
    assert.equal(issue(3).comments.length, count + 1);
    assert.equal(reads() - before, 2, 'the issue read and the five-minute check, as before');

    const repeated = await c.call('POST', 'comment', { body: { n: 3, text: 'Counted.' } });
    assert.deepEqual(repeated.json, { ok: true, id: posted.json.id, comments: count + 1 }, 'a comment found already posted is the count as read');
    assert.equal(issue(3).comments.length, count + 1);

    t.fake.state.hook = (r) => (r.kind === 'user' && r.method === 'GET' && r.path === '/repos/amontlabs/silo/issues/3'
      ? { status: 200, body: { number: 3, title: 'x', labels: [{ name: 'workshop:entry' }], state: 'open', user: { login: 'x' } } }
      : null);
    assert.deepEqual((await c.call('POST', 'vote', { body: { n: 3, on: true } })).json, { ok: true }, 'no count read, no field');
    const bare = await c.call('POST', 'comment', { body: { n: 3, text: 'Uncounted.' } });
    assert.equal(Object.hasOwn(bare.json, 'comments'), false);
  } finally { t.fake.state.hook = null; await t.close(); }
});
