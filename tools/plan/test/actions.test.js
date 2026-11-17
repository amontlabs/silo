import { test } from 'node:test';
import assert from 'node:assert/strict';
import { basePlan } from './helpers.js';
import { fakeKeychain, world } from './github-helpers.js';
import { CLEARED_COMMENT, RECORDED_COMMENT } from '../model/comments.js';

const taken = () => {
  const files = basePlan();
  files['features/core.json'] = { ...files['features/core.json'], verdict: 'take', built: '2026-11-01' };
  files['features/cli.json'] = { ...files['features/cli.json'], verdict: 'take' };
  files['features/fork.json'] = { ...files['features/fork.json'], verdict: 'take', needs: ['core', 'cli'] };
  return files;
};

async function synced(t, files = basePlan()) {
  const w = await world(t, files);
  assert.equal((await w.run(['sync'])).code, 0);
  return w;
}

const verdictLabels = (issue) => issue.labels.filter((l) => l.startsWith('verdict:'));

test('decide: the bot sets the label and comments; the file is never written; pull counts it', async (t) => {
  const w = await synced(t);
  const fork = w.fake.entry('fork');
  const take = await w.run(['decide', 'fork', 'take']);
  assert.equal(take.code, 0, take.err);
  assert.equal(take.out, 'Recorded on GitHub. The commit follows: git pull.');
  assert.deepEqual(verdictLabels(fork), ['verdict:take']);
  assert.deepEqual(w.fake.commentsOn(fork.number).map((c) => [c.user.login, c.body]), [[w.fake.bot.login, RECORDED_COMMENT]]);
  assert.equal(w.feature('fork').verdict, undefined);

  const events = w.fake.events.length;
  assert.equal((await w.run(['decide', 'fork', 'drop'])).code, 0);
  assert.deepEqual(verdictLabels(fork), ['verdict:drop']);
  // The new label goes on before the old one comes off: never a moment with no verdict.
  assert.deepEqual(w.fake.events.slice(events).map((e) => `${e.event} ${e.label.name}`), ['labeled verdict:drop', 'unlabeled verdict:take']);
  const pulled = JSON.parse((await w.run(['pull', '--json'])).out);
  assert.deepEqual(pulled.changes.map((c) => [c.id, c.to, c.by]), [['fork', 'drop', w.fake.bot.login]]);

  const clear = await w.run(['decide', 'fork', 'clear', '--json']);
  assert.equal(clear.code, 0);
  assert.equal(JSON.parse(clear.out).changed, true);
  assert.deepEqual(verdictLabels(fork), []);
  assert.equal(w.fake.commentsOn(fork.number).at(-1).body, CLEARED_COMMENT);
  const comments = w.fake.comments.length;
  assert.equal((await w.run(['decide', 'fork', 'clear'])).code, 0);
  assert.equal(w.fake.comments.length, comments, 'nothing to change: nothing posted');
});

test('decide: exit 3 without credentials, refused on a built feature or one without an issue', async (t) => {
  const files = taken();
  const w = await synced(t, files);
  const none = await w.run(['decide', 'fork', 'take'], { deps: { keychain: fakeKeychain() } });
  assert.equal(none.code, 3);
  assert.match(none.err, /missing WORKSHOP_APP_ID/);
  const down = await w.run(['decide', 'fork', 'take'], { deps: { api: 'http://127.0.0.1:9' } });
  assert.equal(down.code, 3);
  assert.match(down.err, /could not reach GitHub/);
  const built = await w.run(['decide', 'core', 'drop']);
  assert.equal(built.code, 1);
  assert.match(built.err, /core is built \(2026-11-01\); its verdict never changes/);
  w.fake.close(w.fake.entry('cli').number, 'completed');
  assert.match((await w.run(['decide', 'cli', 'drop'])).err, /is closed as completed\); its verdict never changes/);
  assert.equal(w.fake.events.filter((e) => e.label?.name?.startsWith('verdict:')).length, 0, 'nothing recorded');
  assert.equal(w.fake.comments.length, 0);
});

test('decide: a feature whose issue does not exist yet is refused', async (t) => {
  const w = await world(t, basePlan());
  const result = await w.run(['decide', 'fork', 'take']);
  assert.equal(result.code, 1);
  assert.match(result.err, /no issue on GitHub yet/);
});

test('start: refusals from the file, the claim, a second claim refused with the branch', async (t) => {
  const files = taken();
  files['features/old.json'] = { ...files['features/old.json'], verdict: 'later' };
  const w = await synced(t, files);
  assert.match((await w.run(['start', 'old', '--branch', 'b'])).err, /no verdict take/);
  assert.match((await w.run(['start', 'fork', '--branch', 'b'])).err, /needs cli, not built yet/);
  assert.match((await w.run(['start', 'core', '--branch', 'b'])).err, /built already/);
  assert.equal((await w.run(['start', 'nobody'])).code, 1);

  const cli = w.fake.entry('cli');
  const claim = await w.run(['start', 'cli']); // the checkout's branch
  assert.equal(claim.code, 0, claim.err);
  assert.equal(claim.out, `Claimed cli on #${cli.number}: being built on main.`);
  assert.ok(cli.labels.includes('workshop:building'));
  assert.deepEqual(w.fake.commentsOn(cli.number).map((c) => c.body), ['Being built on main.']);
  const again = await w.run(['start', 'cli', '--branch', 'feat/other']);
  assert.equal(again.code, 1);
  assert.match(again.err, /being built already on main/);
  const status = JSON.parse((await w.run(['status', '--json'])).out);
  assert.deepEqual(status.building, [{ id: 'cli', name: 'CLI', branch: 'main' }]);
});

test('start: the lost race removes its own comment and exits 1; the winner keeps the claim', async (t) => {
  const w = await synced(t, taken());
  const cli = w.fake.entry('cli');
  w.fake.afterWrite = ({ method, path }) => {
    // The other session's claim lands between this one's label and its comment.
    if (method === 'POST' && path === `/issues/${cli.number}/labels`) {
      w.fake.afterWrite = null;
      w.fake.addComment(cli.number, w.fake.bot.login, 'Being built on feat/first.', { type: 'Bot' });
    }
  };
  const lost = await w.run(['start', 'cli', '--branch', 'feat/second']);
  assert.equal(lost.code, 1);
  assert.match(lost.err, /another session claimed cli first, on feat\/first/);
  assert.deepEqual(w.fake.commentsOn(cli.number).map((c) => c.body), ['Being built on feat/first.']);
  assert.ok(cli.labels.includes('workshop:building'));
});

test('start: a claim older than the label (a stopped build) does not count', async (t) => {
  const w = await synced(t, taken());
  const cli = w.fake.entry('cli');
  assert.equal((await w.run(['start', 'cli', '--branch', 'feat/old'])).code, 0);
  assert.equal((await w.run(['stop', 'cli'])).code, 0);
  const result = await w.run(['start', 'cli', '--branch', 'feat/new']);
  assert.equal(result.code, 0, result.err);
  assert.deepEqual(w.fake.commentsOn(cli.number).map((c) => c.body), ['Being built on feat/old.', 'Being built on feat/new.']);
});

test('start without GitHub: the claim is not shared, exit 0', async (t) => {
  const w = await synced(t, taken());
  const result = await w.run(['start', 'cli', '--branch', 'b'], { deps: { keychain: fakeKeychain() } });
  assert.equal(result.code, 0);
  assert.match(result.out, /The claim is not shared: missing WORKSHOP_APP_ID/);
});

test('stop removes the building label; on a feature not being built it says so and exits 0', async (t) => {
  const w = await synced(t, taken());
  const cli = w.fake.entry('cli');
  const idle = await w.run(['stop', 'cli']);
  assert.equal(idle.code, 0);
  assert.match(idle.out, /cli is not being built/);
  await w.run(['start', 'cli', '--branch', 'b']);
  const stopped = await w.run(['stop', 'cli']);
  assert.equal(stopped.code, 0);
  assert.match(stopped.out, /no longer being built/);
  assert.ok(!cli.labels.includes('workshop:building'));
});

test('decline closes a suggestion as not planned with the reason as the bot\'s comment', async (t) => {
  const w = await synced(t);
  const suggestion = w.fake.addIssue({ title: 'Idea', labels: ['workshop:suggestion'], user: { login: 'visitor', type: 'User' } });
  const result = await w.run(['decline', `#${suggestion.number}`, '--why', 'Out of scope for Silo.']);
  assert.equal(result.code, 0, result.err);
  assert.deepEqual([suggestion.state, suggestion.state_reason], ['closed', 'not_planned']);
  assert.deepEqual(w.fake.commentsOn(suggestion.number).map((c) => [c.user.login, c.body]), [[w.fake.bot.login, 'Out of scope for Silo.']]);
  assert.match((await w.run(['decline', String(suggestion.number), '--why', 'Again.'])).err, /closed already/);
  assert.match((await w.run(['decline', String(w.fake.entry('fork').number), '--why', 'No.'])).err, /is not a suggestion/);
  assert.match((await w.run(['decline', '999', '--why', 'No.'])).err, /is not a suggestion/);
});
