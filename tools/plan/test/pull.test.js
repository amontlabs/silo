import { test } from 'node:test';
import assert from 'node:assert/strict';
import { basePlan, git } from './helpers.js';
import { world } from './github-helpers.js';
import { commitMessage } from '../github/pull.js';

async function synced(t, files = basePlan()) {
  const w = await world(t, files);
  assert.equal((await w.run(['sync'])).code, 0);
  return w;
}

const pull = async (w, ...args) => {
  const result = await w.run(['pull', '--json', ...args]);
  assert.equal(result.code, 0, result.err);
  return JSON.parse(result.out);
};

test('one counted label: the file gets that verdict; by a person who can decide, or by the bot', async (t) => {
  const w = await synced(t);
  w.fake.setVerdictLabel(w.fake.entry('fork').number, 'take', 'owner');
  w.fake.setVerdictLabel(w.fake.entry('cli').number, 'later', 'maintainer');
  w.fake.label(w.fake.entry('core').number, 'verdict:drop', w.fake.bot.login);
  const report = await pull(w);
  assert.deepEqual(report.changes.map((c) => [c.id, c.from, c.to, c.by]), [['core', null, 'drop', w.fake.bot.login], ['cli', null, 'later', 'maintainer'], ['fork', null, 'take', 'owner']]);
  assert.equal(w.feature('fork').verdict, undefined, 'a report writes nothing');
  const written = await pull(w, '--write');
  assert.deepEqual(written.written, ['core', 'cli', 'fork']);
  assert.equal(w.feature('fork').verdict, 'take');
  assert.equal(w.feature('cli').verdict, 'later');
  assert.equal((await pull(w)).changes.length, 0, 'a second pull finds nothing');
});

test('a label that does not count, two labels, a built feature: reported, the file left alone', async (t) => {
  const files = basePlan();
  files['features/core.json'] = { ...files['features/core.json'], verdict: 'take', built: '2026-11-01' };
  const w = await synced(t, files);
  w.fake.setVerdictLabel(w.fake.entry('fork').number, 'take', 'helper'); // triage only
  const cli = w.fake.entry('cli').number;
  w.fake.label(cli, 'verdict:take', 'owner');
  w.fake.label(cli, 'verdict:drop', 'owner');
  w.fake.label(w.fake.entry('core').number, 'verdict:drop', 'owner');
  w.fake.label(w.fake.entry('old').number, 'verdict:take', 'someone[bot]');
  const report = await pull(w, '--write');
  assert.deepEqual(report.changes, []);
  const why = Object.fromEntries(report.reports.map((r) => [r.id, r.why]));
  assert.match(why.fork, /set by helper, who cannot decide/);
  assert.match(why.cli, /several verdict labels \(verdict:take, verdict:drop\)/);
  assert.match(why.core, /built on 2026-11-01: its verdict never changes/);
  assert.match(why.old, /set by someone\[bot\], who cannot decide/);
  assert.equal(w.feature('fork').verdict, undefined);
  assert.equal(w.feature('core').verdict, 'take');
});

test('no verdict label clears the file only on a counted removal; an issue with no verdict event is reported', async (t) => {
  const files = basePlan();
  for (const id of ['core', 'cli', 'fork']) files[`features/${id}.json`] = { ...files[`features/${id}.json`], verdict: 'later' };
  const w = await synced(t, files);
  for (const id of ['core', 'cli']) w.fake.setVerdictLabel(w.fake.entry(id).number, 'later', 'owner');
  w.fake.unlabel(w.fake.entry('core').number, 'verdict:later', 'owner');
  w.fake.unlabel(w.fake.entry('cli').number, 'verdict:later', 'helper');
  const report = await pull(w, '--write');
  assert.deepEqual(report.changes.map((c) => [c.id, c.to, c.by]), [['core', null, 'owner']]);
  assert.match(report.reports.find((r) => r.id === 'cli').why, /removed by helper, who cannot decide/);
  assert.match(report.reports.find((r) => r.id === 'fork').why, /the file says later, GitHub has no verdict: was the issue replaced\?/);
  assert.equal(w.feature('core').verdict, undefined);
  assert.equal(w.feature('fork').verdict, 'later');
  assert.equal(w.feature('cli').verdict, 'later');
});

test('pull --commit: one commit, subject Owner: …, a line per feature with who and when', async (t) => {
  const w = await synced(t);
  const env = { GIT_AUTHOR_NAME: 'Bot', GIT_AUTHOR_EMAIL: 'bot@example.invalid', GIT_COMMITTER_NAME: 'Bot', GIT_COMMITTER_EMAIL: 'bot@example.invalid' };
  for (const [k, v] of Object.entries(env)) process.env[k] = v;
  t.after(() => { for (const k of Object.keys(env)) delete process.env[k]; });
  w.fake.setVerdictLabel(w.fake.entry('fork').number, 'take', 'owner');
  w.fake.setVerdictLabel(w.fake.entry('cli').number, 'drop', 'maintainer');
  const result = await pull(w, '--commit');
  assert.match(result.commit, /^[0-9a-f]{40}$/);
  const message = git(w.root, 'log', '-1', '--format=%B').trim().split('\n');
  assert.equal(message[0], 'Owner: drop cli, take fork');
  assert.equal(message[1], '');
  assert.match(message[2], new RegExp(`^drop cli \\(#${w.fake.entry('cli').number}\\): by maintainer at 2026-11-16T\\d\\d:\\d\\d:\\d\\dZ$`));
  assert.match(message[3], /^take fork \(#\d+\): by owner at /);
  assert.deepEqual(git(w.root, 'show', '--name-only', '--format=', 'HEAD').trim().split('\n'), ['plan/features/cli.json', 'plan/features/fork.json']);
  assert.equal(git(w.root, 'status', '--porcelain'), '');
  const nothing = await pull(w, '--commit');
  assert.equal(nothing.commit, null);
});

test('the subject is at most 72 characters, then …', () => {
  const changes = Array.from({ length: 9 }, (_, i) => ({ id: `feature-${i}`, issue: i + 1, from: null, to: 'take', by: 'owner', at: '2026-11-20T12:00:00Z' }));
  const { subject, body } = commitMessage(changes);
  assert.equal(subject.length, 72);
  assert.ok(subject.endsWith('…'));
  assert.ok(subject.startsWith('Owner: take feature-0, take feature-1'));
  assert.equal(body.split('\n').length, 9);
  assert.equal(commitMessage([{ id: 'x', issue: 2, from: 'take', to: null, by: null, at: null }]).subject, 'Owner: clear x');
});

test('pull writes only over the verdict it read: a file changed meanwhile is skipped', async (t) => {
  const w = await synced(t);
  w.fake.setVerdictLabel(w.fake.entry('fork').number, 'take', 'owner');
  w.fake.afterWrite = null;
  // Changing the file between the read and the write: the events request is the last read before writing.
  const original = w.deps.fetch;
  const fetch = async (url, init) => {
    const response = await original(url, init);
    if (String(url).includes(`/issues/${w.fake.entry('fork').number}/events`)) w.change({ fork: { verdict: 'later' } }, 'Meanwhile');
    return response;
  };
  const result = await w.run(['pull', '--write', '--json'], { deps: { fetch } });
  const json = JSON.parse(result.out);
  assert.deepEqual(json.written, []);
  assert.match(json.skipped[0], /changed during the run/);
  assert.equal(w.feature('fork').verdict, 'later');
});

test('only the three verdict labels are verdict labels: a stray verdict:* label is no verdict and no verdict event', async (t) => {
  const files = basePlan();
  files['features/core.json'] = { ...files['features/core.json'], verdict: 'later' };
  const w = await synced(t, files);
  const core = w.fake.entry('core');
  const fork = w.fake.entry('fork');
  w.fake.label(core.number, 'verdict:later', 'owner');
  w.fake.unlabel(core.number, 'verdict:later', 'owner');
  w.fake.label(core.number, 'verdict:refine', 'owner'); // newer, but not a verdict
  w.fake.label(fork.number, 'verdict:tkae', 'owner');
  const report = await pull(w);
  assert.deepEqual(report.changes.map((c) => [c.id, c.to, c.by]), [['core', null, 'owner']]);
  assert.deepEqual(report.reports, []);
});

test('verdict labels in another capitalisation are read the same', async (t) => {
  const w = await synced(t);
  w.fake.labels.push({ name: 'Verdict:Take', color: 'ededed', description: '' });
  w.fake.labels = w.fake.labels.filter((l) => l.name !== 'verdict:take');
  w.fake.label(w.fake.entry('fork').number, 'verdict:take', 'owner');
  w.fake.label(w.fake.entry('cli').number, 'VERDICT:DROP', 'owner');
  assert.ok(w.fake.entry('fork').labels.includes('Verdict:Take'));
  const report = await pull(w);
  assert.deepEqual(report.changes.map((c) => [c.id, c.to]), [['cli', 'drop'], ['fork', 'take']]);
  assert.deepEqual(report.reports, []);
});
