import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createPublicKey, createPrivateKey, verify } from 'node:crypto';
import { lstat, mkdir, mkdtemp, readFile, readlink, readdir, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createAppJwt } from '../server/app-auth.mjs';
import { readConfig } from '../server/config.mjs';
import { createNodeHandler } from '../server/node.mjs';
import { seal, unseal } from '../server/session.mjs';
import { createWorkshop, LABELS } from '../server/workshop.mjs';
import { createFakeGitHub, FAKE_APP, seedDemo } from './fake-github.mjs';
import { ENDPOINTS, prepareVercel, RUNTIME } from './prepare-vercel.mjs';
import { startDev } from './workshop-dev.mjs';

const ORIGIN = 'https://silo.test';
const START = Date.parse('2026-11-16T12:00:00Z');

async function setup({ configured = true, seed = true } = {}) {
  const clock = { t: START };
  const now = () => clock.t;
  const fake = createFakeGitHub({ now });
  await fake.listen();
  if (seed) seedDemo(fake);
  const logs = [];
  const outputs = [];
  const env = configured ? fake.workshopEnv(ORIGIN) : { WORKSHOP_SITE_ORIGIN: ORIGIN };
  const config = readConfig(env, { apiBase: fake.url, webBase: fake.url });
  const workshop = createWorkshop({ config, now, log: (entry) => logs.push(JSON.stringify(entry)) });
  const secrets = () => [...fake.issuedTokens, fake.clientSecret, env.WORKSHOP_SESSION_SECRET, ...[...fake.state.codes.keys()]].filter(Boolean);

  function client() {
    const jar = new Map();
    async function call(method, path, { body, headers = {} } = {}) {
      const h = {};
      if (method === 'POST') Object.assign(h, { origin: ORIGIN, 'x-workshop': '1', 'content-type': 'application/json' });
      if (jar.size) h.cookie = [...jar].map(([name, value]) => `${name}=${value}`).join('; ');
      for (const [name, value] of Object.entries(headers)) {
        if (value === undefined) delete h[name];
        else h[name] = value;
      }
      const response = await workshop.handle({ method, url: `/api/workshop/${path}`, headers: h, body: body === undefined ? '' : typeof body === 'string' ? body : JSON.stringify(body) });
      outputs.push(JSON.stringify(response));
      for (const cookie of response.cookies) {
        const [pair, ...attributes] = cookie.split('; ');
        const index = pair.indexOf('=');
        if (attributes.includes('Max-Age=0')) jar.delete(pair.slice(0, index));
        else jar.set(pair.slice(0, index), pair.slice(index + 1));
      }
      return { ...response, json: response.body ? JSON.parse(response.body) : null };
    }
    async function signIn(login) {
      const start = await call('GET', 'login?return=/workshop?open=3');
      assert.equal(start.status, 302);
      const authorize = new URL(start.headers.Location);
      const approve = new URL('/login/oauth/fake-approve', fake.url);
      for (const key of ['redirect_uri', 'state', 'code_challenge']) approve.searchParams.set(key, authorize.searchParams.get(key));
      approve.searchParams.set('login', login);
      const back = new URL((await fetch(approve, { redirect: 'manual' })).headers.get('location'));
      assert.equal(back.origin + back.pathname, `${ORIGIN}/api/workshop/callback`);
      const done = await call('GET', `callback${back.search}`);
      assert.equal(done.status, 302, done.body);
      assert.equal(done.headers.Location, '/workshop?open=3');
      return done;
    }
    return { call, signIn, jar };
  }

  return { fake, workshop, config, clock, logs, outputs, secrets, client, close: () => fake.close() };
}

const count = (fake, method, pattern) => fake.state.requests.filter((r) => r.method === method && pattern.test(r.path)).length;
const issueOf = (fake, n) => fake.state.issues.get(n);
const entryNumber = 2;
const decisionNumber = 4;

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
    await c.call('GET', 'state');
    t.clock.t += 61_000;
    await c.call('GET', 'state');
    t.clock.t += 50 * 60_000;
    await c.call('GET', 'state');
    assert.equal(count(t.fake, 'POST', /access_tokens$/), 1);
    t.clock.t += 6 * 60_000;
    await c.call('GET', 'state');
    assert.equal(count(t.fake, 'POST', /access_tokens$/), 2);
  } finally { await t.close(); }
});

test('state lists every Workshop issue with counts, paginated, publicly cacheable, and cached for 60 seconds', async () => {
  const t = await setup();
  try {
    t.fake.state.perPage = 2;
    const c = t.client();
    const first = await c.call('GET', 'state');
    assert.equal(first.status, 200);
    assert.equal(first.headers['Cache-Control'], 'public, s-maxage=60, stale-while-revalidate=300');
    assert.deepEqual(first.cookies, []);
    const issues = first.json.issues;
    assert.equal(first.json.repo, 'amontlabs/silo');
    assert.deepEqual(Object.keys(issues).map(Number).sort((a, b) => a - b), [1, 2, 3, 4, 5, 6]);
    assert.deepEqual(issues[1], {
      kind: 'entry', entry: 'engine-service', title: 'The engine runs on its own', state: 'open', reason: null, verdict: 'take',
      up: 1, comments: 2, author: 'silo-workshop[bot]', bot: true, createdAt: issues[1].createdAt, closedAt: null,
    });
    assert.equal(issues[3].verdict, 'later');
    assert.equal(issues[5].kind, 'decision');
    assert.equal(issues[5].reason, 'completed');
    assert.equal(issues[6].kind, 'suggestion');
    assert.equal(issues[6].author, 'visitor-fake');
    assert.equal(issues[6].bot, false);
    const lists = count(t.fake, 'GET', /\/issues$/);
    assert.ok(lists > 3, 'pagination followed');
    t.clock.t += 59_000;
    await c.call('GET', 'state');
    assert.equal(count(t.fake, 'GET', /\/issues$/), lists);
    t.clock.t += 2_000;
    await c.call('GET', 'state');
    assert.ok(count(t.fake, 'GET', /\/issues$/) > lists);
  } finally { await t.close(); }
});

test('on a rate limit, state serves the last good answer marked stale for up to 10 minutes, then 429, without calling GitHub while blocked', async () => {
  const t = await setup();
  try {
    const c = t.client();
    assert.equal((await c.call('GET', 'state')).status, 200);
    t.fake.state.rateLimited = true;
    t.clock.t += 61_000;
    const stale = await c.call('GET', 'state');
    assert.equal(stale.status, 200);
    assert.equal(stale.json.stale, true);
    assert.equal(Object.keys(stale.json.issues).length, 6);
    const calls = t.fake.state.requests.length;
    t.clock.t += 30_000;
    assert.equal((await c.call('GET', 'state')).json.stale, true);
    assert.equal(t.fake.state.requests.length, calls, 'no GitHub call while blocked');
    t.clock.t += 10 * 60_000;
    const limited = await c.call('GET', 'state');
    assert.equal(limited.status, 429);
    assert.equal(limited.json.error, 'rate_limited');
    assert.equal(limited.headers['Cache-Control'], 'private, no-store');
    t.fake.state.rateLimited = false;
    t.clock.t += 5 * 60_000;
    assert.equal((await c.call('GET', 'state')).json.stale, undefined);
  } finally { await t.close(); }
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
    for (const path of ['state', 'me', 'issue?n=1', 'login']) {
      const response = await u.client().call('GET', path);
      assert.equal(response.status, 503);
      assert.equal(response.json.error, 'not_configured');
    }
  } finally { await u.close(); }
});

test('me needs a session and reports canDecide from the repository permission', async () => {
  const t = await setup();
  try {
    const anonymous = await t.client().call('GET', 'me');
    assert.equal(anonymous.status, 401);
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const me = await visitor.call('GET', 'me');
    assert.deepEqual(me.json, { login: 'visitor-fake', avatar: 'https://avatars.example.test/visitor-fake.png', canDecide: false });
    assert.equal(me.headers['Cache-Control'], 'private, no-store');
    const owner = t.client();
    await owner.signIn('owner-fake');
    assert.equal((await owner.call('GET', 'me')).json.canDecide, true);
    const permissionReads = count(t.fake, 'GET', /permission$/);
    await owner.call('GET', 'me');
    assert.equal(count(t.fake, 'GET', /permission$/), permissionReads, 'cached for five minutes');
  } finally { await t.close(); }
});

test('issue returns text and comments; anonymous reads are public, signed-in reads are private with viewerUp', async () => {
  const t = await setup();
  try {
    const anonymous = await t.client().call('GET', 'issue?n=1');
    assert.equal(anonymous.status, 200);
    assert.equal(anonymous.headers['Cache-Control'], 'public, s-maxage=30, stale-while-revalidate=300');
    assert.equal(anonymous.json.body, 'A background engine owns every computer.');
    assert.equal(anonymous.json.up, 1);
    assert.equal(anonymous.json.viewerUp, false);
    assert.equal(anonymous.json.url, 'https://github.com/amontlabs/silo/issues/1');
    assert.deepEqual(anonymous.json.comments.map((c) => [c.author, c.bot, c.text]), [
      ['visitor-fake', false, 'Looking forward to this.\nSee https://example.com for `context`.'],
      ['silo-workshop[bot]', true, 'Posted by an agent.'],
    ]);
    const reads = count(t.fake, 'GET', /\/issues\/1$/);
    t.clock.t += 29_000;
    await t.client().call('GET', 'issue?n=1');
    assert.equal(count(t.fake, 'GET', /\/issues\/1$/), reads, 'cached for 30 seconds');

    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const signed = await visitor.call('GET', 'issue?n=1');
    assert.equal(signed.json.viewerUp, true);
    assert.equal(signed.headers['Cache-Control'], 'private, no-store');

    assert.equal((await t.client().call('GET', 'issue?n=7')).status, 404, 'an ordinary issue');
    assert.equal((await t.client().call('GET', 'issue?n=8')).status, 404, 'a pull request');
    assert.equal((await t.client().call('GET', 'issue?n=99')).status, 404);
    assert.equal((await t.client().call('GET', 'issue?n=1x')).status, 400);
    assert.equal(count(t.fake, 'GET', /\/issues\/(7|8|99)$/), 0, 'anonymous reads outside the state cost no GitHub call');
    assert.equal((await visitor.call('GET', 'issue?n=7')).status, 404, 'signed in, read directly');
    assert.equal(count(t.fake, 'GET', /\/issues\/7$/), 1);
  } finally { await t.close(); }
});

test('comment, vote and suggest write as the signed-in person', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const comment = await c.call('POST', 'comment', { body: { n: 2, text: '  Yes please.  ' } });
    assert.equal(comment.status, 200);
    const posted = issueOf(t.fake, 2).comments.at(-1);
    assert.equal(posted.user.login, 'visitor-fake');
    assert.equal(posted.body, 'Yes please.');

    assert.equal((await c.call('POST', 'vote', { body: { n: 2, on: true } })).status, 200);
    assert.deepEqual(issueOf(t.fake, 2).reactions.map((r) => r.user.login).sort(), ['owner-fake', 'visitor-fake']);
    assert.equal((await c.call('GET', 'issue?n=2')).json.viewerUp, true);
    assert.equal((await c.call('POST', 'vote', { body: { n: 2, on: false } })).status, 200);
    assert.deepEqual(issueOf(t.fake, 2).reactions.map((r) => r.user.login), ['owner-fake'], 'only their own reaction removed');

    const suggest = await c.call('POST', 'suggest', { body: { title: 'Run on FreeBSD', text: 'As a host.' } });
    assert.equal(suggest.status, 200);
    const created = issueOf(t.fake, suggest.json.number);
    assert.equal(created.user.login, 'visitor-fake');
    assert.deepEqual(created.labels, [LABELS.suggestion]);
    const labelCall = t.fake.state.requests.findLast((r) => r.path.endsWith(`/issues/${suggest.json.number}/labels`));
    assert.equal(labelCall.kind, 'installation');
    const state = await c.call('GET', 'state');
    assert.equal(state.json.issues[suggest.json.number].kind, 'suggestion', 'a write refreshes this instance\'s cache');
  } finally { await t.close(); }
});

test('verdict transitions leave exactly the right labels and state; a note is the owner\'s comment', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('owner-fake');
    const issue = issueOf(t.fake, entryNumber);
    issue.labels.push('other-label');
    const steps = [
      ['take', ['workshop:entry', 'other-label', 'verdict:take'], 'open'],
      ['later', ['workshop:entry', 'other-label', 'verdict:later'], 'open'],
      ['drop', ['workshop:entry', 'other-label', 'verdict:drop'], 'closed'],
      [null, ['workshop:entry', 'other-label'], 'open'],
    ];
    for (const [verdict, labels, state] of steps) {
      const response = await c.call('POST', 'verdict', { body: { n: entryNumber, verdict } });
      assert.equal(response.status, 200, response.body);
      assert.deepEqual([...issue.labels].sort(), [...labels].sort());
      assert.equal(issue.state, state);
      if (verdict === 'drop') assert.equal(issue.state_reason, 'not_planned');
    }
    const refine = await c.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'refine', note: 'Narrow it to Linux.' } });
    assert.equal(refine.status, 200);
    assert.deepEqual(issue.comments.at(-1).user.login, 'owner-fake');
    assert.deepEqual(issue.comments.at(-1).body, 'Narrow it to Linux.');
    assert.ok(issue.labels.includes('verdict:refine'));
    const patch = t.fake.state.requests.findLast((r) => r.method === 'PATCH');
    assert.equal(patch.login, 'owner-fake', 'GitHub shows the owner as the actor');
  } finally { await t.close(); }
});

test('decide comments and closes an open decision as completed', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('owner-fake');
    const response = await c.call('POST', 'decide', { body: { n: decisionNumber, text: 'Debian.' } });
    assert.equal(response.status, 200);
    const issue = issueOf(t.fake, decisionNumber);
    assert.equal(issue.state, 'closed');
    assert.equal(issue.state_reason, 'completed');
    assert.equal(issue.comments.at(-1).body, 'Debian.');
    assert.equal(issue.comments.at(-1).user.login, 'owner-fake');
    assert.equal((await c.call('POST', 'decide', { body: { n: decisionNumber, text: 'Again.' } })).status, 404, 'no longer open');
    assert.equal((await c.call('POST', 'decide', { body: { n: entryNumber, text: 'x' } })).status, 404, 'not a decision');
    assert.equal((await c.call('POST', 'verdict', { body: { n: decisionNumber, verdict: 'take' } })).status, 404, 'verdict only on entries');
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
      [anonymous, 'verdict', { n: 1, verdict: 'take' }, {}, 401, 'signed_out'],
      [anonymous, 'decide', { n: 4, text: 'x' }, {}, 401, 'signed_out'],
      [visitor, 'verdict', { n: 1, verdict: 'take' }, {}, 403, 'cannot_decide'],
      [visitor, 'decide', { n: 4, text: 'x' }, {}, 403, 'cannot_decide'],
      [visitor, 'comment', { n: 7, text: 'x' }, {}, 404, 'not_found'],
      [visitor, 'vote', { n: 7, on: true }, {}, 404, 'not_found'],
      [visitor, 'comment', { n: 8, text: 'x' }, {}, 404, 'not_found'],
      [owner, 'verdict', { n: 7, verdict: 'take' }, {}, 404, 'not_found'],
      [owner, 'decide', { n: 7, text: 'x' }, {}, 404, 'not_found'],
      [visitor, 'comment', { n: 1, text: 'x'.repeat(5001) }, {}, 400, 'too_long'],
      [visitor, 'comment', { n: 1, text: '   ' }, {}, 400, 'empty'],
      [visitor, 'comment', { n: '1', text: 'x' }, {}, 400, 'bad_input'],
      [visitor, 'vote', { n: 1, on: 'yes' }, {}, 400, 'bad_input'],
      [visitor, 'suggest', { title: 'x'.repeat(121), text: 'y' }, {}, 400, 'too_long'],
      [visitor, 'suggest', { title: '', text: 'y' }, {}, 400, 'empty'],
      [owner, 'verdict', { n: 1, verdict: 'refine' }, {}, 400, 'empty'],
      [owner, 'verdict', { n: 1, verdict: 'refine', note: 'x'.repeat(5001) }, {}, 400, 'too_long'],
      [owner, 'verdict', { n: 1, verdict: 'maybe' }, {}, 400, 'bad_input'],
      [owner, 'decide', { n: 4, text: '' }, {}, 400, 'empty'],
    ];
    for (const [who, path, body, headers, status, code] of cases) {
      const response = await who.call('POST', path, { body, headers });
      assert.equal(response.status, status, `${path} ${JSON.stringify(headers)} ${response.body}`);
      assert.equal(response.json.error, code, `${path} ${JSON.stringify(headers)}`);
      assert.equal(typeof response.json.message, 'string');
    }
    assert.equal(JSON.stringify([...t.fake.state.issues.values()]), before, 'no refusal changed anything');
    assert.equal((await visitor.call('POST', 'comment', { body: '{not json' })).status, 400);
    assert.equal((await visitor.call('GET', 'comment')).status, 405);
    assert.equal((await visitor.call('GET', 'nothing')).status, 404);
    for (const back of ['//evil.test', 'https://evil.test/workshop', '/workshop/../api/workshop/me', '/workshop/%2e%2e/api', '/workshopx', '/workshop\\evil', '/\\evil.test', 'workshop', '/workshop//evil.test', '/api/workshop/me']) {
      const response = await anonymous.call('GET', `login?return=${encodeURIComponent(back)}`);
      assert.equal(response.status, 400, back);
      assert.equal(response.json.error, 'bad_return', back);
    }
    const ok = await anonymous.call('GET', 'login?return=' + encodeURIComponent('/workshop/#entry-3'));
    assert.equal(ok.status, 302);
  } finally { await t.close(); }
});

test('session cookie: sealed round trip, tampering, purpose, expiry and attributes', async () => {
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
    const cookie = done.cookies.find((v) => v.startsWith('__Secure-workshop-session='));
    assert.match(cookie, /; Path=\/api\/workshop; Max-Age=2592000; HttpOnly; SameSite=Lax; Secure$/);
    assert.equal(done.headers['Cache-Control'], 'private, no-store');
    const value = c.jar.get('__Secure-workshop-session');
    c.jar.set('__Secure-workshop-session', value.slice(0, -3) + (value.endsWith('AAA') ? 'BBB' : 'AAA'));
    const tampered = await c.call('GET', 'me');
    assert.equal(tampered.status, 401);
    assert.ok(tampered.cookies.some((v) => v.startsWith('__Secure-workshop-session=;') && v.includes('Max-Age=0')));

    const d = t.client();
    await d.signIn('visitor-fake');
    t.clock.t += 31 * 24 * 3600_000;
    assert.equal((await d.call('GET', 'me')).status, 401, 'expired after 30 days');
  } finally { await t.close(); }
});

test('the user token is refreshed when under five minutes remain; a failed refresh clears the session', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const first = c.jar.get('__Secure-workshop-session');
    t.clock.t += 7 * 3600_000 + 56 * 60_000;
    const refreshed = await c.call('GET', 'me');
    assert.equal(refreshed.status, 200);
    assert.notEqual(c.jar.get('__Secure-workshop-session'), first);
    assert.equal(refreshed.headers['Cache-Control'], 'private, no-store');
    assert.equal((await c.call('POST', 'comment', { body: { n: 1, text: 'After refresh.' } })).status, 200);

    t.fake.state.refresh.clear();
    t.clock.t += 8 * 3600_000;
    const failed = await c.call('GET', 'me');
    assert.equal(failed.status, 401);
    assert.equal(c.jar.has('__Secure-workshop-session'), false);
  } finally { await t.close(); }
});

test('sign-in state cookie: mismatch, missing, expired and cancelled callbacks are handled', async () => {
  const t = await setup();
  try {
    const c = t.client();
    const start = await c.call('GET', 'login');
    assert.ok(start.cookies[0].startsWith('__Secure-workshop-signin='));
    assert.match(start.cookies[0], /Max-Age=600; HttpOnly; SameSite=Lax; Secure/);
    const authorize = new URL(start.headers.Location);
    assert.equal(authorize.searchParams.get('code_challenge_method'), 'S256');
    assert.equal(authorize.searchParams.get('redirect_uri'), `${ORIGIN}/api/workshop/callback`);
    const wrong = await c.call('GET', 'callback?code=abc&state=nope');
    assert.equal(wrong.status, 400);
    assert.equal(wrong.json.error, 'bad_state');
    assert.equal(c.jar.size, 0, 'the state cookie is cleared after any callback');

    const missing = await t.client().call('GET', `callback?code=abc&state=${authorize.searchParams.get('state')}`);
    assert.equal(missing.status, 400);

    const d = t.client();
    const again = new URL((await d.call('GET', 'login')).headers.Location);
    t.clock.t += 11 * 60_000;
    assert.equal((await d.call('GET', `callback?code=abc&state=${again.searchParams.get('state')}`)).status, 400, 'expired');

    const e = t.client();
    const third = new URL((await e.call('GET', 'login')).headers.Location);
    const denied = await e.call('GET', `callback?error=access_denied&state=${third.searchParams.get('state')}`);
    assert.equal(denied.status, 302);
    assert.equal(denied.headers.Location, '/workshop');
  } finally { await t.close(); }
});

test('logout clears the session and revokes the user token', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const tokens = t.fake.state.tokens.size;
    const out = await c.call('POST', 'logout', { body: {} });
    assert.equal(out.status, 200);
    assert.equal(c.jar.size, 0);
    assert.equal(t.fake.state.tokens.size, tokens - 1);
    assert.equal((await c.call('GET', 'me')).status, 401);
  } finally { await t.close(); }
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
    await visitor.call('POST', 'comment', { body: { n: 1, text: 'hi' } });
    await visitor.call('POST', 'vote', { body: { n: 1, on: true } });
    await visitor.call('POST', 'suggest', { body: { title: 'T', text: 'x' } });
    await owner.call('POST', 'verdict', { body: { n: 2, verdict: 'refine', note: 'n' } });
    await owner.call('POST', 'decide', { body: { n: 4, text: 'd' } });
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
    for (const line of t.logs) assert.deepEqual(Object.keys(JSON.parse(line)).filter((k) => !['endpoint', 'status', 'github', 'failure'].includes(k)), []);
  } finally { await t.close(); }
});

// Model: random operations by random people on random issues. After every step,
// only Workshop issues have changed, an entry carries at most one verdict label
// that matches the model, it is closed as not planned exactly when dropped, and
// decisions close only through decide by someone who can decide.
test('model: writes only reach Workshop issues and keep verdicts consistent', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const outside = [7, 8].map((n) => JSON.stringify(issueOf(t.fake, n)));
    const model = new Map([1, 2, 3].map((n) => [n, { verdict: ['take', null, 'later'][n - 1], dropped: false }]));
    const decisions = new Map([[4, 'open'], [5, 'closed']]);
    let seed = 12345;
    const rand = (k) => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed % k; };
    const numbers = [1, 2, 3, 4, 5, 6, 7, 8, 99];
    const verdicts = ['take', 'refine', 'later', 'drop', null];
    for (let step = 0; step < 150; step += 1) {
      const who = rand(2) ? owner : visitor;
      const isOwner = who === owner;
      const n = numbers[rand(numbers.length)];
      const op = rand(4);
      let response;
      if (op === 0) {
        const verdict = verdicts[rand(verdicts.length)];
        const note = verdict === 'refine' || rand(3) === 0 ? `note ${step}` : undefined;
        response = await who.call('POST', 'verdict', { body: { n, verdict, note } });
        if (isOwner && model.has(n)) {
          assert.equal(response.status, 200);
          model.set(n, { verdict, dropped: verdict === 'drop' });
        } else {
          assert.notEqual(response.status, 200);
        }
      } else if (op === 1) {
        response = await who.call('POST', 'decide', { body: { n, text: `decided ${step}` } });
        if (isOwner && decisions.get(n) === 'open') {
          assert.equal(response.status, 200);
          decisions.set(n, 'closed');
        } else {
          assert.notEqual(response.status, 200);
        }
      } else if (op === 2) {
        response = await who.call('POST', 'comment', { body: { n, text: `c ${step}` } });
        assert.equal(response.status === 200, n <= 6);
      } else {
        response = await who.call('POST', 'vote', { body: { n, on: rand(2) === 0 } });
        assert.equal(response.status === 200, n <= 6);
      }
      assert.deepEqual([7, 8].map((m) => JSON.stringify(issueOf(t.fake, m))), outside);
      for (const [m, expected] of model) {
        const issue = issueOf(t.fake, m);
        const labels = issue.labels.filter((l) => l.startsWith('verdict:'));
        assert.deepEqual(labels, expected.verdict ? [`verdict:${expected.verdict}`] : [], `entry ${m} at step ${step}`);
        assert.equal(issue.state === 'closed' && issue.state_reason === 'not_planned', expected.dropped);
        assert.ok(issue.labels.includes('workshop:entry'));
      }
      for (const [m, expected] of decisions) assert.equal(issueOf(t.fake, m).state, expected);
    }
  } finally { await t.close(); }
});

test('the Node adapter refuses bodies over 16 KB', async () => {
  const t = await setup();
  try {
    const handler = createNodeHandler(t.workshop);
    const req = Readable.from([Buffer.alloc(17 * 1024, 'a')]);
    Object.assign(req, { method: 'POST', url: '/api/workshop/comment', headers: { origin: ORIGIN, 'x-workshop': '1' } });
    const res = fakeResponse();
    await handler(req, res);
    assert.equal(res.statusCode, 413);
  } finally { await t.close(); }
});

function fakeResponse() {
  return {
    headers: {},
    setHeader(name, value) { this.headers[name.toLowerCase()] = value; },
    end(body) { this.body = body; this.ended = true; },
  };
}

async function fixtureSite(page = 'workshop/index.html') {
  const dir = await mkdtemp(join(tmpdir(), 'workshop-site-'));
  await mkdir(join(dir, 'dist', 'workshop'), { recursive: true });
  await writeFile(join(dir, 'dist', 'index.html'), '<!doctype html><title>Silo</title>');
  if (page) await writeFile(join(dir, 'dist', page), '<!doctype html><title>Workshop</title>');
  return dir;
}

test('Build Output layout: static site, one Node.js function at every endpoint, and the Workshop routes', async () => {
  const site = await fixtureSite();
  const outputDir = join(site, 'output');
  try {
    const website = new URL('..', import.meta.url).pathname;
    await prepareVercel({ siteDir: website, distDir: join(site, 'dist'), outputDir });
    const config = JSON.parse(await readFile(join(outputDir, 'config.json'), 'utf8'));
    assert.deepEqual(config, { version: 3, routes: [{ src: '^/workshop/?$', dest: '/workshop/index.html' }, { handle: 'filesystem' }] });
    for (const path of ['/workshop', '/workshop/']) assert.match(path, new RegExp(config.routes[0].src));
    assert.doesNotMatch('/workshops', new RegExp(config.routes[0].src));
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
    assert.deepEqual(vc, { runtime: RUNTIME, handler: 'api/workshop.cjs', launcherType: 'Nodejs', shouldAddHelpers: false, maxDuration: 15 });
    assert.match(vc.runtime, /^nodejs(22|24|26)\.x$/);
    const serverFiles = (await readdir(join(main, 'server'))).sort();
    assert.deepEqual(serverFiles, ['app-auth.mjs', 'config.mjs', 'github.mjs', 'node.mjs', 'session.mjs', 'workshop.mjs']);

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
      const result = await prepareVercel({ siteDir: website, distDir: join(flat, 'dist'), outputDir: join(flat, 'output') });
      assert.equal(result.config.routes[0].dest, '/workshop.html');
    } finally { await rm(flat, { recursive: true, force: true }); }
    const none = await fixtureSite(null);
    try {
      await assert.rejects(prepareVercel({ siteDir: website, distDir: join(none, 'dist'), outputDir: join(none, 'output') }), /no Workshop page/);
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
  const site = await fixtureSite();
  const dev = await startDev({ fake: true, port: await freePort(), distDir: join(site, 'dist'), log: () => {} });
  try {
    for (const path of ['/workshop', '/workshop/']) {
      const page = await fetch(dev.origin + path);
      assert.equal(page.status, 200);
      assert.equal(await page.text(), '<!doctype html><title>Workshop</title>');
    }
    assert.equal((await fetch(dev.origin + '/')).status, 200);
    assert.equal((await fetch(dev.origin + '/%2e%2e/package.json')).status, 404);
    const state = await (await fetch(dev.origin + '/api/workshop/state')).json();
    const entries = Object.values(state.issues).filter((i) => i.kind === 'entry');
    assert.ok(entries.length >= 3);
    assert.ok(entries.some((i) => i.entry === 'engine-service'));
    assert.ok(Object.values(state.issues).some((i) => i.kind === 'decision' && i.state === 'open'));

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
    const session = callback.headers.getSetCookie().find((c) => c.startsWith('workshop-session=')).split(';')[0];
    const me = await (await fetch(dev.origin + '/api/workshop/me', { headers: { cookie: session } })).json();
    assert.deepEqual([me.login, me.canDecide], ['owner-fake', true]);
    const comment = await fetch(dev.origin + '/api/workshop/comment', {
      method: 'POST',
      headers: { cookie: session, origin: dev.origin, 'x-workshop': '1', 'content-type': 'application/json' },
      body: JSON.stringify({ n: 1, text: 'From the dev server.' }),
    });
    assert.equal(comment.status, 200);
  } finally {
    await dev.close();
    await rm(site, { recursive: true, force: true });
  }
});
