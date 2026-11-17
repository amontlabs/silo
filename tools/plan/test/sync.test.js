import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { basePlan, git } from './helpers.js';
import { world } from './github-helpers.js';
import { readChange, readSyncedMarker, wantedIssue } from '../model/index.js';
import { LABELS } from '../model/names.js';

const SITE = 'http://127.0.0.1:9';
const history = (w, number) => w.fake.commentsOn(number).map((c) => readChange(c.body)).filter(Boolean);

test('sync creates the six labels and one issue per feature; a second run changes nothing', async (t) => {
  const w = await world(t, basePlan());
  const first = await w.run(['sync', '--json']);
  assert.equal(first.code, 0, first.err);
  const result = JSON.parse(first.out);
  assert.equal(result.created, 4);
  assert.equal(result.labels, 6);
  assert.deepEqual(w.fake.labels.map((l) => l.name), LABELS.map((l) => l.name));
  const head = w.head();
  for (const id of ['core', 'cli', 'fork', 'old']) {
    const issue = w.fake.entry(id);
    const wanted = wantedIssue({ id, ...w.feature(id) }, { synced: head, site: SITE });
    assert.equal(issue.title, wanted.title);
    assert.equal(issue.body, wanted.body);
    assert.deepEqual(issue.labels, ['workshop:entry']);
    assert.equal(issue.state, 'open');
  }
  assert.equal(w.fake.comments.length, 0, 'no history comment for a new issue');
  const before = w.fake.snapshot();
  const writes = w.fake.writes;
  const second = await w.run(['sync', '--json']);
  assert.equal(second.code, 0);
  assert.equal(JSON.parse(second.out).unchanged, 4);
  assert.equal(w.fake.writes, writes);
  assert.equal(w.fake.snapshot(), before);
});

test('sync reads origin/main, never the working tree; a plan that fails the check is not synced', async (t) => {
  const w = await world(t, basePlan());
  writeFileSync(join(w.root, 'plan', 'features', 'core.json'), '{"name":"Changed in the tree"}\n');
  assert.equal((await w.run(['sync'])).code, 0);
  assert.equal(w.fake.entry('core').title, 'Core');
  git(w.root, 'commit', '-q', '-am', 'Broken');
  git(w.root, 'push', '-q', 'origin', 'HEAD:main');
  const writes = w.fake.writes;
  const broken = await w.run(['sync']);
  assert.equal(broken.code, 1);
  assert.match(broken.err, /fails plan check/);
  assert.equal(w.fake.writes, writes);
});

test('dry run says what it would do and writes nothing', async (t) => {
  const w = await world(t, basePlan());
  const dry = await w.run(['sync', '--dry-run']);
  assert.equal(dry.code, 0, dry.err);
  assert.match(dry.out, /would create the label verdict:take/);
  assert.match(dry.out, /would create the issue of fork/);
  assert.equal(w.fake.writes, 0);
});

test('an issue is updated, closed as not planned, reopened, and closed as completed with its building label removed', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  const core = w.fake.entry('core');
  w.change({ fork: { name: 'Fork a computer' } });
  await w.run(['sync']);
  assert.equal(fork.title, 'Fork a computer');

  // The owner's drop: the label first, then pull brings it to the file, then sync closes the issue.
  w.fake.setVerdictLabel(fork.number, 'drop');
  w.change({ fork: { verdict: 'drop' } }, 'Owner: drop fork');
  await w.run(['sync']);
  assert.deepEqual([fork.state, fork.state_reason], ['closed', 'not_planned']);
  w.fake.setVerdictLabel(fork.number, 'take');
  w.change({ fork: { verdict: 'take' } }, 'Owner: take fork');
  await w.run(['sync']);
  assert.deepEqual([fork.state, fork.state_reason], ['open', 'reopened']);

  w.fake.setVerdictLabel(core.number, 'take');
  w.fake.label(core.number, 'workshop:building', 'owner');
  w.change({ core: { verdict: 'take', built: '2026-11-19' } }, 'Built core');
  await w.run(['sync']);
  assert.deepEqual([core.state, core.state_reason], ['closed', 'completed']);
  assert.ok(!core.labels.includes('workshop:building'));
  assert.ok(core.labels.includes('verdict:take'), 'sync never touches a verdict label');
  const snapshot = w.fake.snapshot();
  await w.run(['sync']);
  assert.equal(w.fake.snapshot(), snapshot);
});

test('a verdict on the issue that pull has not brought yet holds the state; sync writes no verdict label', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  w.fake.setVerdictLabel(fork.number, 'take');
  w.change({ fork: { verdict: 'drop' } }, 'Hand-written drop');
  const { out } = await w.run(['sync']);
  assert.match(out, /its state waits for plan pull/);
  assert.equal(fork.state, 'open');
  assert.deepEqual(fork.labels.filter((l) => l.startsWith('verdict:')), ['verdict:take']);
  const labelEvents = w.fake.events.filter((e) => e.label?.name?.startsWith('verdict:') && e.actor.login === w.fake.bot.login);
  assert.deepEqual(labelEvents, []);
});

test('history: one comment when the feature changed since its marker, the marker moved each time', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  const first = readSyncedMarker(fork.body);
  assert.equal(first, w.head());

  const second = w.change({ fork: { what: 'Copy a computer, fast.' } });
  await w.run(['sync']);
  assert.deepEqual(history(w, fork.number), [{ commit: second, before: first, fields: ['what'] }]);
  assert.equal(readSyncedMarker(fork.body), second);
  const text = w.fake.commentsOn(fork.number)[0].body;
  assert.equal(text.split('\n')[0], `Updated in ${second.slice(0, 7)}: what`);
  assert.equal(text.split('\n')[1], `https://github.com/amontlabs/silo/compare/${first}...${second}`);
  assert.ok(!text.includes('Copy a computer'), 'no text from the plan in a history comment');

  // A commit that does not touch the feature moves no marker and posts nothing.
  const core = w.fake.entry('core');
  assert.equal(readSyncedMarker(core.body), first);

  w.fake.setVerdictLabel(fork.number, 'take');
  const third = w.change({ fork: { verdict: 'take', notes: 'Agreed.\n' } }, 'Owner: take fork');
  const fourth = w.change({ core: { what: 'The engine, in Zig.' } });
  await w.run(['sync']);
  assert.deepEqual(history(w, fork.number).at(-1), { commit: fourth, before: second, fields: ['verdict', 'notes'], verdict: 'take' });
  assert.equal(readSyncedMarker(fork.body), fourth);
  assert.deepEqual(history(w, core.number), [{ commit: fourth, before: first, fields: ['what'] }]);
  assert.ok(third);

  w.fake.setVerdictLabel(core.number, 'take');
  w.change({ core: { verdict: 'take' } }, 'Owner: take core');
  const built = w.change({ core: { built: '2026-11-19' } }, 'Built core');
  await w.run(['sync']);
  assert.deepEqual(history(w, core.number).at(-1), { commit: built, before: fourth, fields: ['verdict', 'built'], verdict: 'take', built: '2026-11-19' });

  w.fake.setVerdictLabel(fork.number, null);
  const cleared = w.change({ fork: { verdict: undefined } }, 'Owner: clear fork');
  await w.run(['sync']);
  assert.deepEqual(history(w, fork.number).at(-1), { commit: cleared, before: fourth, fields: ['verdict'], verdict: null });

  const count = w.fake.comments.length;
  await w.run(['sync']);
  assert.equal(w.fake.comments.length, count, 'a second run posts nothing');
});

test('history: a marker whose commit is not reachable any more only moves the marker', async (t) => {
  const w = await world(t, basePlan());
  w.change({ fork: { what: 'Before the rewrite.' } });
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  git(w.root, 'reset', '-q', '--hard', 'HEAD~1');
  const rewritten = w.change({ fork: { what: 'After the rewrite.' } }, 'Rewritten');
  await w.run(['sync']);
  assert.deepEqual(history(w, fork.number), []);
  assert.equal(readSyncedMarker(fork.body), rewritten);
  assert.match(fork.body, /^After the rewrite\./);
});

test('a run stopped half way resumes without duplicates: between issues, and between a history comment and its marker', async (t) => {
  const w = await world(t, basePlan());
  w.fake.limitAfterWrites = 8; // the six labels and two issues
  const stopped = await w.run(['sync']);
  assert.equal(stopped.code, 3);
  assert.match(stopped.err, /rate limit.*the next run resumes/);
  w.fake.limitAfterWrites = null;
  assert.equal((await w.run(['sync'])).code, 0);
  assert.equal([...w.fake.issues.values()].length, 4);

  const fork = w.fake.entry('fork');
  const before = readSyncedMarker(fork.body);
  const next = w.change({ fork: { what: 'Changed.' } });
  w.fake.limitAfterWrites = w.fake.writes + 1; // the history comment lands, the marker does not
  assert.equal((await w.run(['sync'])).code, 3);
  assert.equal(readSyncedMarker(fork.body), before);
  assert.equal(history(w, fork.number).length, 1);
  w.fake.limitAfterWrites = null;
  assert.equal((await w.run(['sync'])).code, 0);
  assert.deepEqual(history(w, fork.number), [{ commit: next, before, fields: ['what'] }]);
  assert.equal(readSyncedMarker(fork.body), next);
});

test('a suggestion named by suggested_in is closed once, with a link to the entry', async (t) => {
  const w = await world(t, basePlan());
  const suggestion = w.fake.addIssue({ title: 'Fork please', labels: ['workshop:suggestion'], user: { login: 'visitor', type: 'User' } });
  const other = w.fake.addIssue({ title: 'Not a suggestion', user: { login: 'visitor', type: 'User' } });
  w.change({ fork: { suggested_in: suggestion.number }, cli: { suggested_in: other.number } });
  const { out } = await w.run(['sync']);
  assert.match(out, new RegExp(`suggested_in #${other.number} is not a suggestion`));
  const entry = w.fake.entry('fork');
  assert.deepEqual([suggestion.state, suggestion.state_reason], ['closed', 'completed']);
  assert.deepEqual(w.fake.commentsOn(suggestion.number).map((c) => c.body), [`Placed in the plan as #${entry.number}: ${SITE}/workshop#forks/fork`]);
  assert.match(entry.body, new RegExp(`Suggested in #${suggestion.number}`));
  assert.equal(other.state, 'open');
  w.fake.reopen(suggestion.number);
  await w.run(['sync']);
  assert.equal(suggestion.state, 'open', 'reopened by a person: left as they want it');
  assert.equal(w.fake.commentsOn(suggestion.number).length, 1);
});

test('an entry issue edited on GitHub after its marker is found again and put back, never duplicated', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  const body = fork.body;
  fork.body = `${body}\n\nAdded on GitHub.`;
  fork.title = 'Renamed on GitHub';
  await w.run(['sync']);
  assert.equal([...w.fake.issues.values()].length, 4);
  assert.equal(fork.body, body);
  assert.equal(fork.title, 'Fork');
  // An issue nobody but the bot could make: a person's issue with the marker is never an entry's.
  w.fake.addIssue({ title: 'Planted', body: '<!-- silo-entry: core -->', labels: ['workshop:entry'], user: { login: 'visitor', type: 'User' } });
  const core = w.fake.entry('core');
  w.change({ core: { name: 'Core engine' } });
  await w.run(['sync']);
  assert.equal(core.title, 'Core engine');
  assert.equal([...w.fake.issues.values()].find((i) => i.title === 'Planted').body, '<!-- silo-entry: core -->');
});
