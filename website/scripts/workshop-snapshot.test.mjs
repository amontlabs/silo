import { test } from 'node:test';
import assert from 'node:assert/strict';
import { CUTS } from '../server/issues.mjs';
import { SNAPSHOT } from '../server/snapshot.mjs';
import { BUDGET } from '../server/workshop.mjs';
import { count, setup, withBudget } from './workshop-test-support.mjs';

const LIST = /^\/repos\/amontlabs\/silo\/issues$/;
const COMMENTS = /^\/repos\/amontlabs\/silo\/issues\/comments$/;
const lists = (t) => t.fake.state.requests.filter((r) => r.method === 'GET' && LIST.test(r.path));
const github = (t) => t.fake.state.requests.length;

async function withSnapshot(values, run) {
  const saved = { ...SNAPSHOT };
  Object.assign(SNAPSHOT, values);
  try {
    await run();
  } finally {
    Object.assign(SNAPSHOT, saved);
  }
}

// Reads once, lets the background refresh it may start finish, and reads again.
async function settle(t, path = 'state') {
  await t.client().call('GET', path);
  await t.workshop.idle();
  return t.client().call('GET', path);
}

test('1,000 anonymous reads of state and issues cause no GitHub call beyond the refreshes the clock allows', async () => {
  const t = await setup();
  try {
    const c = t.client();
    assert.equal((await c.call('GET', 'state')).status, 200);
    const afterCold = github(t);
    const started = t.clock.t;
    for (let i = 0; i < 1000; i += 1) {
      t.clock.t += 150;
      const path = i % 2 ? 'state' : `issue?n=${(i % 6) + 1}`;
      assert.equal((await c.call('GET', path)).status, 200);
      if (i % 50 === 0) await t.workshop.idle();
    }
    await t.workshop.idle();
    const elapsed = t.clock.t - started;
    const refreshes = lists(t).length - 1;
    assert.ok(refreshes <= Math.floor(elapsed / SNAPSHOT.maxAge), `${refreshes} refreshes in ${elapsed} ms`);
    const calls = t.fake.state.requests.slice(afterCold).filter((r) => r.kind !== 'app');
    assert.ok(calls.every((r) => LIST.test(r.path) || COMMENTS.test(r.path)), 'only refresh calls');
    assert.equal(calls.length, refreshes * 2, 'one issue page and one comment call per refresh');
  } finally { await t.close(); }
});

test('a refresh with nothing changed sends conditional requests and gets 304s', async () => {
  const t = await setup();
  try {
    t.fake.state.perPage = 3;
    await t.client().call('GET', 'state');
    const cold = lists(t);
    assert.ok(cold.length >= 3 && cold.every((r) => r.status === 200));
    t.clock.t += 61_000;
    await settle(t);
    const again = lists(t).slice(cold.length);
    assert.equal(again.length, cold.length);
    assert.ok(again.every((r) => r.status === 304), 'every unchanged page is a 304');
    t.clock.t += SNAPSHOT.fullReloadEvery;
    await settle(t);
    const full = t.fake.state.requests.filter((r) => COMMENTS.test(r.path) && r.query.includes('sort=created'));
    assert.equal(full.length, 2);
    assert.equal(full[1].status, 304, 'the full comment reload is conditional too');
  } finally { await t.close(); }
});

test('changes on GitHub show up after the next refresh; a deleted comment within the 10-minute reload', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.call('GET', 'state');
    const added = t.fake.addComment(1, 'visitor-fake', 'A new comment.');
    t.fake.editComment(1, t.fake.state.issues.get(1).comments[0].id, 'Edited.');
    t.fake.react(3, 'visitor-fake');
    t.fake.state.issues.get(2).labels.push('verdict:take');
    const fresh = t.fake.addIssue({ title: 'New decision', labels: ['workshop:decision'] });
    Object.assign(t.fake.state.issues.get(6), { state: 'closed', state_reason: 'not_planned', closed_at: new Date(t.clock.t).toISOString() });
    t.clock.t += 61_000;
    const state = (await settle(t)).json.issues;
    assert.equal(state[1].comments, 3);
    assert.equal(state[3].up, 1);
    assert.equal(state[2].verdict, 'take');
    assert.equal(state[fresh.number].kind, 'decision');
    assert.deepEqual([state[6].state, state[6].reason], ['closed', 'not_planned']);
    const issue = (await c.call('GET', 'issue?n=1')).json;
    assert.deepEqual(issue.comments.map((x) => x.text), ['Edited.', 'Posted by an agent.', 'A new comment.']);

    t.fake.deleteComment(1, added.id);
    t.clock.t += 61_000;
    await settle(t);
    assert.ok((await c.call('GET', 'issue?n=1')).json.comments.some((x) => x.id === added.id), 'an incremental refresh cannot see a deletion');
    t.clock.t += SNAPSHOT.fullReloadEvery;
    await settle(t);
    assert.equal((await c.call('GET', 'issue?n=1')).json.comments.some((x) => x.id === added.id), false, 'gone after the full reload');
  } finally { await t.close(); }
});

test('each kind of write patches the snapshot at once, even when the refresh it starts fails', async () => {
  const t = await setup();
  try {
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const owner = t.client();
    await owner.signIn('owner-fake');
    const reader = t.client();
    await reader.call('GET', 'state');
    t.fake.state.hook = (r) => (r.kind === 'installation' && r.method === 'GET' && (LIST.test(r.path) || COMMENTS.test(r.path)) ? { status: 500 } : null);
    await visitor.call('POST', 'comment', { body: { n: 2, text: 'Patched in.' } });
    await visitor.call('POST', 'vote', { body: { n: 2, on: true } });
    await owner.call('POST', 'verdict', { body: { n: 2, verdict: 'drop', note: 'Not now.' } });
    await owner.call('POST', 'decide', { body: { n: 4, text: 'Debian.' } });
    const suggestion = await visitor.call('POST', 'suggest', { body: { title: 'Patched suggestion', text: 'x' } });
    await visitor.call('POST', 'vote', { body: { n: 1, on: false } });
    const state = (await reader.call('GET', 'state')).json.issues;
    assert.deepEqual([state[2].comments, state[2].up, state[2].verdict, state[2].state, state[2].reason], [2, 2, 'drop', 'closed', 'not_planned']);
    assert.deepEqual([state[4].state, state[4].reason, state[4].comments], ['closed', 'completed', 2]);
    assert.equal(state[1].up, 0);
    assert.deepEqual([state[suggestion.json.number].kind, state[suggestion.json.number].author], ['suggestion', 'visitor-fake']);
    const issue = (await reader.call('GET', 'issue?n=2')).json;
    assert.deepEqual(issue.comments.slice(-2).map((x) => [x.author, x.text]), [['visitor-fake', 'Patched in.'], ['owner-fake', 'Not now.']]);
    assert.equal((await reader.call('GET', 'issue?n=4')).json.comments.at(-1).text, 'Debian.');
    assert.equal((await reader.call('GET', `issue?n=${suggestion.json.number}`)).json.body, 'x');
    await t.workshop.idle();
    assert.equal((await reader.call('GET', 'state')).json.issues[2].verdict, 'drop', 'the patch holds through the failed refresh');
    t.fake.state.hook = null;
    t.clock.t += 61_000;
    const truth = (await settle(t)).json.issues;
    assert.deepEqual([truth[2].comments, truth[2].up, truth[2].verdict], [2, 2, 'drop'], 'GitHub agrees after the refresh');
  } finally { await t.close(); }
});

test('cold start: GitHub down answers 502 and waits before trying again; GitHub slow is waited for once', async () => {
  const t = await setup();
  try {
    t.fake.state.hook = (r) => (LIST.test(r.path) ? { status: 500 } : null);
    const down = await t.client().call('GET', 'state');
    assert.equal(down.status, 502);
    assert.equal(down.json.error, 'github_failed');
    assert.equal((await t.client().call('GET', 'issue?n=1')).status, 502);
    assert.equal(lists(t).length, 1, 'no second try within 60 seconds');
    t.fake.state.hook = null;
    t.clock.t += 61_000;
    assert.equal((await t.client().call('GET', 'state')).status, 200);
  } finally { t.fake.state.hook = null; await t.close(); }

  const s = await setup();
  try {
    s.fake.state.hook = (r) => (LIST.test(r.path) ? { delay: 300 } : null);
    const started = Date.now();
    const [first, second] = await Promise.all([s.client().call('GET', 'state'), s.client().call('GET', 'issue?n=1')]);
    assert.ok(Date.now() - started >= 250);
    assert.deepEqual([first.status, second.status], [200, 200]);
    assert.equal(lists(s).length, 1, 'two requests during a refresh share it');
    const quick = Date.now();
    assert.equal((await s.client().call('GET', 'state')).status, 200);
    assert.ok(Date.now() - quick < 200, 'served from the snapshot');
  } finally { s.fake.state.hook = null; await s.close(); }
});

test('a refresh that fails halfway leaves the previous snapshot intact', async () => {
  const t = await setup();
  try {
    t.fake.state.perPage = 3;
    const before = (await t.client().call('GET', 'state')).json;
    t.fake.state.issues.get(1).title = 'Renamed';
    t.fake.addIssue({ title: 'Late entry', labels: ['workshop:entry'] });
    t.fake.state.hook = (r) => (LIST.test(r.path) && r.query.includes('page=2') ? { status: 502 } : null);
    t.clock.t += 61_000;
    const during = await settle(t);
    assert.deepEqual(during.json.issues, before.issues, 'nothing from the failed refresh');
    t.fake.state.hook = null;
    t.clock.t += 61_000;
    const after = (await settle(t)).json.issues;
    assert.equal(after[1].title, 'Renamed');
    assert.ok(Object.values(after).some((i) => i.title === 'Late entry'));
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('when GitHub fails, the snapshot keeps answering, stale after 10 minutes, with no retry before the time GitHub gave', async () => {
  const t = await setup();
  try {
    const c = t.client();
    await c.call('GET', 'state');
    t.fake.state.rateLimited = true;
    t.clock.t += 61_000;
    const limited = await settle(t);
    assert.equal(limited.status, 200);
    assert.equal(limited.json.stale, undefined);
    const tried = lists(t).length;
    t.clock.t += 61_000;
    await settle(t);
    assert.equal(lists(t).length, tried, 'GitHub gave 120 seconds');
    t.clock.t += 60_000;
    await settle(t);
    assert.equal(lists(t).length, tried + 1);
    t.clock.t += 9 * 60_000;
    const stale = await c.call('GET', 'state');
    assert.equal(stale.json.stale, true);
    assert.equal((await c.call('GET', 'issue?n=1')).json.stale, true);
    t.fake.state.rateLimited = false;
    await t.workshop.idle();
    t.clock.t += 3 * 60_000;
    const back = await settle(t);
    assert.equal(back.json.stale, undefined);
  } finally { await t.close(); }
});

test('anonymous reads are public for everyone, read no cookie and set none', async () => {
  const t = await setup();
  try {
    const signed = t.client();
    await signed.signIn('visitor-fake');
    for (const path of ['state', 'issue?n=1']) {
      for (const who of [t.client(), signed]) {
        const response = await who.call('GET', path);
        assert.equal(response.headers['Cache-Control'], 'public, s-maxage=10, stale-while-revalidate=60');
        assert.equal(response.headers.Vary, undefined);
        assert.deepEqual(response.cookies, []);
      }
    }
    const garbage = await t.workshop.handle({ method: 'GET', url: '/api/workshop/state', headers: { cookie: '__Host-workshop-session=garbage' }, body: '' });
    assert.deepEqual(garbage.cookies, [], 'a bad session cookie is not even read');
    assert.equal((await t.client().call('GET', 'issue?n=7')).status, 404);
    assert.equal((await t.client().call('GET', 'issue?n=99')).status, 404);
  } finally { await t.close(); }
});

test('bounds: issues, comments per issue, the full reload, and the cuts', async () => {
  const t = await setup();
  try {
    for (let i = 0; i < 150; i += 1) t.fake.addComment(2, 'visitor-fake', `c${i}`);
    t.fake.addComment(1, 'visitor-fake', 'z'.repeat(CUTS.comment + 10));
    t.fake.state.issues.get(3).body = 'b'.repeat(CUTS.body + 10);
    const c = t.client();
    await c.call('GET', 'state');
    const two = (await c.call('GET', 'issue?n=2')).json;
    assert.equal(two.comments.length, SNAPSHOT.commentsPerIssue);
    assert.equal(two.more, true);
    assert.equal(two.comments.at(-1).text, 'c149');
    const one = (await c.call('GET', 'issue?n=1')).json;
    assert.deepEqual([one.comments.at(-1).text.length, one.comments.at(-1).cut], [CUTS.comment, true]);
    assert.equal((await c.call('GET', 'issue?n=3')).json.body.length, CUTS.body);
  } finally { await t.close(); }

  await withSnapshot({ issues: 3, fullCommentPages: 1, comments: 2 }, async () => {
    const s = await setup();
    try {
      s.fake.state.commentsPerPage = 3;
      const state = (await s.client().call('GET', 'state')).json;
      assert.deepEqual(Object.keys(state.issues), ['1', '2', '3'], 'at most SNAPSHOT.issues, oldest first');
      const held = (await s.client().call('GET', 'issue?n=1')).json;
      assert.equal(held.more, true, 'comments beyond the full reload are not held');
      const all = [1, 2, 3].map(async (n) => (await s.client().call('GET', `issue?n=${n}`)).json.comments.length);
      assert.ok((await Promise.all(all)).reduce((a, b) => a + b, 0) <= 2, 'the total comment bound');
    } finally { await s.close(); }
  });
});

test('the refresh counts against the installation backstop', async () => {
  assert.equal(BUDGET.refreshCallsPerMinute, 60);
  const t = await setup();
  try {
    await withBudget({ refreshCallsPerMinute: 1 }, async () => {
      assert.equal((await t.client().call('GET', 'state')).status, 502);
    });
  } finally { await t.close(); }
});
