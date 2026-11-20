import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startFake } from './fake-github.js';
import { run, sandbox } from './setup-helpers.js';

test('decline closes a suggestion as not planned with the reason as the bot\'s comment, once', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const suggestion = fake.addIssue({ title: 'Idea', labels: ['workshop:suggestion'], user: { login: 'visitor', type: 'User' } });
  const entry = fake.addIssue({ title: 'Fork', body: 'Copies.\n\n<!-- silo-entry: fork -->', labels: ['workshop:entry'], user: fake.bot });
  const plain = fake.addIssue({ title: 'Bug', labels: [], user: { login: 'visitor', type: 'User' } });

  const result = await run(['decline', `#${suggestion.number}`, '--why', 'Out of scope for Silo.'], { fake, box });
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.stdout, `Closed #${suggestion.number} as not planned.\n`);
  assert.deepEqual([suggestion.state, suggestion.state_reason], ['closed', 'not_planned']);
  assert.deepEqual(fake.commentsOn(suggestion.number).map((c) => [c.user.login, c.body]), [[fake.bot.login, 'Out of scope for Silo.']]);

  const again = await run(['decline', String(suggestion.number), '--why', 'Again.'], { fake, box });
  assert.equal(again.code, 1);
  assert.match(again.stderr, /closed already/);
  for (const number of [entry.number, plain.number, 999]) {
    const refused = await run(['decline', String(number), '--why', 'No.'], { fake, box });
    assert.equal(refused.code, 1);
    assert.match(refused.stderr, /is not a suggestion/);
  }
  assert.equal(fake.commentsOn(suggestion.number).length, 1);
  assert.deepEqual([entry.state, plain.state], ['open', 'open']);
});

test('decline after a run stopped between the comment and the close posts no second comment', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const suggestion = fake.addIssue({ title: 'Idea', labels: ['workshop:suggestion'], user: { login: 'visitor', type: 'User' } });
  fake.addComment(suggestion.number, fake.bot.login, 'Out of scope.', { type: 'Bot' });
  assert.equal((await run(['decline', String(suggestion.number), '--why', 'Out of scope.'], { fake, box })).code, 0);
  assert.equal(fake.commentsOn(suggestion.number).length, 1);
  assert.equal(suggestion.state, 'closed');
});

test('decline: wrong use exits 2, no credentials exits 3, and nothing changes', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const suggestion = fake.addIssue({ title: 'Idea', labels: ['workshop:suggestion'], user: { login: 'visitor', type: 'User' } });
  for (const argv of [['decline', 'x', '--why', 'No.'], ['decline', '4'], ['decline', '4', '--why', ' '], ['decline', '--why', 'No.'], ['nonsense']]) {
    assert.equal((await run(argv, { fake, box })).code, 2, argv.join(' '));
  }
  const unconfigured = await run(['decline', String(suggestion.number), '--why', 'No.'], { fake, box, keychain: { available: () => true, get: () => null, set() {} } });
  assert.equal(unconfigured.code, 3);
  assert.match(unconfigured.stderr, /missing WORKSHOP_APP_ID and WORKSHOP_PRIVATE_KEY: run `workshop setup`/);
  assert.equal(suggestion.state, 'open');
  assert.equal(fake.commentsOn(suggestion.number).length, 0);
});

test('workshop alone lists its four commands, one line each', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  const box = sandbox(t);
  const names = ['setup', 'store', 'doctor', 'decline'];
  const { code, stdout } = await run([], { fake, box });
  assert.equal(code, 0);
  for (const name of names) assert.match(stdout, new RegExp(`^  workshop ${name}`, 'm'));
  assert.deepEqual(JSON.parse((await run(['--json'], { fake, box })).stdout).commands.map((item) => item.name), names);
});
