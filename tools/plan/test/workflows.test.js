// The two workflows cannot run before the owner creates the App; these tests hold what can be
// checked here: their triggers, history, permissions, group, and the push that retries.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cleanup, git, temp } from './helpers.js';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const workflow = (name) => readFileSync(join(ROOT, '.github', 'workflows', name), 'utf8');

test('plan-sync: on push to main, the whole history, group plan never cancelled, the two secrets', () => {
  const text = workflow('plan-sync.yml');
  assert.match(text, /on:\n {2}push:\n {4}branches: \[main\]/);
  assert.match(text, /fetch-depth: 0/);
  assert.match(text, / {4}concurrency:\n {6}group: plan\n {6}cancel-in-progress: false/);
  assert.match(text, /WORKSHOP_APP_ID: \$\{\{ secrets\.WORKSHOP_APP_ID \}\}/);
  assert.match(text, /WORKSHOP_PRIVATE_KEY: \$\{\{ secrets\.WORKSHOP_PRIVATE_KEY \}\}/);
  assert.match(text, /run: tools\/plan\/bin\/plan sync$/m);
  assert.doesNotMatch(text, /contents: write/);
});

test('plan-verdict: only verdict labels, main with its history, contents write, pull --commit, push, sync', () => {
  const text = workflow('plan-verdict.yml');
  assert.match(text, /issues:\n {4}types: \[labeled, unlabeled\]/);
  assert.match(text, /if: github\.event_name == 'workflow_dispatch' \|\| startsWith\(github\.event\.label\.name, 'verdict:'\)/);
  assert.doesNotMatch(text.split('\n').filter((line) => /run:/.test(line)).join('\n'), /github\.event/, 'no event data in a script');
  assert.match(text, /ref: main\n {10}fetch-depth: 0/);
  assert.match(text, / {4}permissions:\n {6}contents: write/);
  assert.match(text, / {4}concurrency:\n {6}group: plan\n {6}cancel-in-progress: false/);
  const steps = [...text.matchAll(/run: (.+)$/gm)].map((m) => m[1]);
  assert.deepEqual(steps, ['tools/plan/bin/plan pull --commit', 'tools/plan/bin/push-main', 'tools/plan/bin/plan sync']);
});

test('push-main rebases on a main that moved meanwhile and pushes', () => {
  const base = temp('plan-push-');
  try {
    const origin = join(base, 'origin.git');
    git(base, 'init', '-q', '--bare', '-b', 'main', origin);
    const clone = (name) => {
      const dir = join(base, name);
      git(base, 'clone', '-q', origin, dir);
      return dir;
    };
    const first = clone('first');
    writeFileSync(join(first, 'a'), 'a\n');
    git(first, 'add', 'a');
    git(first, 'commit', '-q', '-m', 'a');
    git(first, 'push', '-q', 'origin', 'HEAD:main');
    const second = clone('second');
    writeFileSync(join(first, 'b'), 'b\n');
    git(first, 'add', 'b');
    git(first, 'commit', '-q', '-m', 'b');
    git(first, 'push', '-q', 'origin', 'HEAD:main');
    writeFileSync(join(second, 'c'), 'c\n');
    git(second, 'add', 'c');
    git(second, 'commit', '-q', '-m', 'Owner: take c');
    const env = { ...process.env, GIT_AUTHOR_NAME: 'T', GIT_AUTHOR_EMAIL: 't@example.invalid', GIT_COMMITTER_NAME: 'T', GIT_COMMITTER_EMAIL: 't@example.invalid', GIT_CONFIG_GLOBAL: '/dev/null' };
    execFileSync(join(ROOT, 'tools', 'plan', 'bin', 'push-main'), { cwd: second, env, stdio: 'pipe' });
    assert.deepEqual(git(origin, 'log', '--format=%s', 'main').trim().split('\n'), ['Owner: take c', 'b', 'a']);
  } finally {
    cleanup(base);
  }
});
