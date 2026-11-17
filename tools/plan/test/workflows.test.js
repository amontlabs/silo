// The workflow cannot run before the owner creates the App; these tests hold what can be checked
// here: its triggers, history, permissions, group and steps, and the push that tries again on a
// main that moved, run against the fake GitHub.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { basePlan, git } from './helpers.js';
import { world } from './github-helpers.js';
import { appKeys } from './fake-github.js';
import { canonicalFeature, canonicalText } from '../model/canonical.js';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const BIN = join(ROOT, 'tools', 'plan', 'bin');
const workflow = (name) => readFileSync(join(ROOT, '.github', 'workflows', name), 'utf8');

test('plan.yml: one job for both directions; verdict labels only; main with its history; group plan never cancelled', () => {
  assert.ok(!existsSync(join(ROOT, '.github', 'workflows', 'plan-sync.yml')));
  assert.ok(!existsSync(join(ROOT, '.github', 'workflows', 'plan-verdict.yml')));
  const text = workflow('plan.yml');
  assert.match(text, /on:\n {2}push:\n {4}branches: \[main\]\n {2}issues:\n {4}types: \[labeled, unlabeled\]\n {2}workflow_dispatch:/);
  assert.match(text, /if: github\.event_name != 'issues' \|\| contains\(fromJSON\('\["verdict:take","verdict:later","verdict:drop"\]'\), github\.event\.label\.name\)/);
  assert.doesNotMatch(text.split('\n').filter((line) => /run:/.test(line)).join('\n'), /github\.event/, 'no event data in a script');
  assert.match(text, /ref: main\n {10}fetch-depth: 0\n {10}persist-credentials: false/);
  assert.match(text, /^permissions:\n {2}contents: read/m);
  assert.match(text, / {4}permissions:\n {6}contents: write/);
  assert.match(text, / {4}concurrency:\n {6}group: plan\n {6}cancel-in-progress: false/);
  assert.deepEqual(text.match(/^jobs:\n((?: {2}\S.*\n|(?: {4,}.*)?\n)*)/m)[1].match(/^ {2}\S+:$/gm), ['  plan:'], 'one job');
});

test('plan.yml: pull --commit, check, push with the token given to that step only, then sync even when the pull failed', () => {
  const text = workflow('plan.yml');
  const steps = text.split(/\n {6}- /).slice(1);
  const runs = steps.map((step) => step.match(/run: (.+)$/m)?.[1]).filter(Boolean);
  assert.deepEqual(runs, ['tools/plan/bin/plan pull --commit', 'tools/plan/bin/plan check', 'tools/plan/bin/push-main', 'tools/plan/bin/plan sync']);
  const step = (command) => steps.find((s) => s.includes(`run: ${command}`));
  assert.deepEqual(steps.filter((s) => /github\.token|secrets\.GITHUB_TOKEN/.test(s)), [step('tools/plan/bin/push-main')]);
  assert.doesNotMatch(text.split('\n    steps:')[0], /github\.token|GITHUB_TOKEN/, 'not in the job\'s env');
  assert.match(step('tools/plan/bin/push-main'), /PUSH_TOKEN: \$\{\{ github\.token \}\}/);
  assert.match(step('tools/plan/bin/plan sync'), /if: \$\{\{ !cancelled\(\) \}\}/);
  for (const command of ['tools/plan/bin/plan pull --commit', 'tools/plan/bin/push-main', 'tools/plan/bin/plan sync']) {
    assert.match(step(command), /WORKSHOP_APP_ID: \$\{\{ secrets\.WORKSHOP_APP_ID \}\}/);
    assert.match(step(command), /WORKSHOP_PRIVATE_KEY: \$\{\{ secrets\.WORKSHOP_PRIVATE_KEY \}\}/);
  }
});

/** Runs a file without blocking the event loop, so the fake GitHub in this process can answer. */
function spawnFile(file, args, { cwd, env }) {
  return new Promise((resolve) => {
    const child = spawn(file, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    let err = '';
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (err += d));
    child.on('close', (code) => resolve({ code, out, err }));
  });
}

/** A synced world with the owner's take on fork, and the job's checkout of main with the verdict committed. */
async function job(t) {
  const w = await world(t, basePlan());
  assert.equal((await w.run(['sync'])).code, 0);
  w.fake.setVerdictLabel(w.fake.entry('fork').number, 'take', 'owner');
  const runner = join(w.base, 'runner');
  git(w.base, 'clone', '-q', w.origin, runner);
  const env = {
    PATH: process.env.PATH, HOME: w.home, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null',
    GIT_AUTHOR_NAME: 'Bot', GIT_AUTHOR_EMAIL: 'bot@example.invalid', GIT_COMMITTER_NAME: 'Bot', GIT_COMMITTER_EMAIL: 'bot@example.invalid',
    WORKSHOP_GITHUB_API: w.fake.url, WORKSHOP_REPO: w.fake.repo, WORKSHOP_APP_ID: w.fake.appId, WORKSHOP_PRIVATE_KEY: appKeys().privateKey, PLAN_SITE: 'http://127.0.0.1:9',
  };
  const pulled = await spawnFile(join(BIN, 'plan'), ['pull', '--commit'], { cwd: runner, env });
  assert.equal(pulled.code, 0, pulled.err);
  const pushMain = () => spawnFile(join(BIN, 'push-main'), [], { cwd: runner, env });
  const atOrigin = (id) => JSON.parse(git(w.origin, 'show', `main:plan/features/${id}.json`));
  return { w, pushMain, atOrigin };
}

test('push-main: a verdict on a main that did not move is pushed; with no commit there is nothing to push', async (t) => {
  const { w, pushMain, atOrigin } = await job(t);
  const pushed = await pushMain();
  assert.equal(pushed.code, 0, pushed.err);
  assert.equal(atOrigin('fork').verdict, 'take');
  assert.equal(git(w.origin, 'log', '-1', '--format=%s', 'main').trim(), 'Owner: take fork');
  const again = await pushMain();
  assert.equal(again.code, 0);
  assert.match(again.out, /nothing to push/);
});

test('push-main: on a main that moved with a change next to the verdict, it resets, pulls the verdict again, checks and pushes', async (t) => {
  const { w, pushMain, atOrigin } = await job(t);
  // The lines next to the verdict, changed on main meanwhile.
  const meanwhile = w.change({ fork: { suggest: { verdict: 'later', why: 'Changed meanwhile.' } } }, 'Meanwhile');
  const pushed = await pushMain();
  assert.equal(pushed.code, 0, pushed.err);
  assert.match(pushed.out, /main moved; pulling the verdicts again/);
  assert.deepEqual(git(w.origin, 'log', '--format=%s', '-2', 'main').trim().split('\n'), ['Owner: take fork', 'Meanwhile']);
  assert.equal(git(w.origin, 'rev-parse', 'main~1').trim(), meanwhile);
  assert.equal(atOrigin('fork').verdict, 'take');
  assert.equal(atOrigin('fork').suggest.why, 'Changed meanwhile.');
});

test('push-main: a push rejected while main did not move fails at once with git\'s own message', async (t) => {
  const { w, pushMain } = await job(t);
  const hook = join(w.origin, 'hooks', 'pre-receive');
  const tries = join(w.base, 'tries');
  writeFileSync(hook, `#!/bin/sh\necho try >> '${tries}'\necho "main is protected here" >&2\nexit 1\n`);
  chmodSync(hook, 0o755);
  const before = git(w.origin, 'rev-parse', 'main').trim();
  const pushed = await pushMain();
  assert.equal(pushed.code, 1);
  assert.match(pushed.err, /main is protected here/);
  assert.doesNotMatch(pushed.err + pushed.out, /three tries|pulling the verdicts again/);
  assert.equal(readFileSync(tries, 'utf8'), 'try\n', 'one push, no retry');
  assert.equal(git(w.origin, 'rev-parse', 'main').trim(), before);
});

test('push-main: a main that moved to a plan failing the check gets no verdict commit', async (t) => {
  const { w, pushMain } = await job(t);
  const path = join(w.root, 'plan', 'features', 'cli.json');
  writeFileSync(path, canonicalText(canonicalFeature({ ...JSON.parse(readFileSync(path, 'utf8')), needs: ['nobody'] })));
  git(w.root, 'commit', '-q', '-am', 'Broken meanwhile');
  git(w.root, 'push', '-q', 'origin', 'HEAD:main');
  const broken = git(w.origin, 'rev-parse', 'main').trim();
  const pushed = await pushMain();
  assert.notEqual(pushed.code, 0);
  assert.match(pushed.out, /main moved; pulling the verdicts again/);
  assert.equal(git(w.origin, 'rev-parse', 'main').trim(), broken, 'nothing pushed');
});
