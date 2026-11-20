// Entries' threads: opened by the App at the first comment or 👍 on an entry, found again by every later
// write, shared by two writes at once, bounded by our limits; and votes on comments.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readEntryMarker } from '../../tools/workshop/model/index.js';
import { BUDGET, createWorkshop } from '../server/workshop.mjs';
import { readPlanEntries } from '../server/entries.mjs';
import { ORIGIN, PLAN_ENTRIES, issueOf, setup, withBudget } from './workshop-test-support.mjs';

const ISSUES = /^\/repos\/amontlabs\/silo\/issues$/;
const created = (t) => t.fake.state.requests.filter((r) => r.method === 'POST' && ISSUES.test(r.path));
const entryIssues = (t, id) => [...t.fake.state.issues.values()].filter((i) => i.labels.includes('workshop:entry') && readEntryMarker(i.body) === id).map((i) => i.number);
const since = (t, before) => t.fake.state.requests.slice(before).map((r) => `${r.method} ${r.path.replace(/\d+/g, 'N')} ${r.kind}${r.login && r.kind === 'user' ? ` ${r.login}` : ''}`);

async function signedIn(t, login = 'visitor-fake') {
  const c = t.client();
  await c.signIn(login);
  // A warm instance: the snapshot is there, as on any instance that has answered once.
  await c.call('GET', 'state');
  await t.workshop.idle();
  return c;
}

test('the first comment on an entry opens one issue as the App and posts the comment as the person', async () => {
  const long = { id: 'long-title', kind: 'decision', title: `Which ${'very '.repeat(30)}long question is this?`, part: null, suggestedIn: null };
  const t = await setup({ entries: [...PLAN_ENTRIES, long] });
  try {
    const c = await signedIn(t);
    const before = t.fake.state.requests.length;
    const answer = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Can a fork outlive its parent?' } });
    assert.equal(answer.status, 200, answer.body);
    const n = answer.json.n;
    assert.deepEqual(answer.json, { ok: true, id: answer.json.id, comments: 1, n });
    const issue = issueOf(t.fake, n);
    assert.equal(issue.title, 'Fork a computer');
    assert.equal(issue.body, `Feature of the Silo plan: ${ORIGIN}/workshop#computers/fork-computers\n\n<!-- silo-entry: fork-computers -->`);
    assert.deepEqual(issue.labels, ['workshop:entry']);
    assert.equal(issue.user.login, 'silo-workshop[bot]');
    assert.deepEqual(issue.comments.map((x) => [x.user.login, x.body]), [['visitor-fake', 'Can a fork outlive its parent?']]);
    // The calls, in order, and whose token each used: the App reads the entry issues, opens the issue
    // and reads them again; the person posts their own words.
    await t.workshop.idle();
    const calls = since(t, before).filter((call) => !/^GET \/repos\/amontlabs\/silo\/issues(\/comments)? installation$/.test(call) || call === 'GET /repos/amontlabs/silo/issues installation');
    assert.deepEqual(calls.slice(0, 4), [
      'GET /repos/amontlabs/silo/issues installation',
      'POST /repos/amontlabs/silo/issues installation',
      'GET /repos/amontlabs/silo/issues installation',
      'POST /repos/amontlabs/silo/issues/N/comments user visitor-fake',
    ]);

    const state = (await c.call('GET', 'state')).json;
    assert.equal(state.threads['fork-computers'], n);
    assert.equal(state.issues[n].entry, 'fork-computers');

    // A decision's title is cut at a word, with an ellipsis, to the title limit; its row is under #product.
    const decision = await c.call('POST', 'comment', { body: { entry: 'long-title', text: 'Hm.' } });
    const opened = issueOf(t.fake, decision.json.n);
    assert.ok([...opened.title].length <= 120 && opened.title.endsWith('very…'), opened.title);
    assert.match(opened.body, /^Decision of the Silo plan: https:\/\/silo\.test\/workshop#product\/long-title\n/);
  } finally { await t.close(); }
});

test('the first 👍 on an entry opens one issue and gives the 👍 as the person; removing one where there is no thread opens nothing', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    const off = await c.call('POST', 'vote', { body: { entry: 'which-distro', on: false } });
    assert.deepEqual([off.status, off.json], [200, { ok: true, up: 0 }]);
    assert.equal(created(t).length, 0);
    assert.deepEqual(entryIssues(t, 'which-distro'), []);

    const on = await c.call('POST', 'vote', { body: { entry: 'which-distro', on: true } });
    assert.equal(on.status, 200, on.body);
    assert.deepEqual(on.json, { ok: true, up: 1, n: on.json.n });
    const issue = issueOf(t.fake, on.json.n);
    assert.deepEqual([issue.title, issue.labels, issue.user.login], ['Which Linux distribution do computers start with?', ['workshop:entry'], 'silo-workshop[bot]']);
    assert.deepEqual(issue.reactions.map((r) => r.user.login), ['visitor-fake']);
    const reaction = t.fake.state.requests.findLast((r) => r.method === 'POST' && r.path.endsWith('/reactions'));
    assert.deepEqual([reaction.kind, reaction.login], ['user', 'visitor-fake']);

    // Removing it now finds the thread.
    const removed = await c.call('POST', 'vote', { body: { entry: 'which-distro', on: false } });
    assert.deepEqual(removed.json, { ok: true, up: 0, n: on.json.n });
    assert.equal(created(t).length, 1);
  } finally { await t.close(); }
});

test('a second comment, and an issue another instance opened since the snapshot, open nothing', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    const first = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'One.' } });
    const other = await signedIn(t, 'owner-fake');
    const second = await other.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Two.' } });
    assert.equal(second.json.n, first.json.n);
    assert.equal(created(t).length, 1);
    assert.deepEqual(issueOf(t.fake, first.json.n).comments.map((x) => x.body), ['One.', 'Two.']);

    // Another instance opened which-distro's thread after this instance's snapshot was read.
    const elsewhere = t.fake.addIssue({ title: 'Which Linux distribution do computers start with?', body: 'Decision of the Silo plan.\n\n<!-- silo-entry: which-distro -->', labels: ['workshop:entry'] });
    const found = await c.call('POST', 'comment', { body: { entry: 'which-distro', text: 'Debian.' } });
    assert.equal(found.json.n, elsewhere.number);
    assert.equal(created(t).length, 1, 'nothing opened');
    assert.deepEqual(issueOf(t.fake, elsewhere.number).comments.map((x) => x.body), ['Debian.']);
  } finally { await t.close(); }
});

test('two first comments at once on one instance share one opening', async () => {
  const t = await setup();
  try {
    const a = await signedIn(t);
    const b = await signedIn(t, 'owner-fake');
    // Every App call waits a little, so that both requests are in flight together.
    t.fake.state.hook = (r) => (r.kind === 'installation' && ISSUES.test(r.path) ? { delay: 30 } : null);
    const [one, two] = await Promise.all([
      a.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From a.' } }),
      b.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From b.' } }),
    ]);
    t.fake.state.hook = null;
    assert.deepEqual([one.status, two.status], [200, 200], one.body + two.body);
    assert.equal(one.json.n, two.json.n);
    assert.equal(created(t).length, 1);
    assert.deepEqual(entryIssues(t, 'fork-computers'), [one.json.n]);
    assert.deepEqual(issueOf(t.fake, one.json.n).comments.map((x) => x.body).sort(), ['From a.', 'From b.']);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('an id that is no entry of the plan, and a request naming both an issue and an entry, are refused before any GitHub call', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    const before = t.fake.state.requests.length;
    for (const body of [{ entry: 'nope', text: 'x' }, { entry: 'constructor', text: 'x' }, { entry: '__proto__', on: true }]) {
      const answer = await c.call('POST', body.text ? 'comment' : 'vote', { body });
      assert.deepEqual([answer.status, answer.json.error], [404, 'not_found'], JSON.stringify(body));
    }
    for (const body of [{ n: 1, entry: 'fork-computers', text: 'x' }, { entry: 3, text: 'x' }, { entry: 'fork-computers', text: '' }]) {
      assert.equal((await c.call('POST', 'comment', { body })).status, 400, JSON.stringify(body));
    }
    assert.equal((await c.call('POST', 'vote', { body: { entry: 'fork-computers' } })).status, 400);
    assert.equal(t.fake.state.requests.length, before);
  } finally { await t.close(); }
});

test('openings are limited per person, for everyone, and by the App\'s ceiling; each answered 429 in plain words', async () => {
  const entries = [...PLAN_ENTRIES, ...['e1', 'e2', 'e3', 'e4'].map((id) => ({ id, kind: 'feature', title: `Entry ${id}`, part: 'computers', suggestedIn: null }))];
  const t = await setup({ entries });
  try {
    assert.deepEqual([BUDGET.openingsPerPersonPerHour, BUDGET.openingsPerHour, BUDGET.openCallsPerMinute], [5, 30, 10]);
    const visitor = await signedIn(t);
    const owner = await signedIn(t, 'owner-fake');
    t.fake.addUser('third-fake');
    const third = await signedIn(t, 'third-fake');
    // Everyone's limit counts the entry issues created on GitHub in the last hour: the seeded ones are older.
    t.clock.t += 3601_000;
    await withBudget({ openingsPerPersonPerHour: 1, openingsPerHour: 2 }, async () => {
      assert.equal((await visitor.call('POST', 'comment', { body: { entry: 'e1', text: 'x' } })).status, 200);
      const person = await visitor.call('POST', 'comment', { body: { entry: 'e2', text: 'x' } });
      assert.deepEqual([person.status, person.json.message], [429, 'You have started too many new threads this hour. Try again later.']);
      assert.ok(Number(person.headers['Retry-After']) > 0);
      // A thread that exists is no opening.
      assert.equal((await visitor.call('POST', 'comment', { body: { entry: 'e1', text: 'again' } })).status, 200);
      assert.equal((await owner.call('POST', 'vote', { body: { entry: 'e2', on: true } })).status, 200);
      const everyone = await third.call('POST', 'vote', { body: { entry: 'e3', on: true } });
      assert.deepEqual([everyone.status, everyone.json.message], [429, 'Too many new threads were started on the Workshop this hour. Try again later.']);
      assert.equal(created(t).length, 2);
      t.clock.t += 3601_000;
      assert.equal((await visitor.call('POST', 'comment', { body: { entry: 'e3', text: 'x' } })).status, 200, 'an hour later');
    });
    t.clock.t += 60_000;
    await withBudget({ openCallsPerMinute: 1 }, async () => {
      // The read before the opening takes the minute's one call: the opening is refused, nothing is created.
      const ceiling = await visitor.call('POST', 'comment', { body: { entry: 'e4', text: 'x' } });
      assert.equal(ceiling.status, 429);
      assert.equal(created(t).length, 3);
      assert.deepEqual(entryIssues(t, 'e4'), []);
    });
  } finally { await t.close(); }
});

test('the person\'s write failing after the opening: the issue stays, the error carries n, and the next try finds it', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    t.fake.state.hook = (r) => (r.method === 'POST' && /\/issues\/\d+\/comments$/.test(r.path) ? { status: 500 } : null);
    const failed = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Lost?' } });
    t.fake.state.hook = null;
    assert.equal(failed.status, 502);
    assert.equal(failed.json.error, 'github_failed');
    const n = failed.json.n;
    assert.deepEqual(entryIssues(t, 'fork-computers'), [n]);
    const again = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Lost?' } });
    assert.deepEqual([again.status, again.json.n], [200, n]);
    assert.equal(created(t).length, 1);
    assert.deepEqual(issueOf(t.fake, n).comments.map((x) => x.body), ['Lost?']);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a closed thread stays the thread: a write GitHub refuses is "closed", and no second issue is opened', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    const issue = issueOf(t.fake, 1);
    issue.state = 'closed';
    issue.state_reason = 'completed';
    // GitHub takes comments on a closed issue that is not locked.
    assert.deepEqual((await c.call('POST', 'comment', { body: { entry: 'engine-service', text: 'Still here.' } })).json.n, 1);
    issue.locked = true;
    for (const [endpoint, body] of [['comment', { entry: 'engine-service', text: 'Locked?' }], ['vote', { entry: 'engine-service', on: true }]]) {
      const refused = await c.call('POST', endpoint, { body });
      assert.deepEqual([refused.status, refused.json.error, refused.json.message, refused.json.n], [403, 'closed', 'This thread is closed on GitHub.', 1]);
    }
    assert.equal(created(t).length, 0);
    // The form with n answers as before.
    const plain = await c.call('POST', 'comment', { body: { n: 1, text: 'Locked?' } });
    assert.deepEqual([plain.status, plain.json.error, Object.hasOwn(plain.json, 'n')], [403, 'forbidden', false]);
  } finally { await t.close(); }
});

test('suggested_in: an entry written from a suggestion keeps the suggestion\'s issue as its thread', async () => {
  const entries = [...PLAN_ENTRIES, { id: 'not-from-one', kind: 'feature', title: 'Named after an ordinary issue', part: 'computers', suggestedIn: 7 }];
  const t = await setup({ entries });
  try {
    const c = await signedIn(t);
    const comment = await c.call('POST', 'comment', { body: { entry: 'share-link', text: 'Glad it made it.' } });
    assert.deepEqual([comment.status, comment.json.n], [200, 6]);
    assert.equal(issueOf(t.fake, 6).comments.at(-1).body, 'Glad it made it.');
    assert.equal(created(t).length, 0);
    const state = (await c.call('GET', 'state')).json;
    assert.equal(state.threads['share-link'], 6);
    assert.deepEqual([state.latest[0].n, state.latest[0].entry], [6, 'share-link']);
    // An issue that is not a Workshop suggestion is no thread: the entry gets its own.
    const own = await c.call('POST', 'comment', { body: { entry: 'not-from-one', text: 'x' } });
    assert.notEqual(own.json.n, 7);
    assert.deepEqual(entryIssues(t, 'not-from-one'), [own.json.n]);
  } finally { await t.close(); }
});

test('several issues for one id: the lowest number is the thread, and the others are ignored', async () => {
  const t = await setup();
  try {
    const marker = 'Feature.\n\n<!-- silo-entry: fork-computers -->';
    const low = t.fake.addIssue({ title: 'Fork a computer', body: marker, labels: ['workshop:entry'] }).number;
    const high = t.fake.addIssue({ title: 'Fork a computer', body: marker, labels: ['workshop:entry'] }).number;
    t.fake.addComment(high, 'visitor-fake', 'On the ignored one.');
    const c = await signedIn(t);
    const state = (await c.call('GET', 'state')).json;
    assert.equal(state.threads['fork-computers'], low);
    assert.equal(state.latest.some((x) => x.n === high), false);
    const answer = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Here.' } });
    assert.equal(answer.json.n, low);
    assert.equal(created(t).length, 0);
  } finally { await t.close(); }
});

test('votes on comments: each comment\'s count, the person\'s own, and giving or taking one back', async () => {
  const t = await setup();
  try {
    const mine = t.fake.addComment(2, 'owner-fake', 'Worth a vote.');
    const other = t.fake.addComment(3, 'owner-fake', 'On another issue.');
    t.fake.reactToComment(mine.id, 'owner-fake');
    const c = await signedIn(t);

    // GET issue: each comment's 👍 count, from what the listing already returned.
    const before = t.fake.state.requests.length;
    const issue = (await c.call('GET', 'issue?n=2')).json;
    assert.deepEqual(issue.comments.map((x) => [x.id, x.up]), [[mine.id, 1]]);
    assert.equal(t.fake.state.requests.length, before, 'no call');

    // GET vote: the comments the person gave a 👍, in one call more, with their token.
    assert.deepEqual((await c.call('GET', 'vote?n=2')).json, { on: false, comments: [] });
    const reading = t.fake.state.requests.length;
    assert.equal((await c.call('POST', 'comment-vote', { body: { n: 2, comment: mine.id, on: true } })).json.up, 2);
    const vote = await c.call('GET', 'vote?n=2');
    assert.deepEqual(vote.json, { on: false, comments: [mine.id] });
    const graphql = t.fake.state.requests.slice(reading).filter((r) => r.path === '/graphql');
    assert.deepEqual(graphql.map((r) => [r.kind, r.login]), [['user', 'visitor-fake']]);

    // POST comment-vote: as the person, counted as POST vote counts.
    const reaction = t.fake.state.requests.findLast((r) => r.method === 'POST' && r.path.endsWith(`/comments/${mine.id}/reactions`));
    assert.deepEqual([reaction.kind, reaction.login], ['user', 'visitor-fake']);
    assert.deepEqual((await c.call('POST', 'comment-vote', { body: { n: 2, comment: mine.id, on: true } })).json, { ok: true, up: 2 }, 'a 👍 already there adds nothing');
    assert.equal((await c.call('GET', 'issue?n=2')).json.comments[0].up, 2, 'the snapshot is patched');
    assert.deepEqual((await c.call('POST', 'comment-vote', { body: { n: 2, comment: mine.id, on: false } })).json, { ok: true, up: 1 });
    assert.deepEqual(issueOf(t.fake, 2).comments[0].reactions.map((r) => r.user.login), ['owner-fake'], 'only their own removed');

    // A comment of another issue, a comment that does not exist, and an issue that is not the Workshop's.
    for (const body of [{ n: 2, comment: other.id, on: true }, { n: 2, comment: 999_999, on: true }, { n: 7, comment: mine.id, on: true }]) {
      const refused = await c.call('POST', 'comment-vote', { body });
      assert.deepEqual([refused.status, refused.json.error], [404, 'not_found'], JSON.stringify(body));
    }
    assert.deepEqual(issueOf(t.fake, 3).comments.at(-1).reactions, []);
    for (const body of [{ n: 2, comment: '1', on: true }, { n: 2, comment: mine.id }, { comment: mine.id, on: true }]) {
      assert.equal((await c.call('POST', 'comment-vote', { body })).status, 400, JSON.stringify(body));
    }
    assert.equal((await t.client().call('POST', 'comment-vote', { body: { n: 2, comment: mine.id, on: true } })).status, 401);
  } finally { await t.close(); }
});

const MORE = ['e1', 'e2', 'e3', 'e4', 'e5', 'e6'].map((id) => ({ id, kind: 'feature', title: `Entry ${id}`, part: 'computers', suggestedIn: null }));

test('at 4 of 5 openings for the person, two openings at once create one issue', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MORE] });
  try {
    const c = await signedIn(t);
    // A minute apart: the App's ceiling of ten calls a minute holds three openings.
    for (const id of ['e1', 'e2', 'e3', 'e4']) {
      t.clock.t += 61_000;
      assert.equal((await c.call('POST', 'comment', { body: { entry: id, text: 'x' } })).status, 200, id);
    }
    t.clock.t += 61_000;
    // A thread found on GitHub gives the charge back: it was no opening.
    t.fake.addIssue({ title: 'Entry e6', body: 'Feature.\n\n<!-- silo-entry: e6 -->', labels: ['workshop:entry'] });
    assert.equal((await c.call('POST', 'comment', { body: { entry: 'e6', text: 'found' } })).status, 200);
    // Every App call waits a little, so that both requests are in flight together.
    t.fake.state.hook = (r) => (r.kind === 'installation' && ISSUES.test(r.path) ? { delay: 20 } : null);
    const both = await Promise.all(['e5', 'which-distro'].map((id) => c.call('POST', 'comment', { body: { entry: id, text: 'x' } })));
    t.fake.state.hook = null;
    assert.deepEqual(both.map((answer) => answer.status).sort(), [200, 429]);
    assert.equal(both.find((answer) => answer.status === 429).json.message, 'You have started too many new threads this hour. Try again later.');
    assert.equal(created(t).length, 5);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('at 29 of 30 openings for everyone, counted on GitHub, two openings at once create one issue, on any instance', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MORE] });
  try {
    t.clock.t += 3601_000;
    const a = await signedIn(t);
    const b = await signedIn(t, 'owner-fake');
    // Opened in the last hour by other instances, or by this one before a restart.
    for (let i = 0; i < 29; i += 1) t.fake.addIssue({ title: `Other ${i}`, body: `Feature.\n\n<!-- silo-entry: other-${i} -->`, labels: ['workshop:entry'] });
    t.fake.state.hook = (r) => (r.kind === 'installation' && ISSUES.test(r.path) ? { delay: 20 } : null);
    const both = await Promise.all([a.call('POST', 'comment', { body: { entry: 'e1', text: 'x' } }), b.call('POST', 'comment', { body: { entry: 'e2', text: 'x' } })]);
    t.fake.state.hook = null;
    assert.deepEqual(both.map((answer) => answer.status).sort(), [200, 429]);
    assert.equal(both.find((answer) => answer.status === 429).json.message, 'Too many new threads were started on the Workshop this hour. Try again later.');
    assert.equal(created(t).length, 1);
    // A new instance knows nothing in memory, and GitHub still says 30.
    const fresh = createWorkshop({ config: t.config, entries: readPlanEntries([...PLAN_ENTRIES, ...MORE]), now: () => t.clock.t, log: () => {} });
    const c = t.client(fresh);
    await c.signIn('visitor-fake');
    assert.equal((await c.call('POST', 'comment', { body: { entry: 'e3', text: 'x' } })).status, 429);
    assert.equal(created(t).length, 1);
    await fresh.idle();
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('two instances opening one entry at the same moment: both writes land on the lower issue, and the higher one is closed without its label', async () => {
  const t = await setup();
  try {
    const other = createWorkshop({ config: t.config, entries: readPlanEntries(PLAN_ENTRIES), now: () => t.clock.t, log: () => {} });
    const a = await signedIn(t);
    const b = t.client(other);
    await b.signIn('owner-fake');
    await b.call('GET', 'state');
    await other.idle();
    // Both openings are sent after both reads found nothing.
    t.fake.state.hook = (r) => (r.method === 'POST' && ISSUES.test(r.path) ? { delay: 40 } : null);
    const [one, two] = await Promise.all([
      a.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From a.' } }),
      b.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From b.' } }),
    ]);
    t.fake.state.hook = null;
    assert.deepEqual([one.status, two.status], [200, 200], one.body + two.body);
    assert.equal(created(t).length, 2, 'each instance opened one');
    const opened = [...t.fake.state.issues.values()].filter((i) => readEntryMarker(i.body) === 'fork-computers').map((i) => i.number).sort((x, y) => x - y);
    const [lower, higher] = opened;
    assert.deepEqual([one.json.n, two.json.n], [lower, lower]);
    assert.deepEqual(issueOf(t.fake, lower).comments.map((x) => x.body).sort(), ['From a.', 'From b.']);
    const closed = issueOf(t.fake, higher);
    assert.deepEqual([closed.state, closed.state_reason, closed.labels, closed.comments.length], ['closed', 'not_planned', [], 0]);
    const retire = t.fake.state.requests.find((r) => r.method === 'PATCH' && r.path.endsWith(`/issues/${higher}`));
    assert.equal(retire.kind, 'installation');
    assert.deepEqual(entryIssues(t, 'fork-computers'), [lower]);
    assert.equal((await a.call('GET', 'state')).json.threads['fork-computers'], lower);
    await other.idle();
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('the read after an opening failing: the person\'s write is not made, and the next try finds the issue', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    let opened = false;
    let failed = false;
    t.fake.state.hook = (r) => {
      if (r.method === 'POST' && ISSUES.test(r.path)) opened = true;
      else if (opened && !failed && r.method === 'GET' && ISSUES.test(r.path) && r.kind === 'installation') {
        failed = true;
        return { status: 500 };
      }
      return null;
    };
    const answer = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Wait.' } });
    t.fake.state.hook = null;
    assert.equal(answer.status, 502);
    assert.match(answer.json.message, /Try again/);
    const [n] = entryIssues(t, 'fork-computers');
    assert.deepEqual(issueOf(t.fake, n).comments, [], 'no write on an issue the second read did not confirm');
    const again = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Wait.' } });
    assert.deepEqual([again.status, again.json.n], [200, n]);
    assert.equal(created(t).length, 1);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a thread from the snapshot whose marker or label was changed on GitHub is no thread: the entry gets its own', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    issueOf(t.fake, 1).body = 'Moved.\n\n<!-- silo-entry: something-else -->';
    issueOf(t.fake, 2).labels = [];
    for (const id of ['engine-service', 'calls-and-events']) {
      const answer = await c.call('POST', 'comment', { body: { entry: id, text: 'Where?' } });
      assert.equal(answer.status, 200, answer.body);
      assert.ok(![1, 2].includes(answer.json.n), `${id}: ${answer.json.n}`);
      assert.deepEqual(entryIssues(t, id), [answer.json.n]);
    }
    assert.deepEqual([issueOf(t.fake, 1).comments.length, issueOf(t.fake, 2).comments.length], [2, 0], 'nothing written on the old ones');
  } finally { await t.close(); }
});

test('an issue GitHub created without its label: the App creates the label and adds it; when that fails, the issue is closed', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    t.fake.state.missingLabels.add('workshop:entry');
    const answer = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Labelled?' } });
    assert.equal(answer.status, 200, answer.body);
    assert.deepEqual(issueOf(t.fake, answer.json.n).labels, ['workshop:entry']);
    assert.equal(t.fake.state.missingLabels.size, 0, 'the label was created');
    const label = t.fake.state.requests.find((r) => r.method === 'POST' && r.path === '/repos/amontlabs/silo/labels');
    assert.equal(label.kind, 'installation');

    t.fake.state.missingLabels.add('workshop:entry');
    t.fake.state.hook = (r) => (r.method === 'POST' && r.path === '/repos/amontlabs/silo/labels' ? { status: 500 } : null);
    const before = created(t).length;
    const refused = await c.call('POST', 'comment', { body: { entry: 'which-distro', text: 'Labelled?' } });
    t.fake.state.hook = null;
    assert.deepEqual([refused.status, refused.json.error], [502, 'github']);
    assert.equal(created(t).length, before + 1);
    const left = [...t.fake.state.issues.values()].filter((i) => readEntryMarker(i.body) === 'which-distro');
    assert.deepEqual(left.map((i) => [i.state, i.state_reason, i.labels, i.comments.length]), [['closed', 'not_planned', [], 0]]);
    assert.equal([...t.fake.state.issues.values()].some((i) => i.state === 'open' && i.user.login === 'silo-workshop[bot]' && i.labels.length === 0 && readEntryMarker(i.body)), false);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a thread locked between the read and the write answers "closed"', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    t.fake.state.hook = (r) => {
      if (r.method === 'POST' && /\/issues\/1\/(comments|reactions)$/.test(r.path)) issueOf(t.fake, 1).locked = true;
      return null;
    };
    for (const [endpoint, body] of [['comment', { entry: 'engine-service', text: 'Just in time?' }], ['vote', { entry: 'engine-service', on: true }]]) {
      issueOf(t.fake, 1).locked = false;
      const refused = await c.call('POST', endpoint, { body });
      assert.deepEqual([refused.status, refused.json.error, refused.json.message, refused.json.n], [403, 'closed', 'This thread is closed on GitHub.', 1]);
    }
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('every answer to a write naming an entry whose thread the server knows carries n, errors included', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    t.fake.state.hook = (r) => (r.method === 'GET' && r.path === '/repos/amontlabs/silo/issues/1' ? { status: 500 } : null);
    for (const [endpoint, body] of [['comment', { entry: 'engine-service', text: 'x' }], ['vote', { entry: 'engine-service', on: true }]]) {
      const failed = await c.call('POST', endpoint, { body });
      assert.deepEqual([failed.status, failed.json.n], [502, 1], failed.body);
    }
  } finally { t.fake.state.hook = null; await t.close(); }
});
