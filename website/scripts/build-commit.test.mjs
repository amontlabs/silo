// The commit the Workshop's page names as the one it was built from (build-commit.mjs): HEAD, only when plan/ is
// exactly what HEAD holds, as far as git can tell.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildCommit } from './build-commit.mjs';

const GIT_ENV = {
  ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'A', GIT_AUTHOR_EMAIL: 'a@example.invalid', GIT_COMMITTER_NAME: 'A', GIT_COMMITTER_EMAIL: 'a@example.invalid',
};
const git = (dir, ...args) => execFileSync('git', ['-C', dir, ...args], { env: GIT_ENV, encoding: 'utf8' }).trim();

/** A repository whose one commit holds plan/order.json and plan/a.md. */
async function repository() {
  const dir = await mkdtemp(join(tmpdir(), 'build-commit-'));
  await mkdir(join(dir, 'plan'));
  await writeFile(join(dir, 'plan', 'order.json'), '{}\n');
  await writeFile(join(dir, 'plan', 'a.md'), 'A.\n');
  git(dir, 'init', '-q', '-b', 'main');
  git(dir, 'add', '-A');
  git(dir, 'commit', '-qm', 'Seed');
  return dir;
}

test('HEAD when plan/ is as HEAD holds it; none with a change, a new file, or another plan folder', async () => {
  const dir = await repository();
  try {
    const head = git(dir, 'rev-parse', 'HEAD');
    assert.equal(buildCommit(dir, join(dir, 'plan')), head);
    assert.equal(buildCommit(dir, join(dir, 'other')), null);
    await writeFile(join(dir, 'plan', 'b.md'), 'B.\n');
    assert.equal(buildCommit(dir, join(dir, 'plan')), null, 'a file git does not track');
    await rm(join(dir, 'plan', 'b.md'));
    await writeFile(join(dir, 'plan', 'a.md'), 'Changed.\n');
    assert.equal(buildCommit(dir, join(dir, 'plan')), null, 'a change');
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('none when a file of plan/ is marked assume-unchanged or skip-worktree: git would hide its edit', async () => {
  for (const flag of ['--assume-unchanged', '--skip-worktree']) {
    const dir = await repository();
    try {
      git(dir, 'update-index', flag, 'plan/a.md');
      await writeFile(join(dir, 'plan', 'a.md'), 'Edited, and git status says nothing.\n');
      assert.equal(git(dir, 'status', '--porcelain', '--', 'plan'), '', `git hides the edit (${flag})`);
      assert.equal(buildCommit(dir, join(dir, 'plan')), null, flag);
      // Even with the file as HEAD holds it: the flag alone means git cannot vouch for it.
      await writeFile(join(dir, 'plan', 'a.md'), 'A.\n');
      assert.equal(buildCommit(dir, join(dir, 'plan')), null, `${flag}, file unchanged`);
    } finally { await rm(dir, { recursive: true, force: true }); }
  }
});
