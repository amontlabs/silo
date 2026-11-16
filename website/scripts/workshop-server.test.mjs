import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createPublicKey, createPrivateKey, generateKeyPairSync, randomBytes, verify } from 'node:crypto';
import { cp, lstat, mkdir, mkdtemp, readFile, readlink, readdir, rm, writeFile } from 'node:fs/promises';
import { connect as netConnect, createServer } from 'node:net';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createAppJwt } from '../server/app-auth.mjs';
import { readConfig } from '../server/config.mjs';
import { DEFAULT_TIMEOUT } from '../server/github.mjs';
import { createNodeHandler, MAX_BODY, readRequest } from '../server/node.mjs';
import { seal, unseal } from '../server/session.mjs';
import { BUDGET, createWorkshop, LABELS } from '../server/workshop.mjs';
import { CUTS, cut, readBody } from '../server/issues.mjs';
import { SNAPSHOT } from '../server/snapshot.mjs';
import { createFakeGitHub, FAKE_APP, progressOf, seedDemo } from './fake-github.mjs';
import { DEFAULT_OUTPUT, ENDPOINTS, prepareVercel, RUNTIME } from './prepare-vercel.mjs';
import { progressLine, startDev } from './workshop-dev.mjs';

import { count, decisionNumber, entryNumber, FAILED, issueOf, ORIGIN, SESSION, setup, START, withBudget } from './workshop-test-support.mjs';

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
      const visitor = t.client();
      await visitor.signIn('visitor-fake');
      t.fake.state.secondaryLimited = kind;
      const limited = await visitor.call('GET', 'me');
      assert.deepEqual([limited.status, limited.json.canDecide], [200, false], kind);
      assert.equal((await c.call('GET', 'state')).status, 502, 'a cold snapshot read while blocked');
      const calls = t.fake.state.requests.length;
      t.fake.state.secondaryLimited = false;
      assert.deepEqual((await visitor.call('GET', 'me')).json.canDecide, false);
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

test('a hung or failing token refresh answers 502 and keeps the session; only an invalid refresh token signs out', async () => {
  const t = await setup({ timeoutMs: 150 });
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const cookie = c.jar.get(SESSION);
    t.clock.t += 7 * 3600_000 + 56 * 60_000;
    t.fake.state.hook = (r) => (r.path === '/login/oauth/access_token' ? { delay: 600 } : null);
    let settled = false;
    const pending = c.call('GET', 'vote?n=1').then((response) => { settled = true; return response; });
    const other = await t.client().call('GET', 'state');
    assert.equal(other.status, 200);
    assert.equal(settled, false, 'other requests are not held by the refresh');
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
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a 401 on the installation token is a 502 and drops the token; a 401 on the person\'s token signs them out', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const tokens = count(t.fake, 'POST', /access_tokens$/);
    t.fake.state.hook = (r) => (r.kind === 'installation' && r.path.endsWith('/permission') ? { status: 401 } : null);
    const me = await c.call('GET', 'me');
    assert.deepEqual([me.status, me.json.canDecide], [200, false], 'me answers even when the permission cannot be read');
    assert.ok(c.jar.has(SESSION), 'still signed in');
    t.fake.state.hook = null;
    assert.equal((await c.call('GET', 'me')).status, 200);
    assert.equal(count(t.fake, 'POST', /access_tokens$/), tokens + 1, 'a new installation token');
    assert.equal(count(t.fake, 'GET', /permission$/), 2, 'a failed read is not cached');

    t.fake.state.hook = (r) => (r.kind === 'user' && r.method === 'POST' ? { status: 401 } : null);
    const comment = await c.call('POST', 'comment', { body: { n: 1, text: 'x' } });
    assert.equal(comment.status, 401);
    assert.equal(comment.json.error, 'signed_out');
    assert.equal(c.jar.has(SESSION), false);
    assert.equal(c.jar.has('workshop-hint'), false);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('me needs a session and reports canDecide from the repository permission', async () => {
  const t = await setup();
  try {
    t.fake.addUser('stranger-fake', { permission: 'none' });
    const anonymous = await t.client().call('GET', 'me');
    assert.equal(anonymous.status, 401);
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const me = await visitor.call('GET', 'me');
    assert.deepEqual(me.json, { login: 'visitor-fake', avatar: 'https://avatars.example.test/visitor-fake.png', canDecide: false });
    assert.equal(me.headers['Cache-Control'], 'private, no-store');
    const stranger = t.client();
    await stranger.signIn('stranger-fake');
    assert.equal((await stranger.call('GET', 'me')).json.canDecide, false);
    const owner = t.client();
    await owner.signIn('owner-fake');
    assert.equal((await owner.call('GET', 'me')).json.canDecide, true);
    const permissionReads = count(t.fake, 'GET', /permission$/);
    await owner.call('GET', 'me');
    assert.equal(count(t.fake, 'GET', /permission$/), permissionReads, 'cached for five minutes');
  } finally { await t.close(); }
});

test('GET vote reads the person\'s own reaction with their token, privately', async () => {
  const t = await setup();
  try {
    assert.equal((await t.client().call('GET', 'vote?n=1')).status, 401);
    const c = t.client();
    await c.signIn('visitor-fake');
    const on = await c.call('GET', 'vote?n=1');
    assert.deepEqual(on.json, { on: true });
    assert.equal(on.headers['Cache-Control'], 'private, no-store');
    assert.deepEqual((await c.call('GET', 'vote?n=2')).json, { on: false });
    assert.equal((await c.call('GET', 'vote?n=7')).status, 404);
    assert.equal(count(t.fake, 'GET', /reactions$/, 'installation'), 0);
  } finally { await t.close(); }
});

test('a signed-in person spends the installation token only on the permission check and labelling a suggestion', async () => {
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
    await visitor.call('POST', 'verdict', { body: { n: 1, verdict: 'take' } });
    await visitor.call('POST', 'verdict', { body: { n: 1, verdict: 'take' } });
    await visitor.call('POST', 'suggest', { body: { title: 'T', text: 'x' } });
    await owner.call('POST', 'verdict', { body: { n: 2, verdict: 'later' } });
    await owner.call('POST', 'decide', { body: { n: 4, text: 'd' } });
    await t.workshop.idle();
    const refreshes = /^\/repos\/amontlabs\/silo\/issues(\/comments)?$/;
    const installation = t.fake.state.requests.slice(before).filter((r) => r.kind === 'installation' && !(r.method === 'GET' && refreshes.test(r.path)));
    assert.deepEqual(installation.map((r) => r.path.replace(/\/\d+\//, '/N/')).sort(), [
      '/repos/amontlabs/silo/collaborators/owner-fake/permission',
      '/repos/amontlabs/silo/collaborators/visitor-fake/permission',
      '/repos/amontlabs/silo/issues/N/labels',
    ].sort(), 'each person\'s permission is read once and cached, yes or no');
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
    assert.deepEqual((await c.call('GET', 'vote?n=2')).json, { on: true });
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

test('verdict transitions change only the verdict labels, one by one, with the person\'s token', async () => {
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
      const before = t.fake.state.requests.length;
      issue.labels.push('added-meanwhile');
      const response = await c.call('POST', 'verdict', { body: { n: entryNumber, verdict } });
      assert.equal(response.status, 200, response.body);
      assert.deepEqual([...issue.labels].sort(), [...labels, 'added-meanwhile'].sort(), 'a label added by someone else survives');
      issue.labels = issue.labels.filter((l) => l !== 'added-meanwhile');
      assert.equal(issue.state, state);
      if (verdict === 'drop') assert.equal(issue.state_reason, 'not_planned');
      const calls = t.fake.state.requests.slice(before).filter((r) => r.method !== 'GET');
      assert.ok(calls.every((r) => r.kind === 'user' && r.login === 'owner-fake'));
      assert.ok(calls.every((r) => /\/labels(\/[^/]+)?$/.test(r.path) || r.method === 'PATCH'));
    }
    const refine = await c.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'refine', note: 'Narrow it to Linux.' } });
    assert.equal(refine.status, 200);
    assert.deepEqual(issue.comments.at(-1).user.login, 'owner-fake');
    assert.deepEqual(issue.comments.at(-1).body, 'Narrow it to Linux.');
    assert.ok(issue.labels.includes('verdict:refine'));
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
      [visitor, 'suggest', { title: 'two\nlines', text: 'y' }, {}, 400, 'bad_input'],
      [visitor, 'suggest', { title: 'bell\u0007', text: 'y' }, {}, 400, 'bad_input'],
      [visitor, 'suggest', { title: 'separator\u2028', text: 'y' }, {}, 400, 'bad_input'],
      [owner, 'verdict', { n: 1 }, {}, 400, 'bad_input'],
      [owner, 'verdict', { n: 1, verdict: 'refine' }, {}, 400, 'empty'],
      [owner, 'verdict', { n: 1, verdict: 'refine', note: 'x'.repeat(5001) }, {}, 400, 'too_long'],
      [owner, 'verdict', { n: 1, verdict: 'maybe' }, {}, 400, 'bad_input'],
      [owner, 'decide', { n: 4, text: '' }, {}, 400, 'empty'],
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
    for (const line of t.logs) assert.deepEqual(Object.keys(JSON.parse(line)).filter((k) => !['endpoint', 'status', 'github', 'failure', 'snapshot'].includes(k)), []);
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
    for (let step = 0; step < 200; step += 1) {
      t.clock.t += 3_000;
      const who = rand(2) ? owner : visitor;
      const isOwner = who === owner;
      const n = numbers[rand(numbers.length)];
      const op = rand(5);
      let response;
      if (op === 0) {
        const verdict = verdicts[rand(verdicts.length)];
        const note = verdict === 'refine' || rand(3) === 0 ? `note ${step}` : undefined;
        response = await who.call('POST', 'verdict', { body: { n, verdict, note } });
        if (isOwner && model.has(n)) {
          assert.equal(response.status, 200, response.body);
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
        assert.equal(response.status === 200, n <= 6, response.body);
      } else if (op === 3) {
        response = await who.call('POST', 'vote', { body: { n, on: rand(2) === 0 } });
        assert.equal(response.status === 200, n <= 6);
      } else {
        response = await who.call('GET', `vote?n=${n}`);
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

// The plan a local server's fake GitHub is seeded from in these tests.
const FIXTURE_FEATURES = [
  { n: 1, id: 'engine-service', name: 'The engine runs on its own', what: 'A background engine owns every computer.' },
  { n: 2, id: 'calls-and-events', name: 'Every call and event defined once', what: 'Each call is written down once.' },
  { n: 3, id: 'vm-runtime', name: 'The VM runtime, packaged', what: 'Silo ships the program that runs each computer.' },
  { n: 4, id: 'fixture-only', name: 'Only in the fixture', what: 'Proves the fixture is read.' },
].map((line) => JSON.stringify(line)).join('\n') + '\n';

async function fixtureSite(page = 'workshop/index.html') {
  const dir = await mkdtemp(join(tmpdir(), 'workshop-site-'));
  await mkdir(join(dir, 'dist', 'workshop'), { recursive: true });
  await writeFile(join(dir, 'dist', 'index.html'), '<!doctype html><title>Silo</title>');
  if (page) await writeFile(join(dir, 'dist', page), '<!doctype html><title>Workshop</title>');
  await writeFile(join(dir, 'features.jsonl'), FIXTURE_FEATURES);
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
      { src: '^/(.*)$', headers: { 'Content-Security-Policy': "frame-ancestors 'none'", 'X-Frame-Options': 'DENY' }, continue: true },
      { src: '^/workshop/?$', dest: '/workshop/index.html' },
      { handle: 'filesystem' },
    ] });
    for (const path of ['/', '/workshop', '/assets/x.js', '/api/workshop/state']) assert.match(path, new RegExp(config.routes[0].src));
    for (const path of ['/workshop', '/workshop/']) assert.match(path, new RegExp(config.routes[1].src));
    for (const path of ['/workshops', '/workshop/x', '/api/workshop/state']) assert.doesNotMatch(path, new RegExp(config.routes[1].src));
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
    assert.deepEqual(await readdir(join(main, 'node_modules')), ['@vercel']);
    assert.deepEqual(await readdir(join(main, 'node_modules', '@vercel')), ['functions'], 'only the package that loads');
    const shipped = await listFiles(join(main, 'node_modules'));
    assert.ok(shipped.length <= 20, `${shipped.length} files`);
    assert.ok(shipped.includes('@vercel/functions/wait-until.js') && shipped.includes('@vercel/functions/package.json'));

    // The packaged function runs on its own, from a clean folder with nothing above it to resolve from.
    const clean = await mkdtemp(join(tmpdir(), 'workshop-func-'));
    try {
      await cp(main, join(clean, 'state.func'), { recursive: true });
      const saved = Object.fromEntries(Object.keys(process.env).filter((k) => k.startsWith('WORKSHOP_')).map((k) => [k, process.env[k]]));
      for (const key of Object.keys(saved)) delete process.env[key];
      try {
        const entry = createRequire(import.meta.url)(join(clean, 'state.func', 'api', 'workshop.cjs'));
        const req = Object.assign(Readable.from([]), { method: 'GET', url: '/api/workshop/state', headers: {} });
        const res = fakeResponse();
        await entry(req, res);
        assert.equal(res.statusCode, 503);
        assert.equal(JSON.parse(res.body).error, 'not_configured');
      } finally {
        Object.assign(process.env, saved);
      }
    } finally { await rm(clean, { recursive: true, force: true }); }
    const serverFiles = (await readdir(join(main, 'server'))).sort();
    assert.deepEqual(serverFiles, ['app-auth.mjs', 'config.mjs', 'github.mjs', 'issues.mjs', 'node.mjs', 'session.mjs', 'snapshot.mjs', 'workshop.mjs']);

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
      assert.equal(result.config.routes[1].dest, '/workshop.html');
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
  const site = await fixtureSite();
  const dev = await startDev({ fake: true, port: await freePort(), distDir: join(site, 'dist'), featuresFile: join(site, 'features.jsonl'), log: () => {} });
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
    assert.deepEqual(entries.map((i) => i.entry).sort(), ['calls-and-events', 'engine-service', 'fixture-only', 'vm-runtime']);
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
    const cookies = callback.headers.getSetCookie();
    assert.ok(cookies.some((c) => c.startsWith('workshop-hint=1')));
    const session = cookies.find((c) => c.startsWith('workshop-session=')).split(';')[0];
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

test('me calls GitHub only for the permission check, cached, and never refreshes the token', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    t.clock.t += 7 * 3600_000 + 58 * 60_000;
    const before = t.fake.state.requests.length;
    assert.equal((await c.call('GET', 'me')).status, 200);
    assert.equal((await c.call('GET', 'me')).status, 200);
    assert.deepEqual(t.fake.state.requests.slice(before).filter((r) => r.kind !== 'app').map((r) => r.path), ['/user', '/repos/amontlabs/silo/collaborators/visitor-fake/permission'], 'the current login, then the permission, besides minting an installation token');
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
    assert.deepEqual((await c.call('GET', 'me')).json.canDecide, false);
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
    assert.equal(t.fake.state.requests.slice(before).filter((r) => r.kind === 'installation' && !r.path.endsWith('/permission')).length, 0, 'other requests never retry');
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

test('verdict: the note is posted only after labels and state succeeded; clearing never reopens a completed entry', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('owner-fake');
    const issue = issueOf(t.fake, entryNumber);
    const comments = issue.comments.length;
    t.fake.state.hook = (r) => (r.method === 'PATCH' ? { status: 500 } : null);
    const failed = await c.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'drop', note: 'Not now.' } });
    assert.equal(failed.status, 502);
    assert.equal(issue.comments.length, comments, 'no note after a failed change');
    t.fake.state.hook = null;
    assert.equal((await c.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'drop', note: 'Not now.' } })).status, 200);
    assert.equal(issue.comments.length, comments + 1);
    assert.equal(issue.comments.at(-1).body, 'Not now.');

    const built = issueOf(t.fake, 3);
    Object.assign(built, { state: 'closed', state_reason: 'completed' });
    for (const verdict of [null, 'take']) {
      assert.equal((await c.call('POST', 'verdict', { body: { n: 3, verdict } })).status, 200);
      assert.deepEqual([built.state, built.state_reason], ['closed', 'completed']);
    }
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('an issue with more than one Workshop kind label is not a Workshop issue', async () => {
  const t = await setup();
  try {
    const both = t.fake.addIssue({ title: 'Both', labels: ['workshop:entry', 'workshop:decision'] });
    const c = t.client();
    await c.signIn('visitor-fake');
    assert.equal((await c.call('GET', 'state')).json.issues[both.number], undefined);
    assert.equal((await c.call('POST', 'comment', { body: { n: both.number, text: 'x' } })).status, 404);
    assert.equal((await c.call('GET', `vote?n=${both.number}`)).status, 404);
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
  const site = await fixtureSite();
  const dev = await startDev({ fake: true, port: await freePort(), distDir: join(site, 'dist'), featuresFile: join(site, 'features.jsonl'), log: () => {} });
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

const PLAN = [
  ['e1', 'engine', 'shell', 'take'], ['e2', 'engine', 'shell', null], ['c1', 'computers', 'shell', 'take'],
  ['p1', 'app', 'loop', 'take'], ['a1', 'agents', 'agent', 'take'], ['a2', 'agents', 'agent', 'later'],
  ['a3', 'agents', 'agent', 'take'], ['a4', 'agents', 'living', 'take'], ['x1', 'changes', 'loop2', 'take'],
  ['g1', undefined, 'gone', 'drop'],
].map(([id, area, stage, verdict], index) => ({ n: index + 1, id, area, stage, verdict, name: id, what: id }));

test('progress: taken entries of engine, computers and app built, two taken agents entries being built', async () => {
  const plan = progressOf(PLAN);
  assert.equal(plan.by, 'area');
  assert.deepEqual([...plan.built], ['e1', 'c1', 'p1']);
  assert.deepEqual([...plan.building], ['a1', 'a3']);
  assert.match(progressLine(plan), /^built: the taken entries of the areas engine, computers, app \(3\); being built: 2 of agents\.$/);

  const t = await setup({ seed: false });
  try {
    seedDemo(t.fake, PLAN, { progress: true });
    const issues = Object.values((await t.client().call('GET', 'state')).json.issues).filter((i) => i.kind === 'entry');
    const by = Object.fromEntries(issues.map((i) => [i.entry, i]));
    for (const id of ['e1', 'c1', 'p1']) assert.deepEqual([by[id].state, by[id].reason], ['closed', 'completed'], id);
    assert.deepEqual(issues.filter((i) => i.building).map((i) => i.entry).sort(), ['a1', 'a3']);
    assert.deepEqual([by.a2.verdict, by.a2.state, by.e2.verdict, by.x1.state], ['later', 'open', null, 'open']);
    assert.deepEqual([by.g1.verdict, by.g1.reason], ['drop', 'not_planned']);
  } finally { await t.close(); }
});

test('progress without areas goes by stage: the first three built, two taken of the fourth being built', () => {
  const plan = progressOf(PLAN.map(({ area, ...rest }) => rest));
  assert.equal(plan.by, 'stage');
  assert.deepEqual(plan.builtGroups, ['shell', 'loop', 'agent']);
  assert.deepEqual([...plan.built], ['e1', 'c1', 'p1', 'a1', 'a3']);
  assert.deepEqual([...plan.building], ['a4']);
  assert.match(progressLine(plan), /^No entry has an area, so progress goes by stage: .*stages shell, loop, agent/);
});

test('without progress the demo seed keeps its own verdicts and no entry is being built', async () => {
  const t = await setup({ seed: false });
  try {
    assert.equal(seedDemo(t.fake, PLAN).progress, null);
    const issues = Object.values((await t.client().call('GET', 'state')).json.issues);
    assert.equal(issues.some((i) => Object.hasOwn(i, 'building')), false);
    assert.equal(issues.filter((i) => i.kind === 'entry' && i.state === 'closed').length, 0);
  } finally { await t.close(); }
});
