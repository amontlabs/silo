import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { listItems } from '../src/decisions.js';
import { startFake } from './fake-github.js';
import { run, sandbox } from './helpers.js';

const REAL = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'docs', 'product');
const entryIssues = (fake) => [...fake.issues.values()].filter((i) => i.labels.includes('workshop:entry'));
const decisionIssues = (fake) => [...fake.issues.values()].filter((i) => i.labels.includes('workshop:decision'));

test('sync on the real files creates every issue, writes numbers back and keeps every other byte', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  fake.pageSize = 25;
  const box = sandbox(t);
  const featuresBefore = box.features();
  const decisionsBefore = box.decisions();
  const lineCount = featuresBefore.split('\n').filter((l) => l.trim()).length;
  const itemCount = listItems(decisionsBefore.split('\n')).filter((i) => i.issue == null).length;

  const first = await run(['sync'], { fake, box });
  assert.equal(first.code, 0, first.stderr);
  assert.equal(first.stdout.trim(), `created ${lineCount + itemCount}, updated 0, unchanged 0, labels 7`);
  assert.equal(entryIssues(fake).length, lineCount);
  assert.equal(decisionIssues(fake).length, itemCount);
  assert.equal(fake.labels.length, 7);

  // features.jsonl: each line gains only `, "issue": N` before its closing brace.
  const before = featuresBefore.split('\n');
  const after = box.features().split('\n');
  assert.equal(after.length, before.length);
  for (let i = 0; i < before.length; i += 1) {
    if (!before[i].trim()) {
      assert.equal(after[i], before[i]);
      continue;
    }
    const n = JSON.parse(after[i]).issue;
    assert.equal(after[i], `${before[i].slice(0, -1)}, "issue": ${n}}`);
    const issue = fake.issues.get(n);
    const entry = JSON.parse(before[i]);
    assert.equal(issue.title, entry.name);
    assert.equal(issue.body, `${entry.what}\n\n<!-- silo-entry: ${entry.id} -->`);
    assert.deepEqual(issue.labels, ['workshop:entry']);
    assert.equal(issue.user.login, 'silo-workshop[bot]');
  }

  // DECISIONS.md: removing the appended references gives back the original bytes.
  const decisions = box.decisions();
  assert.equal(decisions.replace(/ \(#\d+\)(?=\n)/g, ''), decisionsBefore);
  for (const item of listItems(decisions.split('\n'))) {
    const issue = fake.issues.get(item.issue);
    assert.deepEqual(issue.labels, ['workshop:decision']);
    assert.equal(issue.state, 'closed');
    assert.equal(issue.state_reason, 'completed');
    assert.equal(issue.body, item.text);
    assert.ok(issue.title.length <= 100);
    assert.ok(item.text.startsWith(issue.title.replace(/…$/, '')));
  }
  // Oldest decision first, so the bottom line has the lowest number.
  const numbers = listItems(decisions.split('\n')).map((i) => i.issue);
  assert.deepEqual(numbers, numbers.slice().sort((a, b) => b - a));

  // A second run changes nothing anywhere.
  const writes = fake.writes;
  const second = await run(['sync'], { fake, box });
  assert.equal(second.code, 0);
  assert.equal(second.stdout.trim(), `created 0, updated 0, unchanged ${lineCount}`);
  assert.equal(fake.writes, writes);
  assert.equal(box.features(), after.join('\n'));
  assert.equal(box.decisions(), decisions);
  assert.equal(readFileSync(join(REAL, 'features.jsonl'), 'utf8'), featuresBefore, 'the real file is untouched');
});

test('--dry-run writes nothing on GitHub or on disk', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const features = box.features();
  const decisions = box.decisions();
  const result = await run(['sync', '--dry-run'], { fake, box });
  assert.equal(result.code, 0, result.stderr);
  assert.match(result.stdout, /would create label workshop:entry/);
  assert.match(result.stdout, /would create the issue for engine-service/);
  assert.match(result.stdout.trim().split('\n').at(-1), /^would create \d+, would update 0, unchanged 0, labels 7$/);
  assert.equal(fake.log.filter((r) => r.method !== 'GET' && !r.path.includes('access_tokens')).length, 0);
  assert.equal(fake.issues.size, 0);
  assert.equal(box.features(), features);
  assert.equal(box.decisions(), decisions);

  const json = await run(['sync', '--dry-run', '--json'], { fake, box });
  assert.equal(JSON.parse(json.stdout).dryRun, true);
});

test('sync stops cleanly on a rate limit and a rerun finishes without duplicates', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const lineCount = box.features().split('\n').filter((l) => l.trim()).length;
  fake.limitAfterWrites = 40;
  const stopped = await run(['sync'], { fake, box });
  assert.equal(stopped.code, 1);
  assert.match(stopped.stdout, /stopped by GitHub's rate limit, run again after/);
  const numbered = box.features().split('\n').filter((l) => l.includes('"issue"')).length;
  assert.equal(numbered, entryIssues(fake).length, 'every created issue is already written back');

  fake.limitAfterWrites = fake.writes + 50;
  assert.equal((await run(['sync'], { fake, box })).code, 1);
  fake.limitAfterWrites = null;
  const done = await run(['sync'], { fake, box });
  assert.equal(done.code, 0, done.stdout + done.stderr);
  assert.equal(entryIssues(fake).length, lineCount);
  const markers = entryIssues(fake).map((i) => i.body.match(/silo-entry: (\S+) -->/)[1]);
  assert.equal(new Set(markers).size, markers.length);
  const items = listItems(box.decisions().split('\n'));
  assert.ok(items.every((i) => i.issue));
  assert.equal(decisionIssues(fake).length, items.length);
});

test('a created issue whose number was not written back is found by its marker, not created again', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const features = '{"n": 1, "id": "a", "name": "A", "what": "Does a."}\n';
  const box = sandbox(t, { features, decisions: '# D\n\n- 2026-11-16 · owner · One.\n' });
  const issue = fake.addIssue({ title: 'A', body: 'Does a.\n\n<!-- silo-entry: a -->', labels: ['workshop:entry'] });
  const decision = fake.addIssue({ title: '2026-11-16 · owner · One.', body: '2026-11-16 · owner · One.', labels: ['workshop:decision'] });
  const result = await run(['sync'], { fake, box });
  assert.equal(result.code, 0, result.stderr);
  assert.equal(box.features(), `{"n": 1, "id": "a", "name": "A", "what": "Does a.", "issue": ${issue.number}}\n`);
  assert.equal(box.decisions(), `# D\n\n- 2026-11-16 · owner · One. (#${decision.number})\n`);
  assert.equal(fake.issues.size, 2);
  assert.equal(decision.state, 'closed');
});

test('sync updates titles and bodies only: verdict labels and states stay as the owner left them', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const features = [
    '{"n": 1, "id": "a", "name": "A", "what": "Does a.", "verdict": "take"}',
    '{"n": 2, "id": "b", "name": "B", "what": "Does b.", "verdict": null}',
    '{"n": 3, "id": "c", "name": "C", "what": "Does c.", "verdict": null}',
    '',
  ].join('\n');
  const box = sandbox(t, { features, decisions: '# D\n' });
  assert.equal((await run(['sync'], { fake, box })).code, 0);
  const n = (id) => JSON.parse(box.features().split('\n').find((l) => l.includes(`"id": "${id}"`))).issue;
  fake.setVerdictLabel(n('a'), 'refine');
  fake.setVerdictLabel(n('b'), 'drop');
  fake.close(n('b'), 'not_planned');
  fake.close(n('c'), 'completed');
  const other = fake.addIssue({ title: 'Unrelated bug', body: '<!-- silo-entry: a -->' });
  const pr = fake.addIssue({ title: 'A PR', body: '<!-- silo-entry: b -->', labels: ['workshop:entry'], pull_request: true });
  const before = fake.snapshot();

  const edited = box.features().replace('"name": "A"', '"name": "A, renamed"').replace('"what": "Does c."', '"what": "Does c, better."');
  await import('node:fs').then(({ writeFileSync }) => writeFileSync(box.featuresPath, edited));
  const result = await run(['sync'], { fake, box });
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.stdout.trim(), 'created 0, updated 2, unchanged 1');
  assert.deepEqual(fake.snapshot(), before);
  assert.equal(fake.issues.get(n('a')).title, 'A, renamed');
  assert.equal(fake.issues.get(n('c')).body, 'Does c, better.\n\n<!-- silo-entry: c -->');
  const patches = fake.log.filter((r) => r.method === 'PATCH');
  assert.ok(patches.every((r) => Object.keys(r.body).sort().join() === 'body,title'));
  assert.equal(other.title, 'Unrelated bug');
  assert.equal(pr.title, 'A PR');
  assert.equal(box.features(), edited, 'the file already had every number');
});

test('a line naming an issue that is not its Workshop entry stops sync', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const other = fake.addIssue({ title: 'Unrelated' });
  const box = sandbox(t, { features: `{"id": "a", "name": "A", "what": "x", "issue": ${other.number}}\n`, decisions: '' });
  const result = await run(['sync'], { fake, box });
  assert.equal(result.code, 1);
  assert.match(result.stderr, /not a Workshop entry/);
  assert.equal(other.title, 'Unrelated');
});

test('a secondary rate limit (403 with a rate-limit message) also stops sync cleanly', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t, { features: '{"id": "a", "name": "A", "what": "x"}\n', decisions: '' });
  const original = globalThis.fetch;
  const fetch = async (url, init) => {
    if (init.method === 'POST' && String(url).endsWith('/issues')) {
      return new Response(JSON.stringify({ message: 'You have exceeded a secondary rate limit.' }), { status: 403 });
    }
    return original(url, init);
  };
  const result = await run(['sync'], { fake, box, overrides: { fetch } });
  assert.equal(result.code, 1);
  assert.match(result.stdout, /stopped by GitHub's rate limit/);
  assert.equal(box.features(), '{"id": "a", "name": "A", "what": "x"}\n');
});
