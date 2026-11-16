import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { startFake } from './fake-github.js';
import { fakeKeychain, run, sandbox } from './helpers.js';

const FEATURES = '{"n": 1, "id": "a", "name": "A", "what": "Does a."}\n{"n": 2, "id": "b", "name": "B", "what": "Does b."}\n';

async function world(t) {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t, { features: FEATURES, decisions: '# D\n' });
  return { fake, box };
}

test('ask opens a decision issue as the bot, from text, a file or stdin, optionally about an entry', async (t) => {
  const { fake, box } = await world(t);
  assert.equal((await run(['sync'], { fake, box })).code, 0);
  const a = JSON.parse(box.features().split('\n')[0]).issue;

  const plain = await run(['ask', '--title', 'Which first?', '--body', 'A or B?'], { fake, box });
  assert.equal(plain.code, 0, plain.stderr);
  const number = Number(plain.stdout.match(/#(\d+)/)[1]);
  const issue = fake.issues.get(number);
  assert.deepEqual(issue.labels, ['workshop:decision']);
  assert.equal(issue.user.login, 'silo-workshop[bot]');
  assert.equal(issue.state, 'open');
  assert.equal(issue.body, 'A or B?');

  const file = join(box.base, 'body.md');
  writeFileSync(file, 'From a file.\n');
  const fromFile = JSON.parse((await run(['ask', '--title', 'T', '--body', `@${file}`, '--entry', 'a', '--json'], { fake, box })).stdout);
  assert.equal(fake.issues.get(fromFile.number).body, `From a file.\n\nAbout: #${a}`);

  const fromStdin = JSON.parse((await run(['ask', '--title=T', '--body', '-', '--json'], { fake, box, stdin: 'Piped.' })).stdout);
  assert.equal(fake.issues.get(fromStdin.number).body, 'Piped.');

  assert.equal((await run(['ask', '--title', 'T', '--body', 'x', '--entry', 'nope'], { fake, box })).code, 2);
  assert.equal((await run(['ask', '--body', 'x'], { fake, box })).code, 2);
  assert.equal((await run(['ask', '--title', 'x'.repeat(121), '--body', 'x'], { fake, box })).code, 2);
});

test('comment posts as the bot only on Workshop issues', async (t) => {
  const { fake, box } = await world(t);
  const entry = fake.addIssue({ title: 'E', labels: ['workshop:entry'] });
  const suggestion = fake.addIssue({ title: 'S', labels: ['workshop:suggestion'] });
  const other = fake.addIssue({ title: 'Bug', labels: ['bug', 'verdict:take'] });
  const pr = fake.addIssue({ title: 'PR', labels: ['workshop:entry'], pull_request: true });

  for (const target of [entry, suggestion]) {
    const result = await run(['comment', String(target.number), '--text', 'Noted.'], { fake, box });
    assert.equal(result.code, 0, result.stderr);
  }
  assert.deepEqual(fake.comments.map((c) => [c.issue, c.user.login, c.body]), [
    [entry.number, 'silo-workshop[bot]', 'Noted.'],
    [suggestion.number, 'silo-workshop[bot]', 'Noted.'],
  ]);
  for (const target of [other.number, pr.number, 999]) {
    const refused = await run(['comment', String(target), '--text', 'No.'], { fake, box });
    assert.equal(refused.code, 1);
    assert.match(refused.stderr, /is not a Workshop issue/);
  }
  assert.equal(fake.comments.length, 2);
  assert.equal(fake.log.filter((r) => r.method === 'POST' && r.path.endsWith('/comments')).length, 2);
  assert.equal((await run(['comment', 'x', '--text', 'a'], { fake, box })).code, 2);
  assert.equal((await run(['comment', '1'], { fake, box })).code, 2);
});

test('built closes an entry as completed, once; it refuses other issues and dropped entries', async (t) => {
  const { fake, box } = await world(t);
  const entry = fake.addIssue({ title: 'E', body: 'x\n\n<!-- silo-entry: a -->', labels: ['workshop:entry', 'verdict:take'] });
  const decision = fake.addIssue({ title: 'D', labels: ['workshop:decision'] });
  const dropped = fake.addIssue({ title: 'X', labels: ['workshop:entry', 'verdict:drop'] });
  fake.close(dropped.number, 'not_planned');

  const done = await run(['built', String(entry.number), '--json'], { fake, box });
  assert.equal(done.code, 0, done.stderr);
  assert.deepEqual(JSON.parse(done.stdout), { number: entry.number, entry: 'a', changed: true, closed: true, unlabeled: false });
  assert.deepEqual([entry.state, entry.state_reason], ['closed', 'completed']);
  assert.deepEqual(entry.labels, ['workshop:entry', 'verdict:take']);
  assert.match((await run(['built', String(entry.number)], { fake, box })).stdout, /already closed/);

  assert.equal((await run(['built', String(decision.number)], { fake, box })).code, 1);
  assert.equal(decision.state, 'open');
  const refused = await run(['built', String(dropped.number)], { fake, box });
  assert.equal(refused.code, 1);
  assert.equal(dropped.state_reason, 'not_planned');
});

test('building marks an open taken entry, once; --stop removes the mark; built removes it too', async (t) => {
  const { fake, box } = await world(t);
  const taken = fake.addIssue({ title: 'E', body: 'x\n\n<!-- silo-entry: a -->', labels: ['workshop:entry', 'verdict:take'] });
  const writes = () => fake.log.filter((r) => r.method !== 'GET' && !r.path.includes('access_tokens')).length;

  const start = await run(['building', String(taken.number), '--json'], { fake, box });
  assert.equal(start.code, 0, start.stderr);
  assert.deepEqual(fake.labels, [{ name: 'workshop:building', color: '8250df', description: 'Being built now' }], 'the label is made first, as sync makes it');
  const order = fake.log.filter((r) => r.method === 'POST' && r.path.endsWith('/labels')).map((r) => r.path.replace(/^\/repos\/[^/]+\/[^/]+/, ''));
  assert.deepEqual(order, ['/labels', `/issues/${taken.number}/labels`]);
  assert.deepEqual(JSON.parse(start.stdout), { number: taken.number, entry: 'a', building: true, changed: true });
  assert.deepEqual(taken.labels, ['workshop:entry', 'verdict:take', 'workshop:building']);
  const before = writes();
  const again = await run(['building', String(taken.number), '--json'], { fake, box });
  assert.equal(JSON.parse(again.stdout).changed, false);
  assert.match((await run(['building', String(taken.number)], { fake, box })).stdout, /already marked as being built/);
  assert.equal(writes(), before, 'a second run writes nothing');

  taken.labels.pop();
  await run(['building', String(taken.number)], { fake, box });
  assert.equal(fake.labels.length, 1, 'an existing label is not made again');
  assert.equal(fake.log.filter((r) => r.method === 'POST' && r.path.endsWith('/repos/amontlabs/silo/labels')).length, 1);
  const stop = await run(['building', String(taken.number), '--stop', '--json'], { fake, box });
  assert.deepEqual(JSON.parse(stop.stdout), { number: taken.number, entry: 'a', building: false, changed: true });
  assert.deepEqual(taken.labels, ['workshop:entry', 'verdict:take']);
  const stopped = writes();
  assert.match((await run(['building', String(taken.number), '--stop'], { fake, box })).stdout, /was not marked as being built/);
  assert.equal(writes(), stopped);

  await run(['building', String(taken.number)], { fake, box });
  const done = await run(['built', String(taken.number), '--json'], { fake, box });
  assert.deepEqual(JSON.parse(done.stdout), { number: taken.number, entry: 'a', changed: true, closed: true, unlabeled: true });
  assert.deepEqual([taken.state, taken.state_reason, taken.labels], ['closed', 'completed', ['workshop:entry', 'verdict:take']]);

  // A label left on an entry already built (added on GitHub, or a run cut after the close) goes on the next `built`.
  taken.labels.push('workshop:building');
  const cleared = await run(['built', String(taken.number)], { fake, box });
  assert.match(cleared.stdout, /already closed as built\. Removed its building label\./);
  assert.deepEqual(taken.labels, ['workshop:entry', 'verdict:take']);
  assert.deepEqual(JSON.parse((await run(['built', String(taken.number), '--json'], { fake, box })).stdout).changed, false);
});

test('building refuses what the owner has not taken, closed entries and other issues; --stop clears a dropped entry', async (t) => {
  const { fake, box } = await world(t);
  const undecided = fake.addIssue({ title: 'U', labels: ['workshop:entry'] });
  const later = fake.addIssue({ title: 'L', labels: ['workshop:entry', 'verdict:later'] });
  const built = fake.addIssue({ title: 'B', labels: ['workshop:entry', 'verdict:take'] });
  fake.close(built.number, 'completed');
  const decision = fake.addIssue({ title: 'D', labels: ['workshop:decision', 'verdict:take'] });
  const other = fake.addIssue({ title: 'Bug', labels: ['bug', 'verdict:take'] });
  const dropped = fake.addIssue({ title: 'X', labels: ['workshop:entry', 'verdict:drop', 'workshop:building'] });
  fake.close(dropped.number, 'not_planned');

  for (const issue of [undecided, later]) {
    const refused = await run(['building', String(issue.number)], { fake, box });
    assert.equal(refused.code, 1);
    assert.equal(refused.stderr.trim(), `workshop: #${issue.number} is not planned: the owner has not taken it`);
  }
  assert.match((await run(['building', String(built.number)], { fake, box })).stderr, /is not an open Workshop entry/);
  assert.match((await run(['building', String(decision.number)], { fake, box })).stderr, /is not a Workshop entry/);
  assert.match((await run(['building', String(other.number)], { fake, box })).stderr, /is not a Workshop issue/);
  assert.equal((await run(['building', '999'], { fake, box })).code, 1);
  assert.equal(fake.log.filter((r) => r.method === 'POST' && r.path.endsWith('/labels')).length, 0, 'nothing was labelled');

  const cleared = await run(['building', String(dropped.number), '--stop', '--json'], { fake, box });
  assert.equal(cleared.code, 0, cleared.stderr);
  assert.equal(JSON.parse(cleared.stdout).changed, true);
  assert.deepEqual([dropped.labels, dropped.state_reason], [['workshop:entry', 'verdict:drop'], 'not_planned']);
  for (const argv of [['building'], ['building', 'x'], ['building', '1', '--stop=yes'], ['building', '1', '2']]) {
    assert.equal((await run(argv, { fake, box })).code, 2, argv.join(' '));
  }
});

test('sync creates the building label with the others, and doctor lists it', async (t) => {
  const { fake, box } = await world(t);
  assert.equal((await run(['sync'], { fake, box })).code, 0);
  assert.deepEqual(fake.labels.find((label) => label.name === 'workshop:building'), { name: 'workshop:building', color: '8250df', description: 'Being built now' });
  assert.match((await run(['doctor'], { fake, box, env: { WORKSHOP_APP_ID: fake.appId } })).stdout, /^Label workshop:building: yes$/m);
  fake.labels = fake.labels.filter((label) => label.name !== 'workshop:building');
  const missing = await run(['doctor'], { fake, box, env: { WORKSHOP_APP_ID: fake.appId } });
  assert.equal(missing.code, 1);
  assert.match(missing.stdout, /^Problem: labels missing \(sync creates them\): workshop:building$/m);
});

test('doctor reports credentials by source, the installation, permissions and labels, without values', async (t) => {
  const { fake, box } = await world(t);
  const keychain = fakeKeychain({ WORKSHOP_PRIVATE_KEY: (await import('./fake-github.js')).appKeys().privateKey, WORKSHOP_CLIENT_SECRET: 'cs-secret-value' });
  const env = { WORKSHOP_APP_ID: fake.appId };
  const first = await run(['doctor', '--json'], { fake, box, keychain, env });
  assert.equal(first.code, 1);
  const report = JSON.parse(first.stdout);
  assert.deepEqual(report.credentials, { WORKSHOP_APP_ID: 'environment', WORKSHOP_CLIENT_ID: 'missing', WORKSHOP_CLIENT_SECRET: 'keychain', WORKSHOP_PRIVATE_KEY: 'keychain' });
  assert.equal(report.installed, true);
  assert.equal(report.public, true);
  assert.deepEqual(report.permissions, { issues: 'write', metadata: 'read' });
  assert.equal(Object.values(report.labels).filter(Boolean).length, 0);
  assert.ok(!first.stdout.includes('cs-secret-value'));

  await run(['sync'], { fake, box });
  const healthy = await run(['doctor'], { fake, box, keychain, env });
  assert.equal(healthy.code, 0, healthy.stdout);
  assert.match(healthy.stdout, /Label verdict:drop: yes/);
  assert.match(healthy.stdout, /^Public: yes$/m);
  assert.ok(fake.log.some((r) => r.path === '/apps/silo-workshop' && r.auth === null), 'asked as an outsider');

  fake.public = false;
  const privateApp = await run(['doctor'], { fake, box, keychain, env });
  assert.equal(privateApp.code, 1);
  assert.match(privateApp.stdout, /^Public: no \(sign-in will not work for anyone outside its organization\)$/m);
  assert.match(privateApp.stdout, /Problem: the App is private: .*sign-in will not work for anyone else/);
  fake.public = true;

  fake.installed = false;
  const missing = await run(['doctor', '--json'], { fake, box, keychain, env });
  assert.equal(missing.code, 1);
  const notInstalled = JSON.parse(missing.stdout);
  assert.match(notInstalled.problems[0], /not installed.*installations\/new/);
  assert.equal(notInstalled.public, true, 'checked even before the App is installed');
});

test('commands that need GitHub fail plainly when credentials are missing or the App is not installed', async (t) => {
  const { fake, box } = await world(t);
  const none = await run(['sync'], { fake, box, keychain: fakeKeychain() });
  assert.equal(none.code, 1);
  assert.equal(none.stderr.trim().split('\n').length, 1);
  assert.match(none.stderr, /missing WORKSHOP_APP_ID and WORKSHOP_PRIVATE_KEY/);
  fake.installed = false;
  const uninstalled = await run(['pull'], { fake, box });
  assert.equal(uninstalled.code, 1);
  assert.match(uninstalled.stderr, /not installed on amontlabs\/silo/);
});

test('the installation token is narrowed to the repository and to issues', async (t) => {
  const { fake, box } = await world(t);
  await run(['built', '1'], { fake, box });
  assert.deepEqual(fake.tokenRequests[0], { repositories: ['silo'], permissions: { issues: 'write', metadata: 'read' } });
});

test('usage errors exit 2', async (t) => {
  const { fake, box } = await world(t);
  for (const argv of [['nope'], ['sync', 'extra'], ['pull', '--force'], ['built'], ['built', '0'], ['setup', '--org']]) {
    assert.equal((await run(argv, { fake, box })).code, 2, argv.join(' '));
  }
  assert.equal((await run(['--help'], { fake, box })).code, 0);
});
