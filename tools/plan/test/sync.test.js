import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { basePlan, git, run } from './helpers.js';
import { world } from './github-helpers.js';
import { readChange, readEntryMarker as readEntryText, readSyncedMarker, wantedIssue } from '../model/index.js';

const readEntry = (issue) => readEntryText(issue.body);
import { LABELS } from '../model/names.js';
import { canonicalFeature, canonicalOrder, canonicalText } from '../model/canonical.js';

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

test('an entry issue whose workshop:entry label was removed is found by its marker, relabelled, and keeps its verdict', async (t) => {
  const files = basePlan();
  files['features/fork.json'] = { ...files['features/fork.json'], verdict: 'take' };
  const w = await world(t, files);
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  w.fake.setVerdictLabel(fork.number, 'take', 'owner');
  w.fake.unlabel(fork.number, 'workshop:entry', 'helper'); // triage rights are enough
  const result = await w.run(['sync', '--json']);
  assert.equal(result.code, 0, result.err);
  assert.equal(JSON.parse(result.out).created, 0);
  assert.equal([...w.fake.issues.values()].length, 4);
  assert.ok(fork.labels.includes('workshop:entry'));
  const pulled = JSON.parse((await w.run(['pull', '--write', '--json'])).out);
  assert.deepEqual([pulled.changes, pulled.reports], [[], []]);
  assert.equal(w.feature('fork').verdict, 'take');
});

test('a deleted entry issue is made again; pull reports the file\'s verdict and leaves it alone', async (t) => {
  const files = basePlan();
  files['features/fork.json'] = { ...files['features/fork.json'], verdict: 'take' };
  const w = await world(t, files);
  await w.run(['sync']);
  const old = w.fake.entry('fork');
  w.fake.setVerdictLabel(old.number, 'take', 'owner');
  w.fake.deleteIssue(old.number);
  await w.run(['sync']);
  const made = w.fake.entry('fork');
  assert.notEqual(made.number, old.number);
  assert.deepEqual(made.labels, ['workshop:entry'], 'sync never writes a verdict label');
  const pulled = JSON.parse((await w.run(['pull', '--write', '--json'])).out);
  assert.deepEqual(pulled.changes, []);
  assert.match(pulled.reports.find((r) => r.id === 'fork').why, /the file says take, GitHub has no verdict: was the issue replaced\?/);
  assert.equal(w.feature('fork').verdict, 'take');
});

test('history: a run stopped between the comment and the marker, then more commits: no change posted twice', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  const first = readSyncedMarker(fork.body);
  const second = w.change({ fork: { what: 'Changed.' } });
  w.fake.limitAfterWrites = w.fake.writes + 1; // the history comment lands, the marker does not
  assert.equal((await w.run(['sync'])).code, 3);
  w.fake.limitAfterWrites = null;
  assert.equal(readSyncedMarker(fork.body), first);

  // A commit that does not touch fork: its marker only moves.
  const third = w.change({ core: { what: 'The engine, in Zig.' } });
  assert.equal((await w.run(['sync'])).code, 0);
  assert.deepEqual(history(w, fork.number), [{ commit: second, before: first, fields: ['what'] }]);
  assert.equal(readSyncedMarker(fork.body), third);

  // Stopped again, then a commit that does touch fork: one comment, from the last one's commit.
  const fourth = w.change({ fork: { name: 'Fork it' } });
  w.fake.limitAfterWrites = w.fake.writes + 1; // fork's history comment lands, its marker does not
  assert.equal((await w.run(['sync'])).code, 3);
  w.fake.limitAfterWrites = null;
  const fifth = w.change({ fork: { what: 'Changed again.' } });
  assert.equal((await w.run(['sync'])).code, 0);
  assert.deepEqual(history(w, fork.number).slice(1), [{ commit: fourth, before: third, fields: ['name'] }, { commit: fifth, before: fourth, fields: ['what'] }]);
  assert.equal(readSyncedMarker(fork.body), fifth);
});

/**
 * A second sync, from another checkout, run to its end in the middle of this one: when this run
 * sends the request `when` matches, the other one runs first (`before` prepares it).
 */
function interleaved(w, when, before) {
  const other = join(w.base, 'other');
  git(w.base, 'clone', '-q', w.origin, other);
  let done = false;
  return async (url, init) => {
    if (!done && when(String(url), init)) {
      done = true;
      git(other, 'pull', '-q', 'origin', 'main');
      before?.(other);
      const result = await run(other, ['sync'], { deps: { ...w.deps } });
      assert.equal(result.code, 0, result.err);
    }
    return w.deps.fetch(url, init);
  };
}

test('a sync of an older commit beside a newer one never moves a marker backwards', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  const older = readSyncedMarker(fork.body);
  let newer;
  // This run has fetched origin/main; then a newer commit lands and is synced before this run reads the issues.
  const fetch = interleaved(w, (url) => url.endsWith('/labels?per_page=100'), (other) => {
    const path = join(other, 'plan', 'features', 'fork.json');
    writeFileSync(path, readFileSync(path, 'utf8').replace('"Copy a computer."', '"Copy a computer, newer."'));
    git(other, 'commit', '-q', '-am', 'Newer');
    git(other, 'push', '-q', 'origin', 'HEAD:main');
    newer = git(other, 'rev-parse', 'HEAD').trim();
  });
  const result = await w.run(['sync'], { deps: { fetch } });
  assert.equal(result.code, 0, result.err);
  assert.match(result.out, new RegExp(`#${fork.number} fork: synced at ${newer.slice(0, 7)}, newer than ${older.slice(0, 7)}; left alone`));
  assert.equal(readSyncedMarker(fork.body), newer);
  assert.match(fork.body, /^Copy a computer, newer\./);
  assert.deepEqual(history(w, fork.number), [{ commit: newer, before: older, fields: ['what'] }]);
});

/** Changes `order.json` in a checkout (and writes or removes feature files), commits and pushes; returns the commit. */
function pushPlan(root, { stages, write = {}, remove = [] }) {
  const path = join(root, 'plan', 'order.json');
  const order = JSON.parse(readFileSync(path, 'utf8'));
  order.stages = stages(order.stages);
  writeFileSync(path, canonicalText(canonicalOrder(order)));
  for (const [id, data] of Object.entries(write)) writeFileSync(join(root, 'plan', 'features', `${id}.json`), canonicalText(canonicalFeature(data)));
  for (const id of remove) rmSync(join(root, 'plan', 'features', `${id}.json`));
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'Plan change');
  git(root, 'push', '-q', 'origin', 'HEAD:main');
  return git(root, 'rev-parse', 'HEAD').trim();
}

test('an older sync never closes as removed the issue of a feature a newer run added', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const older = w.head();
  let newer;
  // This run syncs the plan without extra; a newer commit adds extra and is synced in the middle of it.
  const fetch = interleaved(w, (url) => url.endsWith('/labels?per_page=100'), (other) => {
    newer = pushPlan(other, {
      stages: (stages) => stages.map((stage) => (stage.id === 'loop' ? { ...stage, features: [...stage.features, 'extra'] } : stage)),
      write: { extra: { name: 'Extra', what: 'Added later.', area: 'forks', needs: [] } },
    });
  });
  const result = await w.run(['sync', '--json'], { deps: { fetch } });
  assert.equal(result.code, 0, result.err);
  const json = JSON.parse(result.out);
  assert.equal(json.synced, older);
  assert.equal(json.removed, 0);
  const extra = w.fake.entry('extra');
  assert.ok(json.notes.includes(`#${extra.number} extra: synced at ${newer.slice(0, 7)}, newer than ${older.slice(0, 7)}; left alone`));
  assert.equal(extra.state, 'open');
  assert.deepEqual(w.fake.commentsOn(extra.number), []);
  assert.equal(readSyncedMarker(extra.body), newer);
});

test('closing an issue as removed moves its marker, so an older sync never reopens it', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const older = w.head();
  const old = w.fake.entry('old');
  let newer;
  // This run syncs the plan with old; a newer commit removes old and is synced in the middle of it.
  const fetch = interleaved(w, (url) => url.endsWith('/labels?per_page=100'), (other) => {
    newer = pushPlan(other, { stages: (stages) => stages.map((stage) => ({ ...stage, features: stage.features.filter((id) => id !== 'old') })), remove: ['old'] });
  });
  const result = await w.run(['sync', '--json'], { deps: { fetch } });
  assert.equal(result.code, 0, result.err);
  const json = JSON.parse(result.out);
  assert.equal(json.synced, older);
  assert.equal(json.reopened, 0);
  assert.ok(json.notes.includes(`#${old.number} old: synced at ${newer.slice(0, 7)}, newer than ${older.slice(0, 7)}; left alone`));
  assert.deepEqual([old.state, old.state_reason], ['closed', 'not_planned']);
  assert.equal(readSyncedMarker(old.body), newer);
  assert.deepEqual(w.fake.commentsOn(old.number).map((c) => c.body), ['No longer in the plan.']);
});

test('closing an issue again (dropped, reopened by hand) moves its marker to the synced commit', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  w.change({ fork: { verdict: 'drop' } }, 'Owner: drop fork');
  w.fake.setVerdictLabel(w.fake.entry('fork').number, 'drop', 'owner');
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  assert.deepEqual([fork.state, fork.state_reason], ['closed', 'not_planned']);
  Object.assign(fork, { state: 'open', state_reason: 'reopened' });
  const head = w.change({ cli: { what: 'Another command.' } });
  assert.equal((await w.run(['sync'])).code, 0);
  assert.deepEqual([fork.state, fork.state_reason], ['closed', 'not_planned']);
  assert.equal(readSyncedMarker(fork.body), head);
});

test('the entry label is not put back while the issue\'s marker is newer; the next run puts it back', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const fork = w.fake.entry('fork');
  const interleave = interleaved(w, (url) => url.endsWith('/labels?per_page=100'), (other) => {
    const path = join(other, 'plan', 'features', 'fork.json');
    writeFileSync(path, readFileSync(path, 'utf8').replace('"Copy a computer."', '"Copy a computer, newer."'));
    git(other, 'commit', '-q', '-am', 'Newer');
    git(other, 'push', '-q', 'origin', 'HEAD:main');
  });
  // After the newer run, before this one reads the issues, someone removes the entry label.
  let removed = false;
  const fetch = async (url, init) => {
    if (!removed && String(url).includes('creator=')) {
      removed = true;
      w.fake.unlabel(fork.number, 'workshop:entry', 'helper');
    }
    return interleave(url, init);
  };
  assert.equal((await w.run(['sync'], { deps: { fetch } })).code, 0);
  assert.ok(!fork.labels.includes('workshop:entry'));
  assert.equal((await w.run(['sync'])).code, 0);
  assert.ok(fork.labels.includes('workshop:entry'));
});

test('two runs that both create an issue: the next sync closes the duplicate, or reports it when it carries a verdict label', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  w.fake.deleteIssue(w.fake.entry('cli').number);
  // Both runs read the issues before either creates cli's.
  const fetch = interleaved(w, (url, init) => init?.method === 'POST' && url.endsWith('/issues'));
  await w.run(['sync'], { deps: { fetch } });
  const both = [...w.fake.issues.values()].filter((i) => readEntry(i) === 'cli');
  assert.equal(both.length, 2);
  const [kept, duplicate] = both;
  const result = await w.run(['sync', '--json']);
  assert.equal(JSON.parse(result.out).duplicatesClosed, 1);
  assert.deepEqual([duplicate.state, duplicate.state_reason], ['closed', 'not_planned']);
  assert.equal(readSyncedMarker(duplicate.body), w.head(), 'closing a duplicate moves its marker');
  assert.equal(readEntry(duplicate), 'cli');
  assert.deepEqual(w.fake.commentsOn(duplicate.number).map((c) => [c.user.login, c.body]), [[w.fake.bot.login, `Duplicate of #${kept.number}.`]]);
  assert.equal(kept.state, 'open');
  const snapshot = w.fake.snapshot();
  const again = JSON.parse((await w.run(['sync', '--json'])).out);
  assert.deepEqual(again.duplicates, []);
  assert.equal(w.fake.snapshot(), snapshot);

  const fork = w.fake.entry('fork');
  const marked = w.fake.addIssue({ title: 'Fork', body: fork.body, labels: ['workshop:entry'], user: w.fake.bot });
  w.fake.label(marked.number, 'verdict:take', 'owner');
  const { out } = await w.run(['sync']);
  assert.match(out, new RegExp(`#${marked.number} duplicates #${fork.number} \\(fork\\) but carries a verdict label; left open`));
  assert.equal(marked.state, 'open');
});

test('the issue of a feature removed from the plan is closed as not planned, once', async (t) => {
  const w = await world(t, basePlan());
  await w.run(['sync']);
  const old = w.fake.entry('old');
  const order = JSON.parse(readFileSync(join(w.root, 'plan', 'order.json'), 'utf8'));
  order.stages = order.stages.map((stage) => ({ ...stage, features: stage.features.filter((id) => id !== 'old') }));
  writeFileSync(join(w.root, 'plan', 'order.json'), canonicalText(canonicalOrder(order)));
  rmSync(join(w.root, 'plan', 'features', 'old.json'));
  git(w.root, 'add', '-A');
  git(w.root, 'commit', '-q', '-m', 'Remove old');
  git(w.root, 'push', '-q', 'origin', 'HEAD:main');
  assert.equal((await w.run(['sync'])).code, 0);
  assert.deepEqual([old.state, old.state_reason], ['closed', 'not_planned']);
  assert.deepEqual(w.fake.commentsOn(old.number).map((c) => [c.user.login, c.body]), [[w.fake.bot.login, 'No longer in the plan.']]);
  const snapshot = w.fake.snapshot();
  await w.run(['sync']);
  assert.equal(w.fake.snapshot(), snapshot);
});
