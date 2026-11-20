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
// The App's calls to the entry issues that the `open` ceiling counts: not the snapshot's refreshes, which carry `page=`.
const openCalls = (t, from) => t.fake.state.requests.slice(from).filter((r) => r.kind === 'installation' && ISSUES.test(r.path) && !/[?&]page=/.test(r.query));
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

const MANY = Array.from({ length: 40 }, (_, i) => ({ id: `m${i}`, kind: 'feature', title: `Entry m${i}`, part: 'computers', suggestedIn: null }));

test('one person opening more than thirty threads in an hour, several in the same minute, is refused none', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MANY] });
  try {
    // The plan's entries and the `open` ceiling bound openings; no budget counts them.
    assert.deepEqual(Object.keys(BUDGET).filter((key) => /opening/i.test(key)), []);
    assert.equal(BUDGET.openCallsPerMinute, 27);
    const c = await signedIn(t);
    const statuses = [];
    // Eight openings a minute, three calls each, for five minutes.
    for (let minute = 0; minute < 5; minute += 1) {
      for (const { id } of MANY.slice(minute * 8, minute * 8 + 8)) {
        statuses.push((await c.call('POST', minute % 2 ? 'vote' : 'comment', { body: minute % 2 ? { entry: id, on: true } : { entry: id, text: 'x' } })).status);
      }
      t.clock.t += 61_000;
    }
    assert.deepEqual(statuses, Array(40).fill(200));
    assert.equal(created(t).length, 40);
    for (const { id } of MANY) assert.equal(entryIssues(t, id).length, 1, id);
  } finally { await t.close(); }
});

test('more openings at once than the `open` ceiling affords: every issue created has its write made, the refused ones made no opening', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MANY] });
  try {
    const c = await signedIn(t);
    t.fake.state.hook = (r) => (r.kind === 'installation' && ISSUES.test(r.path) ? { delay: 20 } : null);
    const ids = MANY.slice(0, 13).map((entry) => entry.id);
    const first = t.fake.state.requests.length;
    const answers = await Promise.all(ids.map((id) => c.call('POST', 'comment', { body: { entry: id, text: 'x' } })));
    t.fake.state.hook = null;
    const made = answers.filter((answer) => answer.status === 200);
    const refused = answers.filter((answer) => answer.status === 429);
    assert.equal(made.length + refused.length, 13, answers.map((answer) => answer.status).join());
    assert.ok(made.length > 0 && refused.length > 0);
    assert.equal(created(t).length, made.length);
    for (const answer of refused) assert.deepEqual([answer.json.message, Object.hasOwn(answer.json, 'n')], ['The Workshop is busy. Try again in a minute.', false]);
    for (const answer of made) assert.deepEqual(issueOf(t.fake, answer.json.n).comments.map((x) => x.body), ['x']);
    // The calls the `open` ceiling counts, as the fake recorded them: 3 for each opening, 1 for each
    // refusal, never above the ceiling. The snapshot's refreshes read the same address, but with `page=`.
    const calls = openCalls(t, first);
    assert.equal(calls.length, made.length * 3 + refused.length, calls.map((r) => `${r.method} ${r.query}`).join('\n'));
    assert.ok(calls.length <= BUDGET.openCallsPerMinute, `${calls.length} calls`);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('the `open` ceiling refuses an opening it has no room for, 429 with the wait, opens nothing, and lets it through in the next minute', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MANY] });
  try {
    const c = await signedIn(t);
    // Four calls a minute: one opening is three, so the second has no room for itself and the read after it.
    await withBudget({ openCallsPerMinute: 4 }, async () => {
      assert.equal((await c.call('POST', 'comment', { body: { entry: 'm0', text: 'x' } })).status, 200);
      const before = t.fake.state.requests.length;
      const refused = await c.call('POST', 'comment', { body: { entry: 'm1', text: 'x' } });
      assert.deepEqual([refused.status, refused.json.error, refused.json.message], [429, 'rate_limited', 'The Workshop is busy. Try again in a minute.']);
      assert.ok(Number(refused.headers['Retry-After']) > 0 && Number(refused.headers['Retry-After']) <= 60, refused.headers['Retry-After']);
      assert.equal(created(t).length, 1);
      assert.deepEqual(entryIssues(t, 'm1'), []);
      assert.equal(since(t, before).filter((call) => call.startsWith('POST')).length, 0, 'no write of any kind');
      t.clock.t += Number(refused.headers['Retry-After']) * 1000;
      const again = await c.call('POST', 'comment', { body: { entry: 'm1', text: 'x' } });
      assert.equal(again.status, 200, again.body);
      assert.equal(created(t).length, 2);
      assert.deepEqual(issueOf(t.fake, again.json.n).comments.map((x) => x.body), ['x']);
    });
  } finally { await t.close(); }
});

test('a read after an opening paid for in one minute and run in the next is counted in the next, never refused', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MANY] });
  try {
    const c = await signedIn(t);
    // Three calls a minute: one opening, whose read after it runs a minute after it was charged.
    await withBudget({ openCallsPerMinute: 3 }, async () => {
      let next = null;
      t.fake.state.hook = (r) => {
        if (r.kind === 'installation' && r.method === 'POST' && ISSUES.test(r.path)) {
          t.clock.t += 61_000;
          next = t.fake.state.requests.length;
          t.fake.state.hook = null;
        }
        return null;
      };
      assert.equal((await c.call('POST', 'comment', { body: { entry: 'm0', text: 'x' } })).status, 200);
      assert.ok(next !== null);
      // In the new minute the read after the opening is already one call, so a second opening has no room.
      const refused = await c.call('POST', 'comment', { body: { entry: 'm1', text: 'x' } });
      assert.deepEqual([refused.status, refused.json.message], [429, 'The Workshop is busy. Try again in a minute.']);
      assert.equal(created(t).length, 1);
      const calls = openCalls(t, next);
      assert.deepEqual(calls.map((r) => r.method), ['GET', 'GET'], 'the read after the opening and the second opening\'s first read');
      assert.ok(calls.length <= BUDGET.openCallsPerMinute);
    });
  } finally { t.fake.state.hook = null; await t.close(); }
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

const MORE = ['e1', 'e2', 'e3'].map((id) => ({ id, kind: 'feature', title: `Entry ${id}`, part: 'computers', suggestedIn: null }));

test('a thread another instance opened, which this instance\'s snapshot does not hold, is found: nothing is opened and the comment is posted', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MORE] });
  try {
    const c = await signedIn(t);
    const elsewhere = t.fake.addIssue({ title: 'Fork a computer', body: 'Feature.\n\n<!-- silo-entry: fork-computers -->', labels: ['workshop:entry'] });
    const posted = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Found.' } });
    assert.deepEqual([posted.status, posted.json.n], [200, elsewhere.number], posted.body);
    assert.deepEqual(issueOf(t.fake, elsewhere.number).comments.map((x) => x.body), ['Found.']);
    assert.equal(created(t).length, 0);
  } finally { await t.close(); }
});

test('an instance with no snapshot reads every page of entry issues: a thread older than the newest page is found, and nothing is opened', async () => {
  const t = await setup({ entries: [...PLAN_ENTRIES, ...MORE] });
  try {
    const hoursAgo = new Date(t.clock.t - 5 * 3600_000).toISOString();
    t.fake.addIssue({ title: 'Entry e1', body: 'Feature.\n\n<!-- silo-entry: e1 -->', labels: ['workshop:entry'], createdAt: hoursAgo });
    const old = [...t.fake.state.issues.values()].find((i) => readEntryMarker(i.body) === 'e1').number;
    // More than a page of newer entry issues for other entries.
    t.clock.t += 60_000;
    for (let i = 0; i < 105; i += 1) t.fake.addIssue({ title: `Other ${i}`, body: `Feature.\n\n<!-- silo-entry: other-${i} -->`, labels: ['workshop:entry'] });
    const fresh = createWorkshop({ config: t.config, entries: readPlanEntries([...PLAN_ENTRIES, ...MORE]), now: () => t.clock.t, log: () => {} });
    const c = t.client(fresh);
    await c.signIn('visitor-fake');
    const answer = await c.call('POST', 'comment', { body: { entry: 'e1', text: 'Old thread.' } });
    assert.deepEqual([answer.status, answer.json.n], [200, old], answer.body);
    assert.equal(created(t).length, 0);
    await fresh.idle();
  } finally { await t.close(); }
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

test('the read after an opening failing: the person\'s write goes on the issue just opened, and the next write finds it', async () => {
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
    const answer = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Here.' } });
    t.fake.state.hook = null;
    assert.ok(failed);
    assert.equal(answer.status, 200, answer.body);
    const [n] = entryIssues(t, 'fork-computers');
    assert.equal(answer.json.n, n);
    assert.deepEqual(issueOf(t.fake, n).comments.map((x) => x.body), ['Here.']);
    const again = await c.call('POST', 'vote', { body: { entry: 'fork-computers', on: true } });
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

// GitHub's list by label lags by `ms` from now on. The clock first moves past it, so that the issues
// seeded at the start are listed.
function lagging(t, ms) {
  t.clock.t += ms;
  t.fake.state.listLag = ms;
}

// A refresh of the snapshot that starts now, waited for; the state it answers.
async function refreshed(t, c) {
  t.clock.t += 31_000;
  const state = await c.call('GET', 'state?fresh=1');
  assert.equal(state.status, 200);
  assert.equal(state.json.old, undefined, 'the refresh ran');
  return state.json;
}

test('GitHub\'s list by label lagging: the first 👍 and the first comment on an entry answer 200 at once, on the issue the App just opened', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    lagging(t, 60_000);
    const vote = await c.call('POST', 'vote', { body: { entry: 'which-distro', on: true } });
    assert.equal(vote.status, 200, vote.body);
    assert.deepEqual(entryIssues(t, 'which-distro'), [vote.json.n]);
    assert.deepEqual(issueOf(t.fake, vote.json.n).reactions.map((r) => r.user.login), ['visitor-fake']);
    assert.deepEqual(vote.json, { ok: true, up: 1, n: vote.json.n });

    const comment = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'First.' } });
    assert.equal(comment.status, 200, comment.body);
    assert.deepEqual(entryIssues(t, 'fork-computers'), [comment.json.n]);
    assert.deepEqual(issueOf(t.fake, comment.json.n).comments.map((x) => x.body), ['First.']);
    assert.equal(created(t).length, 2);

    // The list did not show either issue to the read after its opening.
    const after = t.fake.state.requests.filter((r) => r.kind === 'installation' && r.method === 'GET' && ISSUES.test(r.path) && !/[?&]page=/.test(r.query));
    assert.equal(after.length, 4, 'a read before and a read after each opening');
    const threads = (await c.call('GET', 'state')).json.threads;
    assert.deepEqual([threads['which-distro'], threads['fork-computers']], [vote.json.n, comment.json.n]);
  } finally { await t.close(); }
});

test('GitHub\'s list by label lagging: later writes from the same instance, before and after a refresh that read the lagging list, go on the same issue', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    const other = await signedIn(t, 'owner-fake');
    lagging(t, 5 * 60_000);
    const first = await c.call('POST', 'vote', { body: { entry: 'fork-computers', on: true } });
    assert.equal(first.status, 200, first.body);
    const n = first.json.n;

    // At once, while the list still lags.
    const second = await other.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Right after.' } });
    assert.deepEqual([second.status, second.json.n], [200, n], second.body);

    // A refresh that began after the opening, whose list does not show the issue.
    const state = await refreshed(t, c);
    assert.equal(state.threads['fork-computers'], n);
    assert.deepEqual([state.issues[n].up, state.issues[n].comments], [1, 1], 'what was written on it is kept');
    const third = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'After the refresh.' } });
    assert.deepEqual([third.status, third.json.n], [200, n], third.body);

    assert.equal(created(t).length, 1);
    assert.deepEqual(entryIssues(t, 'fork-computers'), [n]);
    assert.deepEqual(issueOf(t.fake, n).comments.map((x) => x.body), ['Right after.', 'After the refresh.']);
  } finally { await t.close(); }
});

test('GitHub\'s list by label lagging: when the read after the opening shows a lower issue for the entry, that one is the thread and the App closes its own', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    lagging(t, 60_000);
    let lower = null;
    // Another instance opened an issue for the entry just before, and the list already shows it.
    t.fake.state.hook = (r) => {
      if (lower || r.method !== 'POST' || !ISSUES.test(r.path)) return null;
      lower = t.fake.addIssue({ title: 'Fork a computer', body: 'Feature.\n\n<!-- silo-entry: fork-computers -->', labels: ['workshop:entry'] });
      lower.labelChanged.set('workshop:entry', t.clock.t - 60_000);
      return null;
    };
    const answer = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Which one?' } });
    t.fake.state.hook = null;
    assert.deepEqual([answer.status, answer.json.n], [200, lower.number], answer.body);
    assert.deepEqual(issueOf(t.fake, lower.number).comments.map((x) => x.body), ['Which one?']);
    const own = issueOf(t.fake, lower.number + 1);
    assert.deepEqual([own.state, own.state_reason, own.labels, own.comments.length], ['closed', 'not_planned', [], 0]);
    assert.deepEqual(entryIssues(t, 'fork-computers'), [lower.number]);

    // The closed issue is no thread, before and after a refresh.
    for (const state of [(await c.call('GET', 'state')).json, await refreshed(t, c)]) {
      assert.equal(state.threads['fork-computers'], lower.number);
      assert.equal(state.issues[own.number], undefined);
    }
    const again = await c.call('POST', 'vote', { body: { entry: 'fork-computers', on: true } });
    assert.deepEqual([again.status, again.json.n], [200, lower.number]);
    assert.equal(created(t).length, 1);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a thread this instance opened stays known until a refresh\'s own list shows it, and is forgotten an hour after the opening when none does', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    // Shown by a refresh: from then on it is an ordinary issue, gone from the snapshot once the list
    // no longer returns it.
    lagging(t, 30_000);
    const shown = (await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'x' } })).json.n;
    assert.equal((await refreshed(t, c)).threads['fork-computers'], shown);
    issueOf(t.fake, shown).labels = [];
    assert.equal((await refreshed(t, c)).threads['fork-computers'], undefined);

    // Never shown, as with a list that lags two hours: kept by every refresh until an hour after the
    // opening.
    t.fake.state.listLag = 2 * 3600_000;
    const opened = t.clock.t;
    const hidden = (await c.call('POST', 'comment', { body: { entry: 'which-distro', text: 'x' } })).json.n;
    // `refreshed` moves the clock 31 seconds on: these refreshes begin 59 and 60 minutes after the opening.
    t.clock.t = opened + 59 * 60_000 - 31_000;
    assert.equal((await refreshed(t, c)).threads['which-distro'], hidden);
    t.clock.t = opened + 60 * 60_000 - 31_000;
    assert.equal((await refreshed(t, c)).threads['which-distro'], undefined);

    // What happens then: the list still does not show it, so the next write opens a second issue
    // and goes on that one.
    const after = await c.call('POST', 'comment', { body: { entry: 'which-distro', text: 'An hour later.' } });
    assert.equal(after.status, 200, after.body);
    assert.deepEqual(entryIssues(t, 'which-distro'), [hidden, after.json.n]);
    assert.deepEqual(issueOf(t.fake, after.json.n).comments.map((x) => x.body), ['An hour later.']);
    assert.equal((await c.call('GET', 'state')).json.threads['which-distro'], after.json.n);
  } finally { await t.close(); }
});

test('a request that comes during the read after an opening shares the opening: it writes on the lower issue that read finds, never on the issue the App closes', async () => {
  const t = await setup();
  try {
    const a = await signedIn(t);
    const b = await signedIn(t, 'owner-fake');
    lagging(t, 60_000);
    let lower = null;
    let reading;
    const inRead = new Promise((resolve) => { reading = resolve; });
    t.fake.state.hook = (r) => {
      if (r.kind !== 'installation' || !ISSUES.test(r.path) || /[?&]page=/.test(r.query)) return null;
      if (r.method === 'POST' && !lower) {
        // Another instance opened an issue for the entry just before, and the list already shows it.
        lower = t.fake.addIssue({ title: 'Fork a computer', body: 'Feature.\n\n<!-- silo-entry: fork-computers -->', labels: ['workshop:entry'] });
        lower.labelChanged.set('workshop:entry', t.clock.t - 60_000);
      } else if (r.method === 'GET' && lower) {
        reading();
        return { delay: 300 };
      }
      return null;
    };
    const first = a.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From a.' } });
    await inRead;
    const second = await b.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From b.' } });
    const one = await first;
    t.fake.state.hook = null;
    assert.deepEqual([one.status, one.json.n, second.status, second.json.n], [200, lower.number, 200, lower.number], one.body + second.body);
    assert.deepEqual(issueOf(t.fake, lower.number).comments.map((x) => x.body).sort(), ['From a.', 'From b.']);
    const own = issueOf(t.fake, lower.number + 1);
    assert.deepEqual([own.state, own.labels, own.comments.length], ['closed', [], 0]);
    assert.equal(created(t).length, 1);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a request that comes after a refresh put the new issue in the snapshot during the read after its opening still waits for the opening, and writes on the lower issue', async () => {
  const t = await setup();
  try {
    const a = await signedIn(t);
    const b = await signedIn(t, 'owner-fake');
    let lower = null;
    let reading;
    const inRead = new Promise((resolve) => { reading = resolve; });
    t.fake.state.hook = (r) => {
      if (r.kind !== 'installation' || !ISSUES.test(r.path) || /[?&]page=/.test(r.query)) return null;
      if (r.method === 'POST' && !lower) {
        // Another instance's issue for the entry, not labelled yet: no list shows it.
        lower = t.fake.addIssue({ title: 'Fork a computer', body: 'Feature.\n\n<!-- silo-entry: fork-computers -->' });
      } else if (r.method === 'GET' && lower) {
        reading();
        return { delay: 400 };
      }
      return null;
    };
    const first = a.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From a.' } });
    await inRead;
    const n = lower.number + 1;
    // A refresh during the read after the opening: its list shows the new issue.
    t.clock.t += 31_000;
    assert.equal((await b.call('GET', 'state?fresh=1')).json.threads['fork-computers'], n);
    // The other instance's issue gets its label; the read after the opening will show it.
    t.fake.setLabels(lower, ['workshop:entry']);
    const second = await b.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From b.' } });
    const one = await first;
    t.fake.state.hook = null;
    assert.deepEqual([one.status, one.json.n, second.status, second.json.n], [200, lower.number, 200, lower.number], one.body + second.body);
    assert.deepEqual(issueOf(t.fake, lower.number).comments.map((x) => x.body).sort(), ['From a.', 'From b.']);
    const own = issueOf(t.fake, n);
    assert.deepEqual([own.state, own.labels, own.comments.length], ['closed', [], 0]);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('an issue the App closes stays out of the snapshot of a refresh that began while it was being closed', async () => {
  const t = await setup();
  try {
    const a = await signedIn(t);
    const b = await signedIn(t, 'owner-fake');
    let lower = null;
    let during = null;
    let held = false;
    t.fake.state.hook = (r) => {
      if (r.kind !== 'installation') return null;
      if (!lower && r.method === 'POST' && ISSUES.test(r.path)) {
        // Another instance opened an issue for the entry just before; the list shows it.
        lower = t.fake.addIssue({ title: 'Fork a computer', body: 'Feature.\n\n<!-- silo-entry: fork-computers -->', labels: ['workshop:entry'] });
      } else if (lower && !during && r.method === 'PATCH') {
        // While the App closes its issue, a refresh begins; it reads the issue still labelled, and
        // swaps in after the closing has answered.
        t.clock.t += 31_000;
        during = b.call('GET', 'state?fresh=1');
        return { delay: 100 };
      } else if (during && !held && r.method === 'GET' && r.path === '/repos/amontlabs/silo/issues/comments') {
        held = true;
        return { delay: 400 };
      }
      return null;
    };
    const one = await a.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'From a.' } });
    const state = (await during).json;
    t.fake.state.hook = null;
    assert.deepEqual([one.status, one.json.n], [200, lower.number], one.body);
    const n = lower.number + 1;
    assert.equal(issueOf(t.fake, n).state, 'closed');
    assert.ok(held, 'the refresh ran during the closing');
    assert.equal(state.issues[n], undefined);
    assert.equal(state.threads['fork-computers'], lower.number);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a refresh whose list shows a lower issue for the entry makes the instance forget the thread it opened, without closing it', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    lagging(t, 5 * 60_000);
    let lower = null;
    // Another instance opens an issue for the entry just before; the list does not show it yet.
    t.fake.state.hook = (r) => {
      if (!lower && r.kind === 'installation' && r.method === 'POST' && ISSUES.test(r.path)) {
        lower = t.fake.addIssue({ title: 'Fork a computer', body: 'Feature.\n\n<!-- silo-entry: fork-computers -->', labels: ['workshop:entry'] });
      }
      return null;
    };
    const mine = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Mine.' } });
    t.fake.state.hook = null;
    const n = mine.json.n;
    assert.deepEqual([mine.status, n], [200, lower.number + 1], mine.body);

    // The list shows the lower issue, and still not the one this instance opened.
    lower.labelChanged.set('workshop:entry', t.clock.t - 5 * 60_000);
    const state = await refreshed(t, c);
    assert.equal(state.threads['fork-computers'], lower.number);
    assert.equal(state.issues[n], undefined, 'no longer held');
    const later = await c.call('POST', 'vote', { body: { entry: 'fork-computers', on: true } });
    assert.deepEqual([later.status, later.json.n], [200, lower.number]);
    const own = issueOf(t.fake, n);
    assert.deepEqual([own.state, own.labels, own.comments.map((x) => x.body)], ['open', ['workshop:entry'], ['Mine.']], 'not closed');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a thread this instance opened whose label was removed by hand is forgotten at the next write, which then finds the new thread without a lookup', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    lagging(t, 5 * 60_000);
    const n = (await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'One.' } })).json.n;
    issueOf(t.fake, n).labels = [];
    const second = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Two.' } });
    assert.equal(second.status, 200, second.body);
    assert.notEqual(second.json.n, n);
    assert.deepEqual(entryIssues(t, 'fork-computers'), [second.json.n]);
    const state = (await c.call('GET', 'state')).json;
    assert.deepEqual([state.threads['fork-computers'], state.issues[n]], [second.json.n, undefined]);
    const before = t.fake.state.requests.length;
    const third = await c.call('POST', 'vote', { body: { entry: 'fork-computers', on: true } });
    assert.deepEqual([third.status, third.json.n], [200, second.json.n]);
    assert.equal(openCalls(t, before).length, 0, 'no read of entry issues');
  } finally { await t.close(); }
});

test('a person\'s read of a thread this instance opened answering 404 is an error that says to try again, with n, and opens nothing', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    lagging(t, 5 * 60_000);
    const n = (await c.call('POST', 'vote', { body: { entry: 'fork-computers', on: true } })).json.n;
    let missed = false;
    t.fake.state.hook = (r) => {
      if (missed || r.kind !== 'user' || r.method !== 'GET' || r.path !== `/repos/amontlabs/silo/issues/${n}`) return null;
      missed = true;
      return { status: 404, body: { message: 'Not Found' } };
    };
    const refused = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Soon?' } });
    t.fake.state.hook = null;
    assert.ok(missed);
    assert.deepEqual([refused.status, refused.json.n], [502, n], refused.body);
    assert.match(refused.json.message, /Try again/);
    assert.equal(created(t).length, 1);
    const again = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Soon?' } });
    assert.deepEqual([again.status, again.json.n], [200, n], again.body);
    assert.equal(created(t).length, 1);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('an issue the list returns without the label in its own labels is no thread and no Workshop issue', async () => {
  const t = await setup();
  try {
    const c = await signedIn(t);
    lagging(t, 5 * 60_000);
    // The label of engine-service's thread is removed; the list still returns the issue, without it.
    t.fake.setLabels(issueOf(t.fake, 1), []);
    const listed = await fetch(`${t.fake.url}/repos/amontlabs/silo/issues?labels=workshop:entry&state=all`, { headers: { authorization: `token ${[...t.fake.state.tokens].find(([, v]) => v.kind === 'installation')[0]}` } }).then((r) => r.json());
    assert.deepEqual(listed.find((i) => i.number === 1)?.labels, []);
    const state = await refreshed(t, c);
    assert.equal(state.issues[1], undefined);
    assert.equal(state.threads['engine-service'], undefined);
    const answer = await c.call('POST', 'comment', { body: { entry: 'engine-service', text: 'Where now?' } });
    assert.equal(answer.status, 200, answer.body);
    assert.notEqual(answer.json.n, 1);
    assert.deepEqual(entryIssues(t, 'engine-service'), [answer.json.n]);
    assert.equal(issueOf(t.fake, 1).comments.length, 2, 'nothing written on the unlabelled one');
  } finally { await t.close(); }
});

test('a request whose deadline passes after the opening carries n, and the next try goes on that issue', async () => {
  const t = await setup();
  try {
    const short = createWorkshop({ config: t.config, entries: readPlanEntries(PLAN_ENTRIES), now: () => t.clock.t, log: () => {}, deadlineMs: 400 });
    const c = t.client(short);
    await c.signIn('visitor-fake');
    await c.call('GET', 'state');
    await short.idle();
    let opened = false;
    t.fake.state.hook = (r) => {
      if (r.method === 'POST' && ISSUES.test(r.path)) opened = true;
      else if (opened && r.method === 'GET' && ISSUES.test(r.path) && r.kind === 'installation' && !/[?&]page=/.test(r.query)) return { delay: 800 };
      return null;
    };
    const late = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Late.' } });
    const [n] = entryIssues(t, 'fork-computers');
    assert.deepEqual([late.status, late.json.n], [502, n], late.body);
    await short.idle();
    t.fake.state.hook = null;
    const again = await c.call('POST', 'comment', { body: { entry: 'fork-computers', text: 'Late.' } });
    assert.deepEqual([again.status, again.json.n], [200, n], again.body);
    assert.equal(created(t).length, 1);
    await short.idle();
  } finally { t.fake.state.hook = null; await t.close(); }
});

// A small seeded generator, so that a failing sequence can be run again.
function random(seed) {
  let x = seed >>> 0;
  return (n) => {
    x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
    return x % n;
  };
}

test('model: whatever the lag of the list, the refreshes and the writes, one instance opens one issue per entry and every write goes on it', async () => {
  const ids = ['fork-computers', 'which-distro', ...MORE.map((entry) => entry.id)];
  // Each run has one lag: a list that has shown an issue does not hide it again.
  for (const [seed, lag] of [[1, 0], [7, 30_000], [42, 120_000], [5, 120_000]]) {
    const t = await setup({ entries: [...PLAN_ENTRIES, ...MORE] });
    try {
      const pick = random(seed);
      const people = [await signedIn(t), await signedIn(t, 'owner-fake')];
      lagging(t, lag);
      // The reference: the thread each entry has once a write named it.
      const threads = new Map();
      const steps = [`seed ${seed}, lag ${lag}`];
      for (let step = 0; step < 30; step += 1) {
        t.clock.t += 4_000 + pick(36_000);
        const op = pick(5);
        if (op === 0) {
          steps.push('refresh');
          const state = (await people[0].call('GET', 'state?fresh=1')).json;
          for (const [id, n] of threads) assert.equal(state.threads[id], n, `${steps.join(', ')}: ${id} in the state`);
        } else {
          const id = ids[pick(ids.length)];
          const c = people[pick(people.length)];
          const kind = pick(3);
          const body = kind === 0 ? { entry: id, text: `step ${step}` } : { entry: id, on: kind === 1 };
          steps.push(`${kind === 0 ? 'comment' : kind === 1 ? 'vote' : 'unvote'} ${id}`);
          const answer = await c.call('POST', kind === 0 ? 'comment' : 'vote', { body });
          assert.equal(answer.status, 200, `${steps.join(', ')}: ${answer.body}`);
          if (answer.json.n !== undefined) {
            if (!threads.has(id)) threads.set(id, answer.json.n);
            assert.equal(answer.json.n, threads.get(id), `${steps.join(', ')}: the write's thread`);
          } else {
            assert.ok(kind === 2 && !threads.has(id), `${steps.join(', ')}: only removing a 👍 from an entry with no thread names none`);
          }
        }
        for (const id of ids) {
          const issues = [...t.fake.state.issues.values()].filter((i) => readEntryMarker(i.body) === id);
          assert.deepEqual(issues.map((i) => i.number), threads.has(id) ? [threads.get(id)] : [], `${steps.join(', ')}: the issues of ${id}`);
        }
      }
      await t.workshop.idle();
    } finally { await t.close(); }
  }
});
