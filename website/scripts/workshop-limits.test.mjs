import { test } from 'node:test';
import assert from 'node:assert/strict';
import { BUDGET, DEADLINE } from '../server/workshop.mjs';
import { createWorkshop } from '../server/workshop.mjs';
import { count, entryNumber, issueOf, ORIGIN, SESSION, setup, withBudget } from './workshop-test-support.mjs';

const LIST = /^\/repos\/amontlabs\/silo\/issues$/;
const COMMENTS = /^\/repos\/amontlabs\/silo\/issues\/comments$/;

// A workshop on the same fake with other options (a short deadline).
function variant(t, options) {
  return createWorkshop({ config: t.config, now: () => t.clock.t, log: () => {}, ...options });
}

test('anonymous reads cannot make a signed-in person\'s request answer 429', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    await withBudget({ refreshCallsPerMinute: 2 }, async () => {
      for (let i = 0; i < 200; i += 1) {
        if (i % 20 === 0) t.clock.t += 61_000;
        await t.client().call('GET', i % 2 ? 'state' : 'issue?n=1');
        if (i % 20 === 0) await t.workshop.idle();
      }
      const me = await owner.call('GET', 'me');
      assert.deepEqual([me.status, me.json.login], [200, 'owner-fake']);
      for (const [who, path, body] of [
        [visitor, 'comment', { n: 1, text: 'still here' }],
        [visitor, 'vote', { n: 1, on: true }],
        [visitor, 'suggest', { title: 'Still possible', text: 'x' }],
        [owner, 'comment', { n: entryNumber, text: 'the owner too' }],
      ]) {
        const response = await who.call('POST', path, { body });
        assert.equal(response.status, 200, `${path} ${response.body}`);
      }
    });
  } finally { await t.close(); }
});

test('me answers the login and the avatar and nothing else; a 401 clears the hint cookie', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('owner-fake');
    assert.deepEqual((await c.call('GET', 'me')).json, { login: 'owner-fake', avatar: 'https://avatars.example.test/owner-fake.png' });

    const hinted = t.client();
    hinted.jar.set('workshop-hint', '1');
    const out = await hinted.call('GET', 'me');
    assert.equal(out.status, 401);
    assert.ok(out.cookies.some((v) => v.startsWith('workshop-hint=;') && v.includes('Max-Age=0')));
    assert.equal(hinted.jar.has('workshop-hint'), false);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('sign-out reports whether GitHub revoked the grant, always clears the cookies, never refreshes, and counts as a write', async () => {
  const t = await setup();
  try {
    const ok = t.client();
    await ok.signIn('visitor-fake');
    const done = await ok.call('POST', 'logout', { body: {} });
    assert.deepEqual(done.json, { ok: true, revoked: true });

    const failing = t.client();
    await failing.signIn('visitor-fake');
    t.fake.state.hook = (r) => (r.path.endsWith('/grant') ? { status: 500 } : null);
    const failed = await failing.call('POST', 'logout', { body: {} });
    assert.deepEqual(failed.json, { ok: true, revoked: false });
    assert.equal(failing.jar.size, 0, 'cookies cleared anyway');
    t.fake.state.hook = null;

    const before = t.fake.state.requests.length;
    const nobody = await t.client().call('POST', 'logout', { body: {} });
    assert.deepEqual(nobody.json, { ok: true, revoked: false });
    assert.equal(t.fake.state.requests.length, before, 'no session, no GitHub call');

    const expiring = t.client();
    await expiring.signIn('visitor-fake');
    t.clock.t += 7 * 3600_000 + 58 * 60_000;
    const tokens = count(t.fake, 'POST', /access_token$/);
    await expiring.call('POST', 'logout', { body: {} });
    assert.equal(count(t.fake, 'POST', /access_token$/), tokens, 'sign-out never waits on a refresh');

    await withBudget({ writesPerMinute: 1 }, async () => {
      const busy = t.client();
      await busy.signIn('owner-fake');
      assert.equal((await busy.call('POST', 'comment', { body: { n: 1, text: 'one' } })).status, 200);
      const limited = await busy.call('POST', 'logout', { body: {} });
      assert.equal(limited.status, 429);
      assert.equal(busy.jar.has(SESSION), false, 'the cookies are cleared even then');
    });
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('every handler has one deadline for all its GitHub calls, and answers 502 when it passes', async () => {
  assert.equal(DEADLINE, 12_000);
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    const quick = variant(t, { deadlineMs: 400 });
    const cookie = [...owner.jar].map(([k, v]) => `${k}=${v}`).join('; ');
    t.fake.state.hook = (r) => (r.kind === 'user' ? { delay: 250 } : null);
    const started = Date.now();
    const response = await quick.handle({
      method: 'POST', url: '/api/workshop/comment', headers: { origin: ORIGIN, 'x-workshop': '1', cookie }, body: JSON.stringify({ n: entryNumber, text: 'Too slow.' }),
    });
    assert.equal(response.status, 502);
    assert.equal(JSON.parse(response.body).error, 'github_failed');
    assert.ok(Date.now() - started < 1200, `${Date.now() - started} ms`);
    await new Promise((resolve) => setTimeout(resolve, 600));

    t.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 700 } : null);
    const cold = await quick.handle({ method: 'GET', url: '/api/workshop/state', headers: {}, body: '' });
    assert.equal(cold.status, 502, 'a cold read waits no longer than the deadline');
    await quick.idle();
    const warm = await quick.handle({ method: 'GET', url: '/api/workshop/state', headers: {}, body: '' });
    assert.equal(warm.status, 200, 'the refresh went on in the background');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('patches survive a refresh already in flight and are not counted twice', async () => {
  const t = await setup();
  try {
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const reader = t.client();
    await reader.call('GET', 'state');
    t.clock.t += 61_000;
    t.fake.state.hook = (r) => (COMMENTS.test(r.path) ? { delay: 300 } : null);
    await reader.call('GET', 'state');
    assert.equal((await visitor.call('POST', 'comment', { body: { n: 3, text: 'During the refresh.' } })).status, 200);
    assert.equal((await visitor.call('POST', 'vote', { body: { n: 3, on: true } })).status, 200);
    await t.workshop.idle();
    t.fake.state.hook = null;
    let state = (await reader.call('GET', 'state')).json.issues;
    assert.deepEqual([state[3].comments, state[3].up], [1, 1], 'kept through the swap, not doubled');
    assert.equal((await reader.call('GET', 'issue?n=3')).json.comments.at(-1).text, 'During the refresh.');

    assert.equal((await visitor.call('POST', 'comment', { body: { n: 3, text: 'Before the next.' } })).status, 200);
    t.clock.t += 61_000;
    await reader.call('GET', 'state');
    await t.workshop.idle();
    state = (await reader.call('GET', 'state')).json.issues;
    assert.equal(state[3].comments, 2, 'a patch older than the refresh gives way to GitHub');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('titles, bodies, comments and logins are cleaned on the way out', async () => {
  const t = await setup();
  try {
    t.fake.addUser('mal\u202Eory');
    const issue = t.fake.addIssue({ title: 'Safe\u202E title\nsecond line', body: 'x\u0007y\u2066z', labels: ['workshop:suggestion'], author: 'mal\u202Eory' });
    t.fake.addComment(issue.number, 'mal\u202Eory', 'p\u2067q\u0000r\nkept line');
    const state = (await t.client().call('GET', 'state')).json.issues[issue.number];
    assert.deepEqual([state.title, state.author], ['Safe title second line', 'malory']);
    const answer = (await t.client().call('GET', `issue?n=${issue.number}`)).json;
    assert.equal(answer.title, 'Safe title second line');
    assert.equal(answer.body, 'xyz');
    assert.deepEqual([answer.comments[0].text, answer.comments[0].author], ['pqr\nkept line', 'malory']);
  } finally { await t.close(); }
});

test('state answers 503 not_configured when the App is not installed on the repository', async () => {
  const t = await setup();
  try {
    t.fake.state.hook = (r) => (r.path.endsWith('/installation') ? { status: 404 } : null);
    const response = await t.client().call('GET', 'state');
    assert.equal(response.status, 503);
    assert.equal(response.json.error, 'not_configured');
    assert.equal((await t.client().call('GET', 'issue?n=1')).status, 503, 'until the retry time, the same answer');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('the installation ceilings are separate and sum to under 5,000 calls an hour', () => {
  const perMinute = BUDGET.refreshCallsPerMinute + BUDGET.signinLookupsPerMinute + BUDGET.labelCallsPerMinute;
  assert.ok(perMinute * 60 < 5000, `${perMinute * 60}`);
});
