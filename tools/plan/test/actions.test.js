import { test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { basePlan, git } from './helpers.js';
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

test('decide: GitHub and the file at origin/main show the verdict already: "already recorded", nothing changes', async (t) => {
  const files = basePlan();
  files['features/fork.json'] = { ...files['features/fork.json'], verdict: 'take' };
  const w = await synced(t, files);
  const fork = w.fake.entry('fork');
  w.fake.setVerdictLabel(fork.number, 'take', 'owner');
  const before = w.fake.snapshot();
  const events = w.fake.events.length;
  const result = await w.run(['decide', 'fork', 'take']);
  assert.equal(result.code, 0, result.err);
  assert.equal(result.out, `Already recorded: #${fork.number} and the file at origin/main both show take.`);
  assert.equal(w.fake.snapshot(), before);
  assert.equal(w.fake.events.length, events);
  const cleared = await w.run(['decide', 'core', 'clear', '--json']);
  assert.deepEqual([JSON.parse(cleared.out).changed, JSON.parse(cleared.out).recorded], [false, true]);
});

test('decide: the label is there and counts, the file at origin/main differs: nothing changes, the next run of the Plan workflow brings it', async (t) => {
  const w = await synced(t);
  const fork = w.fake.entry('fork');
  // Set by the owner, but its run was lost: the file never got it.
  w.fake.setVerdictLabel(fork.number, 'take', 'owner');
  const before = w.fake.snapshot();
  const events = w.fake.events.length;
  const result = await w.run(['decide', 'fork', 'take']);
  assert.equal(result.code, 0, result.err);
  assert.equal(result.out, 'GitHub already shows take. The commit comes with the next run of the Plan workflow (any push to main starts one).');
  assert.equal(w.fake.events.length, events, 'no label removed and added again');
  assert.equal(w.fake.snapshot(), before);
  const json = JSON.parse((await w.run(['decide', 'fork', 'take', '--json'])).out);
  assert.deepEqual([json.changed, json.recorded, json.waiting], [false, false, true]);
  const pulled = JSON.parse((await w.run(['pull', '--write', '--json'])).out);
  assert.deepEqual(pulled.changes.map((c) => [c.id, c.to, c.by]), [['fork', 'take', 'owner']]);

  // clear, when GitHub shows no verdict and the file at origin/main still has one: the label goes on, then off.
  w.change({ cli: { verdict: 'later' } }, 'Owner: later cli');
  const cli = w.fake.entry('cli');
  const from = w.fake.events.length;
  assert.equal((await w.run(['decide', 'cli', 'clear'])).code, 0);
  assert.deepEqual(w.fake.events.slice(from).map((e) => `${e.event} ${e.label.name}`), ['labeled verdict:later', 'unlabeled verdict:later']);
  assert.deepEqual(verdictLabels(cli), []);
  assert.equal(w.fake.commentsOn(cli.number).at(-1).body, CLEARED_COMMENT);
  const cleared = JSON.parse((await w.run(['pull', '--json'])).out);
  assert.deepEqual(cleared.changes.map((c) => [c.id, c.to, c.by]), [['cli', null, w.fake.bot.login]]);
});

test('decide: the wanted label is there but set by someone who cannot decide: nothing changes, exit 1', async (t) => {
  const w = await synced(t);
  const fork = w.fake.entry('fork');
  w.fake.label(fork.number, 'verdict:take', 'helper');
  const expected = `GitHub shows take on #${fork.number}, set by helper, who cannot decide. Remove that label on GitHub, then run plan decide again.`;
  for (const beside of [null, 'later']) {
    if (beside) w.fake.label(fork.number, `verdict:${beside}`, 'owner');
    const before = w.fake.snapshot();
    const events = w.fake.events.length;
    const result = await w.run(['decide', 'fork', 'take']);
    assert.equal(result.code, 1);
    assert.equal(result.err.trim(), `plan: ${expected}`);
    assert.equal(w.fake.events.length, events);
    assert.equal(w.fake.snapshot(), before);
  }
  // Once the owner sets it themselves, it counts.
  w.fake.unlabel(fork.number, 'verdict:take', 'owner');
  w.fake.label(fork.number, 'verdict:take', 'owner');
  const result = await w.run(['decide', 'fork', 'take']);
  assert.equal(result.code, 0, result.err);
  assert.deepEqual(verdictLabels(fork), ['verdict:take']);
  assert.equal(w.fake.commentsOn(fork.number).at(-1).body, RECORDED_COMMENT);
});

test('decide: origin/main cannot be fetched: it says so and changes nothing', async (t) => {
  const w = await synced(t);
  git(w.root, 'remote', 'set-url', 'origin', join(w.base, 'missing.git'));
  const before = w.fake.snapshot();
  const result = await w.run(['decide', 'fork', 'take']);
  assert.equal(result.code, 1);
  assert.match(result.err, /the file of fork at origin\/main could not be read \(git fetch origin main failed/);
  assert.equal(w.fake.snapshot(), before);
});

test('decide clear: removes the label under the name the issue lists; nothing to clear; a failed removal is an error', async (t) => {
  const w = await synced(t);
  const fork = w.fake.entry('fork');
  w.fake.labels.push({ name: 'Verdict:Later', color: 'ededed', description: '' });
  w.fake.labels = w.fake.labels.filter((l) => l.name !== 'verdict:later');
  w.fake.label(fork.number, 'verdict:later', 'owner');
  assert.deepEqual(fork.labels.filter((l) => /verdict/i.test(l)), ['Verdict:Later']);
  const cleared = await w.run(['decide', 'fork', 'clear']);
  assert.equal(cleared.code, 0, cleared.err);
  assert.equal(cleared.out, 'Recorded on GitHub. The commit follows: git pull.');
  assert.deepEqual(fork.labels.filter((l) => /verdict/i.test(l)), []);
  assert.equal(w.fake.commentsOn(fork.number).at(-1).body, CLEARED_COMMENT);

  // No label on the issue, none in the file.
  const before = w.fake.snapshot();
  const nothing = await w.run(['decide', 'fork', 'clear']);
  assert.equal(nothing.code, 0, nothing.err);
  assert.equal(nothing.out, `There is no verdict to clear: #${fork.number} and the file at origin/main both show none.`);
  assert.equal(w.fake.snapshot(), before);

  // Someone removes the label while decide runs: the removal fails, nothing is recorded.
  w.fake.label(fork.number, 'verdict:take', 'owner');
  const comments = w.fake.commentsOn(fork.number).length;
  const fetch = (url, init) => {
    if (init?.method === 'DELETE' && String(url).includes('/labels/')) w.fake.unlabel(fork.number, 'verdict:take', 'owner');
    return w.deps.fetch(url, init);
  };
  const failed = await w.run(['decide', 'fork', 'clear'], { deps: { fetch } });
  assert.equal(failed.code, 1);
  assert.match(failed.err, /verdict:take could not be removed from #\d+: GitHub does not show it there; nothing is recorded/);
  assert.doesNotMatch(failed.out, /Recorded/);
  assert.equal(w.fake.commentsOn(fork.number).length, comments);
});

test('start: two claims under one branch name: the earliest comment wins, the other withdraws', async (t) => {
  const w = await synced(t, taken());
  const cli = w.fake.entry('cli');
  w.fake.afterWrite = ({ method, path }) => {
    if (method === 'POST' && path === `/issues/${cli.number}/labels`) {
      w.fake.afterWrite = null;
      w.fake.addComment(cli.number, w.fake.bot.login, 'Being built on feat/cli.', { type: 'Bot' });
    }
  };
  const lost = await w.run(['start', 'cli', '--branch', 'feat/cli']);
  assert.equal(lost.code, 1);
  assert.match(lost.err, /another session claimed cli first, on feat\/cli/);
  assert.deepEqual(w.fake.commentsOn(cli.number).map((c) => c.body), ['Being built on feat/cli.']);
});

test('start and stop read the building label in any capitalisation, and remove it under its own name', async (t) => {
  const w = await synced(t, taken());
  const cli = w.fake.entry('cli');
  w.fake.labels.push({ name: 'Workshop:Building', color: 'ededed', description: '' });
  w.fake.labels = w.fake.labels.filter((l) => l.name !== 'workshop:building');
  w.fake.label(cli.number, 'workshop:building', 'owner');
  assert.deepEqual(cli.labels.filter((l) => /building/i.test(l)), ['Workshop:Building']);
  const again = await w.run(['start', 'cli', '--branch', 'b']);
  assert.equal(again.code, 1);
  assert.match(again.err, /being built already/);
  const stopped = await w.run(['stop', 'cli']);
  assert.equal(stopped.code, 0, stopped.err);
  assert.match(stopped.out, /no longer being built/);
  assert.deepEqual(cli.labels.filter((l) => /building/i.test(l)), []);
});
