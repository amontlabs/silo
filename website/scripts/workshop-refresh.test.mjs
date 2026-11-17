import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SNAPSHOT } from '../server/snapshot.mjs';
import { setup } from './workshop-test-support.mjs';

const LIST = /^\/repos\/amontlabs\/silo\/issues$/;
// One per refresh: the first page of the entries list, which every refresh reads first.
const lists = (t) => t.fake.state.requests.filter((r) => r.method === 'GET' && LIST.test(r.path) && r.kind === 'installation' && r.query.includes('workshop%3Aentry') && /[?&]page=1$/.test(r.query));
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function withGap(ms, run) {
  const saved = SNAPSHOT.writeGap;
  SNAPSHOT.writeGap = ms;
  try {
    await run();
  } finally {
    SNAPSHOT.writeGap = saved;
  }
}

test('a write starts a refresh from GitHub at once, without waiting for the snapshot to age', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    await t.client().call('GET', 'state');
    await wait(SNAPSHOT.writeGap + 10);
    const before = lists(t).length;
    t.fake.addComment(3, 'owner-fake', 'Written on GitHub directly.');
    assert.equal((await c.call('POST', 'vote', { body: { n: 1, on: true } })).status, 200);
    await t.workshop.idle();
    assert.equal(lists(t).length, before + 1, 'one refresh, with the clock not moved');
    const issue = (await t.client().call('GET', 'issue?n=3')).json;
    assert.equal(issue.comments.at(-1).text, 'Written on GitHub directly.', 'the snapshot came from GitHub');
  } finally { await t.close(); }
});

test('ten writes in a burst start at most one running and one waiting refresh, and the last write is in the final snapshot', async () => {
  await withGap(300, async () => {
    const t = await setup();
    try {
      const c = t.client();
      await c.signIn('visitor-fake');
      await t.client().call('GET', 'state');
      await wait(310);
      const before = lists(t).length;
      const answers = await Promise.all(Array.from({ length: 10 }, (_, i) => c.call('POST', 'comment', { body: { n: 1, text: `burst ${i}` } })));
      assert.ok(answers.every((answer) => answer.status === 200));
      const lastWrite = t.fake.state.requests.findLast((r) => r.method === 'POST' && r.path.endsWith('/issues/1/comments'));
      await t.workshop.idle();
      const started = lists(t).slice(before);
      assert.ok(started.length >= 1 && started.length <= 2, `${started.length} refreshes`);
      assert.ok(started.at(-1).wall >= lastWrite.wall, 'the final refresh began after the last write');
      t.clock.t += 1;
      const texts = (await t.client().call('GET', 'issue?n=1')).json.comments.map((x) => x.text);
      assert.equal(texts.filter((x) => x.startsWith('burst ')).length, 10);
      assert.equal((await t.client().call('GET', 'state')).json.issues[1].comments, 12);
    } finally { await t.close(); }
  });
});

test('two refresh starts asked for by writes are never closer than the floor, 5 seconds by default', async () => {
  assert.equal(SNAPSHOT.writeGap, 50, 'the tests\' own floor');
  await withGap(400, async () => {
    const t = await setup();
    try {
      const c = t.client();
      await c.signIn('visitor-fake');
      await t.client().call('GET', 'state');
      for (let i = 0; i < 6; i += 1) {
        await c.call('POST', 'comment', { body: { n: 1, text: `c${i}` } });
        await wait(120);
      }
      await t.workshop.idle();
      const starts = lists(t).map((r) => r.wall);
      assert.ok(starts.length >= 2);
      for (let i = 1; i < starts.length; i += 1) assert.ok(starts[i] - starts[i - 1] >= 390, `gap ${starts[i] - starts[i - 1]} ms`);
    } finally { await t.close(); }
  });
  const { SNAPSHOT: fresh } = await import(`../server/snapshot.mjs?default=${Date.now()}`);
  assert.equal(fresh.writeGap, 5000);
  assert.deepEqual([fresh.ageWait, fresh.freshWait, fresh.writeWait, fresh.lostAfter], [1500, 8000, 2000, 15_000]);
});

test('with no write, an instance refreshes once its snapshot is over 30 seconds old', async () => {
  assert.equal(SNAPSHOT.maxAge, 30_000);
  const t = await setup();
  try {
    await t.client().call('GET', 'state');
    const cold = lists(t).length;
    t.clock.t += 29_000;
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    assert.equal(lists(t).length, cold);
    t.clock.t += 2_000;
    await wait(SNAPSHOT.writeGap + 10);
    await t.client().call('GET', 'state');
    await t.workshop.idle();
    assert.equal(lists(t).length, cold + 1);
  } finally { await t.close(); }
});

const COMMENTS = /^\/repos\/amontlabs\/silo\/issues\/comments$/;
const STATE_READ = 'public, s-maxage=10, stale-while-revalidate=86400';
const FRESH_READ = 'public, s-maxage=10';

async function withSnapshot(values, run) {
  const saved = { ...SNAPSHOT };
  Object.assign(SNAPSHOT, values);
  try {
    await run();
  } finally {
    Object.assign(SNAPSHOT, saved);
  }
}

// Calls `path` and answers the response with how long it took on the real clock.
async function timed(c, path) {
  const started = Date.now();
  const response = await c.call('GET', path);
  return { ...response, ms: Date.now() - started };
}

test('state answers at once from an old snapshot and says it is old; a fresh snapshot carries no old', async () => {
  await withSnapshot({ ageWait: 1500 }, async () => {
    const t = await setup();
    try {
      const c = t.client();
      await c.call('GET', 'state');
      const young = await c.call('GET', 'state');
      assert.equal(Object.hasOwn(young.json, 'old'), false);
      assert.equal(young.headers['Cache-Control'], STATE_READ);
      const cold = lists(t).length;
      t.clock.t += SNAPSHOT.maxAge + 1000;
      t.fake.addComment(3, 'owner-fake', 'Seen after the refresh.');
      t.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 400 } : null);
      const old = await timed(c, 'state');
      assert.ok(old.ms < 250, `answered in ${old.ms} ms`);
      assert.equal(old.json.old, true);
      assert.equal(old.json.issues[3].comments, 0, 'the snapshot it had');
      assert.equal(old.headers['Cache-Control'], STATE_READ);
      await t.workshop.idle();
      assert.equal(lists(t).length, cold + 1, 'it started a refresh');
      t.fake.state.hook = null;
      const after = await c.call('GET', 'state');
      assert.equal(Object.hasOwn(after.json, 'old'), false);
      assert.equal(after.json.issues[3].comments, 1);
      assert.equal(lists(t).length, cold + 1);
    } finally { t.fake.state.hook = null; await t.close(); }
  });
});

test('state?fresh=1 waits for the refresh it starts or joins and answers the new snapshot', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.call('GET', 'state');
    const cold = lists(t).length;
    const young = await c.call('GET', 'state?fresh=1');
    assert.equal(lists(t).length, cold, 'no refresh on a fresh snapshot');
    assert.equal(Object.hasOwn(young.json, 'old'), false);
    assert.equal(young.headers['Cache-Control'], FRESH_READ);

    t.clock.t += SNAPSHOT.maxAge + 1000;
    t.fake.addComment(3, 'owner-fake', 'First.');
    t.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 300 } : null);
    const started = await timed(c, 'state?fresh=1');
    assert.ok(started.ms >= 250, `waited ${started.ms} ms`);
    assert.equal(started.json.issues[3].comments, 1);
    assert.equal(Object.hasOwn(started.json, 'old'), false);
    assert.equal(started.headers['Cache-Control'], FRESH_READ);
    assert.equal(lists(t).length, cold + 1);

    t.clock.t += SNAPSHOT.maxAge + 1000;
    t.fake.addComment(3, 'owner-fake', 'Second.');
    const plain = await c.call('GET', 'state');
    assert.equal(plain.json.old, true);
    const joined = await c.call('GET', 'state?fresh=1');
    assert.equal(joined.json.issues[3].comments, 2, 'joined the refresh the plain read started');
    assert.equal(Object.hasOwn(joined.json, 'old'), false);
    await t.workshop.idle();
    assert.equal(lists(t).length, cold + 2, 'one refresh for both');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('state?fresh=1 answers the old snapshot, old and not cached, when the refresh fails or runs past freshWait', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.call('GET', 'state');
    t.clock.t += SNAPSHOT.maxAge + 1000;
    t.fake.state.hook = (r) => (LIST.test(r.path) ? { status: 500 } : null);
    const failed = await c.call('GET', 'state?fresh=1');
    assert.equal(failed.status, 200);
    assert.equal(failed.json.old, true);
    assert.equal(failed.headers['Cache-Control'], 'private, no-store');
  } finally { t.fake.state.hook = null; await t.close(); }

  await withSnapshot({ freshWait: 150 }, async () => {
    const s = await setup();
    try {
      const c = s.client();
      await c.call('GET', 'state');
      s.clock.t += SNAPSHOT.maxAge + 1000;
      s.fake.addComment(3, 'owner-fake', 'Too late for this answer.');
      s.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 600 } : null);
      const slow = await timed(c, 'state?fresh=1');
      assert.ok(slow.ms >= 140 && slow.ms < 500, `answered after ${slow.ms} ms`);
      assert.equal(slow.json.old, true);
      assert.equal(slow.json.issues[3].comments, 0);
      assert.equal(slow.headers['Cache-Control'], 'private, no-store');
      await s.workshop.idle();
    } finally { s.fake.state.hook = null; await s.close(); }
  });
});

test('only fresh=1 waits: any other value or parameter reads like a plain state', async () => {
  await withSnapshot({ freshWait: 1500 }, async () => {
    const t = await setup();
    try {
      const c = t.client();
      await c.call('GET', 'state');
      t.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 400 } : null);
      for (const path of ['state?fresh=true', 'state?fresh=0', 'state?fresh=1&fresh=1', 'state?n=1']) {
        t.clock.t += SNAPSHOT.maxAge + 1000;
        const answer = await timed(c, path);
        assert.ok(answer.ms < 250, `${path}: ${answer.ms} ms`);
        assert.equal(answer.json.old, true, path);
        assert.equal(answer.headers['Cache-Control'], STATE_READ, path);
        await t.workshop.idle();
      }
      t.clock.t += SNAPSHOT.maxAge + 1000;
      const other = await timed(c, 'state?fresh=1&n=1');
      assert.ok(other.ms >= 350, 'another parameter beside fresh=1 is ignored');
      assert.equal(Object.hasOwn(other.json, 'old'), false);
    } finally { t.fake.state.hook = null; await t.close(); }
  });
});

test('issue waits up to ageWait for a refresh that another request started', async () => {
  await withSnapshot({ ageWait: 1500 }, async () => {
    const t = await setup();
    try {
      const c = t.client();
      await c.call('GET', 'state');
      t.clock.t += SNAPSHOT.maxAge + 1000;
      t.fake.addComment(3, 'owner-fake', 'Seen by the issue read.');
      t.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 300 } : null);
      assert.equal((await c.call('GET', 'state')).json.old, true);
      const issue = await c.call('GET', 'issue?n=3');
      assert.equal(issue.json.comments.at(-1)?.text, 'Seen by the issue read.');
      assert.equal(issue.headers['Cache-Control'], 'public, s-maxage=10, stale-while-revalidate=60');
      assert.equal(Object.hasOwn(issue.json, 'old'), false, 'issue carries no old');
    } finally { t.fake.state.hook = null; await t.close(); }
  });
});

test('a cold refresh reads both issue lists and the first comment page together, within the same page budget and ceiling', async () => {
  const t = await setup();
  try {
    t.fake.state.hook = (r) => (r.kind === 'installation' && (LIST.test(r.path) || COMMENTS.test(r.path)) ? { delay: 150 } : null);
    assert.equal((await t.client().call('GET', 'state')).status, 200);
    const calls = t.fake.state.requests.filter((r) => r.kind === 'installation');
    assert.equal(calls.length, 3);
    const lastStart = Math.max(...calls.map((r) => r.wall));
    const firstEnd = Math.min(...calls.map((r) => r.done));
    assert.ok(lastStart < firstEnd, 'all three were in flight at once');
    assert.ok(calls.some((r) => r.query.includes('workshop%3Aentry')) && calls.some((r) => r.query.includes('workshop%3Asuggestion')) && calls.some((r) => COMMENTS.test(r.path)));
  } finally { t.fake.state.hook = null; await t.close(); }

  // With one item a page, the calls a cold refresh makes; a ceiling one short of that refuses it.
  const paged = async (budget, values = {}) => {
    let result;
    await withSnapshot(values, async () => {
      const s = await setup();
      try {
        s.fake.state.perPage = 1;
        s.fake.state.commentsPerPage = 1;
        const run = async () => (await s.client().call('GET', 'state')).status;
        let status;
        if (budget) {
          const { withBudget } = await import('./workshop-test-support.mjs');
          await withBudget({ refreshCallsPerMinute: budget }, async () => { status = await run(); });
        } else {
          status = await run();
        }
        const calls = s.fake.state.requests.filter((r) => r.kind === 'installation' && (LIST.test(r.path) || COMMENTS.test(r.path)));
        result = { status, calls: calls.length, listPages: calls.filter((r) => LIST.test(r.path)).length, logs: s.logs };
      } finally { await s.close(); }
    });
    return result;
  };
  const all = await paged(0);
  assert.equal(all.status, 200);
  assert.equal((await paged(all.calls)).status, 200, `${all.calls} calls fit a ceiling of ${all.calls}`);
  assert.equal((await paged(all.calls - 1)).status, 502, 'every call counts');
  const capped = await paged(0, { issuePages: 3 });
  assert.equal(capped.listPages, 3, 'the two lists share the page budget');
  assert.ok(capped.logs.some((line) => line.includes('issue_pages_limit')));
});

test('me fetches the installation token while it reads the person', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.signIn('visitor-fake');
    t.clock.t += 2 * 3600_000;
    t.fake.state.hook = (r) => (r.path === '/user' ? { delay: 200 } : null);
    const before = t.fake.state.requests.length;
    assert.equal((await c.call('GET', 'me')).status, 200);
    const calls = t.fake.state.requests.slice(before);
    const user = calls.find((r) => r.path === '/user');
    const token = calls.find((r) => r.path.endsWith('/access_tokens'));
    assert.ok(token && token.wall < user.done, 'the token was asked for before the person was read');
    assert.deepEqual(calls.filter((r) => r.kind !== 'app').map((r) => r.path), ['/user', '/repos/amontlabs/silo/collaborators/visitor-fake/permission']);
  } finally { t.fake.state.hook = null; await t.close(); }
});
