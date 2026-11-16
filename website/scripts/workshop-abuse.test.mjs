import { test } from 'node:test';
import assert from 'node:assert/strict';
import { setFlagsFromString } from 'node:v8';
import { runInNewContext } from 'node:vm';
import { SNAPSHOT } from '../server/snapshot.mjs';
import { BUDGET, createWorkshop } from '../server/workshop.mjs';
import { count, decisionNumber, entryNumber, FAILED, issueOf, ORIGIN, SESSION, setup, withBudget } from './workshop-test-support.mjs';

const LIST = /^\/repos\/amontlabs\/silo\/issues$/;
const lists = (t) => t.fake.state.requests.filter((r) => r.method === 'GET' && LIST.test(r.path) && r.kind === 'installation').length;
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function withSnapshot(values, run) {
  const saved = { ...SNAPSHOT };
  Object.assign(SNAPSHOT, values);
  try {
    await run();
  } finally {
    Object.assign(SNAPSHOT, saved);
  }
}

function variant(t, options) {
  return createWorkshop({ config: t.config, now: () => t.clock.t, log: () => {}, ...options });
}

async function signInMany(t, prefix, n) {
  const clients = [];
  for (let i = 0; i < n; i += 1) {
    t.fake.addUser(`${prefix}${i}`, { permission: 'read' });
    const c = t.client();
    await c.signIn(`${prefix}${i}`);
    clients.push(c);
  }
  return clients;
}

test('100 verdict posts from visitors, then 60 verdicts from the owner in one minute: all of the owner\'s succeed', async () => {
  const t = await setup();
  try {
    const visitors = await signInMany(t, 'visitor', 5);
    for (const visitor of visitors) {
      for (let i = 0; i < 20; i += 1) {
        const response = await visitor.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'take' } });
        assert.ok([403, 429].includes(response.status), response.body);
      }
    }
    assert.equal(count(t.fake, 'GET', /permission$/), 5, 'one read per visitor; "no" is cached');
    const owner = t.client();
    await owner.signIn('owner-fake');
    for (let i = 0; i < 60; i += 1) {
      const response = await owner.call('POST', 'verdict', { body: { n: entryNumber, verdict: i % 2 ? 'take' : 'later' } });
      assert.equal(response.status, 200, `verdict ${i}: ${response.body}`);
    }
    assert.equal(count(t.fake, 'GET', /owner-fake\/permission$/), 1, 'no re-read per verdict');
  } finally { await t.close(); }
});

test('a known decider re-reads on an allowance of its own; a GitHub refusal drops the cached yes', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    assert.equal((await owner.call('GET', 'me')).json.canDecide, true);
    t.clock.t += 6 * 60_000;
    const visitors = await signInMany(t, 'reader', BUDGET.permissionChecksPerMinute + 3);
    for (const visitor of visitors) await visitor.call('GET', 'me');
    assert.equal((await owner.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'take' } })).status, 200, 'the general allowance is spent, the owner\'s is not');

    t.fake.state.users.get('owner-fake').permission = 'read';
    const refused = await owner.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'later' } });
    assert.equal(refused.status, 403, 'GitHub refuses the label change');
    assert.equal((await owner.call('GET', 'me')).json.canDecide, false, 'read again after the refusal');
  } finally { await t.close(); }
});

test('background work is handed to keepAlive; a lost refresh is replaced at once; reads and writes wait for their refresh briefly', async () => {
  const t = await setup();
  const kept = [];
  const workshop = variant(t, { keepAlive: (promise) => kept.push(promise) });
  const call = (method, path, body, cookie) => workshop.handle({
    method, url: `/api/workshop/${path}`, body: body ? JSON.stringify(body) : '',
    headers: method === 'POST' ? { origin: ORIGIN, 'x-workshop': '1', cookie } : { cookie },
  });
  try {
    await call('GET', 'state');
    assert.ok(kept.length >= 1, 'the cold refresh is kept alive');

    await withSnapshot({ ageWait: 1500, writeWait: 2000, lostAfter: 200 }, async () => {
      t.clock.t += 31_000;
      t.fake.addComment(3, 'owner-fake', 'Seen by the reader that waited.');
      const fresh = JSON.parse((await call('GET', 'issue?n=3')).body);
      assert.equal(fresh.comments.at(-1)?.text, 'Seen by the reader that waited.', 'the read waited for the refresh it started');

      t.clock.t += 31_000;
      t.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 1000 } : null);
      const before = lists(t);
      await call('GET', 'state');
      await wait(300);
      t.fake.state.hook = null;
      t.clock.t += 31_000;
      await call('GET', 'state');
      assert.equal(lists(t), before + 2, 'the lost refresh was replaced without the 60-second wait');
      await workshop.idle();

      const owner = t.client();
      await owner.signIn('owner-fake');
      const cookie = [...owner.jar].map(([k, v]) => `${k}=${v}`).join('; ');
      t.fake.addComment(5, 'visitor-fake', 'Made on GitHub just before the write.');
      const keptBefore = kept.length;
      const write = await call('POST', 'comment', { n: 1, text: 'A write that waits.' }, cookie);
      assert.equal(write.status, 200);
      assert.ok(kept.length > keptBefore, 'the follow-up refresh is kept alive');
      const issue = JSON.parse((await call('GET', 'issue?n=5')).body);
      assert.equal(issue.comments.at(-1).text, 'Made on GitHub just before the write.', 'the write\'s answer came after its refresh');
    });
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a sign-in cookie works once, and token exchanges have a ceiling', async () => {
  const t = await setup();
  try {
    const c = t.client();
    const start = await c.call('GET', 'login');
    const state = new URL(start.headers.Location).searchParams.get('state');
    const cookie = c.jar.get('__Host-workshop-signin');
    const exchanges = () => count(t.fake, 'POST', /^\/login\/oauth\/access_token$/);
    for (let i = 0; i < 20; i += 1) {
      const replay = await t.workshop.handle({ method: 'GET', url: `/api/workshop/callback?code=made-up-${i}&state=${state}`, headers: { cookie: `__Host-workshop-signin=${cookie}` }, body: '' });
      assert.equal(replay.headers.Location, FAILED);
    }
    assert.equal(exchanges(), 1, 'the cookie was used once');

    for (let i = 0; i < BUDGET.tokenExchangesPerMinute + 10; i += 1) {
      const d = t.client();
      const s = new URL((await d.call('GET', 'login')).headers.Location).searchParams.get('state');
      await d.call('GET', `callback?code=made-up&state=${s}`);
    }
    assert.ok(exchanges() <= BUDGET.tokenExchangesPerMinute, `${exchanges()} exchanges`);
  } finally { await t.close(); }
});

test('a session GitHub rejected is answered 401 without calling GitHub; each session has a read ceiling', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const old = c.jar.get(SESSION);
    for (const [value, entry] of t.fake.state.tokens) if (entry.login === 'visitor-fake') t.fake.state.tokens.delete(value);
    assert.equal((await c.call('GET', 'vote?n=1')).status, 401);
    const before = t.fake.state.requests.length;
    for (let i = 0; i < 50; i += 1) {
      c.jar.set(SESSION, old);
      assert.equal((await c.call(i % 2 ? 'GET' : 'POST', i % 2 ? 'vote?n=1' : 'vote', i % 2 ? {} : { body: { n: 1, on: true } })).status, 401);
    }
    c.jar.set(SESSION, old);
    await c.call('POST', 'logout', { body: {} });
    assert.equal(t.fake.state.requests.length, before, 'no GitHub call for a rejected session');

    const reader = t.client();
    await reader.signIn('owner-fake');
    let limited = 0;
    for (let i = 0; i < 40; i += 1) if ((await reader.call('GET', 'vote?n=1')).status === 429) limited += 1;
    assert.ok(limited > 0);
    assert.ok(count(t.fake, 'GET', /\/issues\/1(\/reactions)?$/, 'user') <= BUDGET.sessionReadsPerMinute + 1);
  } finally { await t.close(); }
});

test('suggestions: 3 a minute and 10 an hour per person; the label ceiling is 20 a minute and charged per call', async () => {
  assert.equal(BUDGET.labelCallsPerMinute, 20);
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    const statuses = [];
    for (let i = 0; i < 4; i += 1) statuses.push((await c.call('POST', 'suggest', { body: { title: `S${i}`, text: 'x' } })).status);
    assert.deepEqual(statuses, [200, 200, 200, 429]);
    for (let minute = 1; minute <= 4; minute += 1) {
      t.clock.t += 61_000;
      for (let i = 0; i < 3; i += 1) statuses.push((await c.call('POST', 'suggest', { body: { title: `M${minute}-${i}`, text: 'x' } })).status);
    }
    assert.equal(statuses.filter((s) => s === 200).length, 10, 'ten an hour');
    const other = t.client();
    await other.signIn('owner-fake');
    assert.equal((await other.call('POST', 'suggest', { body: { title: 'Someone else', text: 'x' } })).status, 200, 'per person');
  } finally { await t.close(); }
});

test('nothing raw is kept: 1,000 foreign issues and 1,000 large foreign comments add under 5 MB to the heap', async () => {
  setFlagsFromString('--expose_gc');
  const gc = runInNewContext('gc');
  const t = await setup();
  try {
    for (let i = 0; i < 1000; i += 1) {
      // Flat strings, so that the fake's own data does not grow when it first serializes them.
      const issue = t.fake.addIssue({ title: `Foreign ${i}`, body: Buffer.alloc(10_000, 'f').toString(), author: 'visitor-fake' });
      t.fake.addComment(issue.number, 'visitor-fake', Buffer.alloc(60_000, 'c').toString());
    }
    t.fake.state.commentsPerPage = 100;
    gc(); gc();
    const before = process.memoryUsage().heapUsed;
    const state = await t.client().call('GET', 'state');
    assert.equal(Object.keys(state.json.issues).length, 6);
    await t.workshop.idle();
    gc(); gc();
    const added = process.memoryUsage().heapUsed - before;
    assert.ok(added < 5 * 1024 * 1024, `${(added / 1024 / 1024).toFixed(1)} MB`);
  } finally { await t.close(); }
});

test('a handler never waits past its deadline on shared work such as an installation token fetch', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    const cookie = [...owner.jar].map(([k, v]) => `${k}=${v}`).join('; ');
    const quick = variant(t, { deadlineMs: 300 });
    t.fake.state.hook = (r) => (r.path.endsWith('/access_tokens') ? { delay: 1500 } : null);
    quick.handle({ method: 'GET', url: '/api/workshop/state', headers: {}, body: '' }).catch(() => {});
    const started = Date.now();
    const me = await quick.handle({ method: 'GET', url: '/api/workshop/me', headers: { cookie }, body: '' });
    assert.ok(Date.now() - started < 700, `${Date.now() - started} ms`);
    assert.deepEqual([me.status, JSON.parse(me.body).canDecide], [200, false]);
    await wait(1600);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('no refresh starts inside GitHub\'s wait after a failure, whatever asks for it', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    await t.client().call('GET', 'state');
    t.clock.t += 31_000;
    t.fake.state.hook = (r) => (LIST.test(r.path) ? { status: 500 } : null);
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    t.fake.state.hook = null;
    const failedAt = lists(t);
    for (let i = 0; i < 5; i += 1) {
      t.clock.t += 10_000;
      await c.call('POST', 'comment', { body: { n: 1, text: `inside the wait ${i}` } });
      await t.client().call('GET', 'state');
      await t.workshop.idle();
    }
    assert.equal(lists(t), failedAt, 'no refresh within the 60 seconds');
    t.clock.t += 11_000;
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    assert.equal(lists(t), failedAt + 1);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a label that failed five times leaves the retry list', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    await t.client().call('GET', 'state');
    t.fake.state.hook = (r) => (r.kind === 'installation' && r.method === 'POST' && r.path.endsWith('/labels') ? { status: 500 } : null);
    assert.equal((await c.call('POST', 'suggest', { body: { title: 'Never labelled', text: 'x' } })).status, 502);
    await t.workshop.idle();
    for (let i = 0; i < 7; i += 1) {
      t.clock.t += 61_000;
      await t.client().call('GET', 'state');
      await t.workshop.idle();
    }
    const attempts = count(t.fake, 'POST', /\/labels$/, 'installation');
    assert.ok(attempts <= 2 + 5, `${attempts} label calls`);
    t.clock.t += 61_000;
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    assert.equal(count(t.fake, 'POST', /\/labels$/, 'installation'), attempts, 'dropped after five failures');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a comment or a verdict note retried within five minutes is posted once', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    for (let i = 0; i < 2; i += 1) assert.equal((await owner.call('POST', 'comment', { body: { n: 1, text: 'Only once.' } })).status, 200);
    assert.equal(issueOf(t.fake, 1).comments.filter((c) => c.body === 'Only once.').length, 1);
    for (let i = 0; i < 2; i += 1) assert.equal((await owner.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'refine', note: 'Narrow it.' } })).status, 200);
    assert.equal(issueOf(t.fake, entryNumber).comments.filter((c) => c.body === 'Narrow it.').length, 1);
    t.clock.t += 6 * 60_000;
    assert.equal((await owner.call('POST', 'comment', { body: { n: 1, text: 'Only once.' } })).status, 200);
    assert.equal(issueOf(t.fake, 1).comments.filter((c) => c.body === 'Only once.').length, 2, 'after five minutes it is a new comment');
  } finally { await t.close(); }
});

test('a request that changed GitHub and then failed still starts the refresh', async () => {
  const t = await setup();
  try {
    const owner = t.client();
    await owner.signIn('owner-fake');
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    let before = lists(t);
    t.fake.state.hook = (r) => (r.method === 'POST' && r.path.endsWith(`/issues/${entryNumber}/comments`) ? { status: 500 } : null);
    assert.equal((await owner.call('POST', 'verdict', { body: { n: entryNumber, verdict: 'drop', note: 'Not now.' } })).status, 502);
    await t.workshop.idle();
    assert.equal(lists(t), before + 1, 'the label change was refreshed');
    before = lists(t);
    t.fake.state.hook = (r) => (r.method === 'PATCH' ? { status: 500 } : null);
    assert.equal((await owner.call('POST', 'decide', { body: { n: decisionNumber, text: 'Debian.' } })).status, 502);
    await t.workshop.idle();
    assert.equal(lists(t), before + 1, 'the posted comment was refreshed');
  } finally { t.fake.state.hook = null; await t.close(); }
});
