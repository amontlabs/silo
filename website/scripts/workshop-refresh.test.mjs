import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SNAPSHOT } from '../server/snapshot.mjs';
import { setup } from './workshop-test-support.mjs';

const LIST = /^\/repos\/amontlabs\/silo\/issues$/;
const lists = (t) => t.fake.state.requests.filter((r) => r.method === 'GET' && LIST.test(r.path) && r.kind === 'installation');
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
  assert.deepEqual([fresh.ageWait, fresh.writeWait, fresh.lostAfter], [1500, 2000, 15_000]);
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
