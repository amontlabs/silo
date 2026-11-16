import assert from 'node:assert/strict';
import { readFileSync, statSync } from 'node:fs';
import { test } from 'node:test';
import { lastPullPath } from '../src/pull.js';
import { startFake } from './fake-github.js';
import { run, sandbox } from './helpers.js';

const FEATURES = [
  '{"n": 1, "id": "a", "name": "A", "what": "Does a.", "verdict": null}',
  '{"n": 2, "id": "b", "name": "B", "what": "Does b.", "verdict": null}',
  '{"n": 3, "id": "c", "name": "C", "what": "Does c.", "verdict": "take"}',
  '{"n": 4, "id": "d", "name": "D", "what": "Does d.", "verdict": null}',
  '{"n": 5, "id": "e", "name": "E", "what": "Does \\"e\\" — é.", "verdict": "take"}',
  '{"n": 6, "id": "f", "name": "F", "what": "Does f."}',
  '',
].join('\n');
const DECISIONS = '# Decisions\n\n- 2026-11-16 · owner · First.\n';

async function world(t) {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t, { features: FEATURES, decisions: DECISIONS });
  assert.equal((await run(['sync'], { fake, box })).code, 0);
  const n = (id) => JSON.parse(box.features().split('\n').find((l) => l.includes(`"id": "${id}"`))).issue;
  return { fake, box, n };
}

test('pull reports verdicts, decisions, suggestions and comments; --write records only the owner’s verdicts', async (t) => {
  const { fake, box, n } = await world(t);
  fake.setVerdictLabel(n('a'), 'refine', 'owner'); // admin: recorded
  fake.setVerdictLabel(n('b'), 'later', 'helper'); // triage: not the owner's
  fake.setVerdictLabel(n('c'), 'take', 'owner'); // same as the file: nothing to say
  fake.setVerdictLabel(n('d'), 'later', 'silo-workshop[bot]'); // the bot: never recorded
  fake.close(n('e'), 'not_planned', 'maintainer'); // drop by closing, maintain role: recorded
  fake.setVerdictLabel(n('f'), 'take', 'owner');
  fake.setVerdictLabel(n('f'), 'take', 'owner');
  fake.issues.get(n('f')).labels.push('verdict:later'); // two verdict labels: a conflict, never recorded

  const decided = fake.addIssue({ title: 'Ship it?', body: 'Should we?', labels: ['workshop:decision'], user: { login: 'silo-workshop[bot]', type: 'Bot' } });
  fake.addComment(decided.number, 'silo-workshop[bot]', 'Context from an agent', { type: 'Bot' });
  fake.addComment(decided.number, 'owner', 'Yes, ship it.');
  fake.close(decided.number, 'completed');
  const referenced = fake.addIssue({ title: 'Old', labels: ['workshop:decision'] });
  fake.close(referenced.number, 'completed');
  await import('node:fs').then(({ writeFileSync }) => writeFileSync(box.decisionsPath, `${box.decisions()}- 2026-11-15 · owner · Old. (#${referenced.number})\n`));
  const open = fake.addIssue({ title: 'Still open', labels: ['workshop:decision'] });

  fake.addIssue({ title: 'Less wanted', labels: ['workshop:suggestion'], user: { login: 'visitor', type: 'User' }, up: 1 });
  fake.addIssue({ title: 'Most wanted', labels: ['workshop:suggestion'], user: { login: 'visitor2', type: 'User' }, up: 5 });
  const unrelated = fake.addIssue({ title: 'A bug report' });
  fake.addComment(n('a'), 'visitor', 'Please keep this.');
  fake.addComment(n('a'), 'silo-workshop[bot]', 'Agent note', { type: 'Bot' });
  fake.addComment(n('a'), 'dependabot[bot]', 'beep', { type: 'Bot' });
  fake.addComment(unrelated.number, 'visitor', 'Not a Workshop issue');
  fake.addComment(open.number, 'owner', 'Thinking.');

  const decisionsBefore = box.decisions();
  const featuresBefore = box.features();
  const read = await run(['pull', '--json'], { fake, box });
  assert.equal(read.code, 0, read.stderr);
  const report = JSON.parse(read.stdout);
  assert.equal(box.features(), featuresBefore, 'pull without --write writes nothing');

  const byId = Object.fromEntries(report.verdicts.map((v) => [v.id, v]));
  assert.deepEqual(Object.keys(byId).sort(), ['a', 'b', 'd', 'e', 'f']);
  assert.deepEqual([byId.a.github, byId.a.confirmed, byId.a.by], ['refine', true, 'owner']);
  assert.deepEqual([byId.b.github, byId.b.confirmed], ['later', false]);
  assert.deepEqual([byId.d.github, byId.d.confirmed], ['later', false]);
  assert.deepEqual([byId.e.github, byId.e.file, byId.e.confirmed, byId.e.by], ['drop', 'take', true, 'maintainer']);
  assert.deepEqual(byId.f.conflict, ['take', 'later']);

  assert.deepEqual(report.decisions.map((d) => d.issue), [decided.number]);
  assert.deepEqual(report.decisions[0].outcome, { author: 'owner', text: 'Yes, ship it.' });
  assert.deepEqual(report.suggestions.map((s) => [s.title, s.up]), [['Most wanted', 5], ['Less wanted', 1]]);
  assert.deepEqual(report.comments.map((c) => [c.issue, c.author, c.text]), [
    [decided.number, 'owner', 'Yes, ship it.'],
    [n('a'), 'visitor', 'Please keep this.'],
    [open.number, 'owner', 'Thinking.'],
  ]);
  assert.equal(report.written, null);

  const human = await run(['pull'], { fake, box });
  assert.match(human.stdout, /#\d+ a: none -> refine by owner/);
  assert.match(human.stdout, /not confirmed as the owner’s/);

  const written = await run(['pull', '--write', '--json'], { fake, box });
  assert.equal(written.code, 0, written.stderr);
  assert.equal(JSON.parse(written.stdout).written, 2);
  const lines = box.features().split('\n');
  const old = featuresBefore.split('\n');
  for (let i = 0; i < lines.length; i += 1) {
    const id = lines[i] && JSON.parse(lines[i]).id;
    if (id === 'a') assert.equal(lines[i], old[i].replace('"verdict": null', '"verdict": "refine"'));
    else if (id === 'e') assert.equal(lines[i], old[i].replace('"verdict": "take"', '"verdict": "drop"'));
    else assert.equal(lines[i], old[i]);
  }
  assert.equal(box.decisions(), decisionsBefore, 'DECISIONS.md is never written by pull');
  assert.equal(statSync(lastPullPath(box.home)).mode & 0o777, 0o600);
  assert.ok(!Number.isNaN(Date.parse(readFileSync(lastPullPath(box.home), 'utf8').trim())));

  // After --write only new comments show, and the recorded verdicts are no longer differences.
  fake.addComment(n('b'), 'visitor', 'New since then', { at: new Date(Date.now() + 60_000).toISOString() });
  const next = JSON.parse((await run(['pull', '--json'], { fake, box })).stdout);
  assert.deepEqual(next.comments.map((c) => c.text), ['New since then']);
  assert.deepEqual(next.verdicts.map((v) => v.id).sort(), ['b', 'd', 'f']);
});

test('a verdict field is added at the end of a line that has none', async (t) => {
  const { fake, box, n } = await world(t);
  fake.issues.get(n('f')).labels.push('verdict:later');
  fake.events.push({ issue: n('f'), event: 'labeled', label: { name: 'verdict:later' }, actor: { login: 'owner', type: 'User' } });
  assert.equal((await run(['pull', '--write'], { fake, box })).code, 0);
  const line = box.features().split('\n').find((l) => l.includes('"id": "f"'));
  assert.equal(line, `{"n": 6, "id": "f", "name": "F", "what": "Does f.", "issue": ${n('f')}, "verdict": "later"}`);
});
