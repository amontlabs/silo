// The commit the Workshop's page is built from, which the page sends with the owner's verdict: the server then
// refuses when the entry's file on main is not the one at that commit (server/verdicts.mjs). It is HEAD when the
// plan is this repository's plan/ and git sees no change under plan/; otherwise the page shows text no commit holds,
// and it has none, so the owner gets no verdict buttons.
import { execFileSync } from 'node:child_process';
import { join, resolve } from 'node:path';

const COMMIT = /^[0-9a-f]{40}$/;

/** @param {string} repository the top of the repository @param {string} planDir the plan the page is built from @returns {string | null} */
export function buildCommit(repository, planDir) {
  if (resolve(planDir) !== resolve(join(repository, 'plan'))) return null;
  const git = (...args) => execFileSync('git', ['-C', repository, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], timeout: 60_000 }).trim();
  try {
    const head = git('rev-parse', 'HEAD');
    if (!COMMIT.test(head)) return null;
    return git('-c', 'core.fsmonitor=false', 'status', '--porcelain', '--untracked-files=all', '--', 'plan') === '' ? head : null;
  } catch {
    return null;
  }
}
